use std::{
    borrow::Cow,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError<T> {
    /// There's not enough space in the buffer to send the value.
    #[error("Not enough space in the buffer to send the value.")]
    Full(T),
    /// The receiver has been dropped.
    #[error("The receiver has been dropped, no further values can be sent.")]
    Dropped(T),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecvError {
    #[error("The sender has been dropped, there are {0} elements that can still be received.")]
    Dropped(usize),
}

pub fn create_bounded<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Inner::with_capacity(capacity);
    let buf_ptr: *const T = inner.buf;
    let inner = Box::into_raw(Box::new(inner));
    let writer = Sender {
        inner,
        write_pos: buf_ptr.cast_mut(),
    };
    let reader = Receiver {
        inner,
        read_pos: buf_ptr,
    };
    (writer, reader)
}

struct Inner<T> {
    buf: *const T,
    capacity: usize,
    len: AtomicUsize,
    /// Is set if either the [`Receiver`] or [`Sender`] has been dropped.
    counterpart_dropped: AtomicBool,
}

pub struct Receiver<T> {
    inner: *const Inner<T>,
    // inner: AtomicPtr<Inner<T>>,
    /// Points to the next element to read.
    ///
    /// Always points into the buffer.
    read_pos: *const T,
}

pub struct Sender<T> {
    inner: *const Inner<T>,
    /// Points to the next element to write.
    ///
    /// Always points into the buffer and as long as the buffer is not full, `write_pos` can always
    /// be written to.
    write_pos: *mut T,
}

// FIXME: Explain why these are safe and if they are even safe.
unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Sync> Sync for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}
unsafe impl<T: Sync> Sync for Receiver<T> {}

impl<T> Inner<T> {
    /// Creates a new `Inner` with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is big enough to cause an the buffer's size to be bigger than
    /// `isize::MAX` if rounded up to the nearest multiple of `T`'s alignment.
    fn with_capacity(capacity: usize) -> Self {
        let layout = Self::layout(capacity);
        let buf: *mut T = unsafe { std::alloc::alloc(layout) }.cast();
        if buf.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        Self {
            buf,
            capacity,
            len: AtomicUsize::new(0),
            counterpart_dropped: AtomicBool::new(false),
        }
    }

    fn layout(capacity: usize) -> std::alloc::Layout {
        std::alloc::Layout::array::<T>(capacity).unwrap()
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns a pointer to the first element after the buffer.
    fn end_of_buf(&self) -> *const T {
        unsafe { self.buf.add(self.capacity) }
    }
}

impl<T> Receiver<T> {
    /// Returns the number of elements in the buffer.
    pub fn len(&self) -> usize {
        unsafe { &*self.inner }.len.load(Ordering::Acquire)
    }

    /// Returns the number of continguous elements starting from the [`Receiver`]'s current
    /// position.
    ///
    /// In the following case, the number of continuous elements would be 5 since the values from
    /// `3` through `7` could be read contiguously:
    ///
    /// ```txt
    /// 8 _ _ 3 4 5 6 7
    ///       ^^^^^^^^^
    ///       ↑
    ///       Receiver position
    /// ```
    pub fn contiguous_len(&self) -> usize {
        let elements_to_end = unsafe { (*self.inner).end_of_buf().offset_from(self.read_pos) };
        debug_assert!(
            elements_to_end >= 0,
            "elements_to_end is negative ({}), end_of_buf: {:p}, read_pos: {:p}",
            elements_to_end,
            unsafe { &*self.inner }.end_of_buf(),
            self.read_pos
        );
        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        len.min(elements_to_end as usize)
    }

    /// Returns whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // The following methods have to:
    // - Handle ZSTs
    // - Drop elements correctly
    // - Check if the writer has been dropped
    //
    // recv -> exactly n elements, blocking until they are available
    // recv_up_to -> 0 to n elements
    // try_recv -> exactly n elements, non-blocking
    // recv_unchecked -> exactly n elements without checking the buffer's length
    //
    // recv_into -> read exactly n elements into a buffer, blocking until they are available
    // recv_into_up_to -> read into a buffer
    // try_recv_into -> read exactly n elements into a buffer, non-blocking
    // recv_into_unchecked -> read exactly n elements into a buffer without checking the reader's length
    //
    // peek_cow -> exactly n elements, blocking until they are available or return an error if impossible
    // peek_cow_up_to -> 0 to n elements
    // try_peek_cow -> exactly n elements, non-blocking, return an error if impossible
    //
    // clear
    // remove -> remove exactly n elements, blocking until they are available
    // remove_up_to -> remove up to n elements
    // try_remove -> remove exactly n elements, non-blocking
    // remove_unchecked -> remove exactly n elements without checking the buffer's length

    /// Reads and removes exactly `n` elements from the front of the buffer.
    ///
    /// Blocks until there are at least `n` elements available.
    pub fn recv(&mut self, n: usize) -> Result<Vec<T>, ()> {
        todo!()
    }

    /// Reads and removes up to `n` elements from the front of the buffer.
    ///
    /// Lock-free and non-blocking.
    // TODO: Check if the writer part has been dropped.
    pub fn recv_up_to(&mut self, n: usize) -> Vec<T> {
        let end_of_buf = unsafe { &*self.inner }.end_of_buf();
        // There are two scenarios to consider:
        // 1. The next n elements are contiguous in memory.
        // 2. The next n elements wrap around at the end of the buffer.
        let len = (unsafe { &*self.inner }).len.load(Ordering::Acquire);

        // Read at most `n` elements.
        let elems_to_read = n.min(len);
        if elems_to_read == 0 {
            return Vec::new();
        }

        let elems_to_end = unsafe { end_of_buf.offset_from(self.read_pos) };
        debug_assert!(
            elems_to_end >= 0,
            "elems_to_end is negative ({}), end_of_buf: {:p}, read_pos: {:p}",
            elems_to_end,
            end_of_buf,
            self.read_pos
        );
        let elems_to_end = elems_to_end as usize;

        let mut output: Vec<T> = Vec::with_capacity(elems_to_read);
        if elems_to_end >= elems_to_read {
            // Case 1: The next n elements are contiguous in memory.

            // SAFETY: We know that there are at least `elems_to_read` contiguous elements starting
            // from `self.read_pos`.
            unsafe {
                std::ptr::copy_nonoverlapping(self.read_pos, output.as_mut_ptr(), elems_to_read);
                output.set_len(elems_to_read);
            };

            self.read_pos = unsafe { self.read_pos.add(elems_to_read) };
            if self.read_pos == end_of_buf {
                // Wrap around at the end of the buffer.
                self.read_pos = unsafe { &*self.inner }.buf;
            }
            unsafe { &*self.inner }
                .len
                .fetch_sub(elems_to_read, Ordering::Release);
        } else {
            // Case 2: The next n elements wrap around at the end of the buffer.

            // Copy the first part up to the end of the buffer.
            unsafe {
                std::ptr::copy_nonoverlapping(self.read_pos, output.as_mut_ptr(), elems_to_end);
            }

            let remaining = elems_to_read - elems_to_end;

            // Copy the second part from the start of the buffer.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (*self.inner).buf,
                    output.as_mut_ptr().add(elems_to_end),
                    remaining,
                );
                output.set_len(elems_to_read);
            }

            self.read_pos = unsafe { (*self.inner).buf.add(remaining) };
        }
        output
    }

    /// Tries to read and remove exactly `n` elements from the front of the buffer.
    ///
    /// Returning immediately with an error containing the number of currently available elements,
    /// if there are fewer than `n` elements in the buffer.
    pub fn try_recv(&mut self, n: usize) -> Result<Vec<T>, usize> {
        todo!()
    }

    /// Reads and removes exactly `n` elements from the front of the buffer without checking the
    /// buffer's length.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `n <= self.len()`, else uninitialized memory will be read.
    ///
    pub unsafe fn recv_unchecked(&mut self, n: usize) -> Result<Vec<T>, RecvError> {
        todo!()
    }

    /// Reads and removes `n` elements from the front of the buffer and appends them to `buf`, blocking
    ///
    /// Blocks until there are `n` elements available.
    pub fn recv_into(&mut self, n: usize, buf: &mut Vec<T>) {
        todo!()
    }

    /// Reads and removes up to `n` elements from the front of the buffer and appends them to
    /// `buf`.
    ///
    /// Returns the number of elements removed.
    ///
    /// Lock-free and non-blocking.
    pub fn recv_into_up_to(&mut self, n: usize, buf: &mut Vec<T>) -> usize {
        todo!()
    }

    /// Tries to read and remove `n` elements from the front of the buffer and append them to `buf`.
    ///
    /// Returns immediately with an error containing the number of currently available elements, if
    /// there are fewer than `n` elements in the buffer.
    pub fn try_recv_into(&mut self, n: usize, buf: &mut Vec<T>) -> Result<(), usize> {
        todo!()
    }

    /// Reads and removes `n` elements from the front of the buffer and appends them to `buf` without
    /// checking the buffer's length.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `n <= self.len()`, else uninitialized memory will be read.
    pub unsafe fn recv_into_unchecked(
        &mut self,
        n: usize,
        buf: &mut Vec<T>,
    ) -> Result<(), RecvError> {
        todo!()
    }

    // pub fn peek(&self, n: usize) -> (&[T], &[T]) {
    //     todo!()
    // }

    /// Removes `n` elements from the front of the buffer, blocking if there are fewer than `n` elements.
    pub fn remove(&mut self, n: usize) -> Result<(), RecvError> {
        todo!()
    }

    /// Removes up to `n` elements from the front of the buffer.
    ///
    /// Returns the number of elements removed.
    pub fn remove_up_to(&mut self, n: usize) -> Result<usize, RecvError> {
        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        if len == 0 {
            return Err(RecvError::Dropped(0));
        }

        let elems_to_remove = n.min(len);
        unsafe {
            self.remove_unchecked(elems_to_remove);
        }
        Ok(elems_to_remove)
    }

    /// Tries to remove `n` elements from the front of the buffer.
    ///
    /// Returns immediately with an error containing the number of currently available elements, if
    /// there are fewer than `n` elements in the buffer.
    pub fn try_remove(&mut self, n: usize) -> Result<(), usize> {
        todo!()
    }

    /// Removes `n` elements from the front of the buffer wihtout checking the buffer's length.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `n <= self.len()`, else uninitialized memory will be read.
    pub unsafe fn remove_unchecked(&mut self, n: usize) {
        // Handle ZSTs or rather, don't handle them.
        if std::mem::size_of::<T>() != 0 {
            let end_of_buf = unsafe { &*self.inner }.end_of_buf();
            let elems_to_end = unsafe { end_of_buf.offset_from(self.read_pos) };
            debug_assert!(
                elems_to_end >= 0,
                "elems_to_end is negative ({}), end_of_buf: {:p}, read_pos: {:p}",
                elems_to_end,
                end_of_buf,
                self.read_pos
            );
            let elems_to_end = elems_to_end as usize;

            if elems_to_end >= n {
                // Case 1: The next n elements are contiguous in memory.

                // SAFETY: We know that there are at least `n` contiguous elements starting from
                // `self.read_pos`.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        self.read_pos.cast_mut(),
                        n,
                    ))
                };

                self.read_pos = unsafe { self.read_pos.add(n) };
                if self.read_pos == end_of_buf {
                    // Wrap around at the end of the buffer.
                    self.read_pos = unsafe { &*self.inner }.buf;
                }
            } else {
                // Case 2: The next n elements wrap around at the end of the buffer.

                // Drop the first part up to the end of the buffer.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        self.read_pos.cast_mut(),
                        elems_to_end,
                    ))
                };

                let remaining = n - elems_to_end;

                // Drop the second part from the start of the buffer.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        (*self.inner).buf.cast_mut(),
                        remaining,
                    ))
                };

                self.read_pos = unsafe { (*self.inner).buf.add(remaining) };
            }
        }

        let previous_len = unsafe { &*self.inner }.len.fetch_sub(n, Ordering::AcqRel);
        debug_assert!(
            previous_len >= n,
            "previous_len: {}, n: {}",
            previous_len,
            n
        );
    }

    /// Removes all elements from the buffer.
    ///
    /// Does **not** guarantee that `self.len() == 0` after the call, since elements may be added by
    /// the [`Sender`] concurrently.
    pub fn clear(&mut self) {
        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        unsafe {
            self.remove_unchecked(len);
        }
    }
}

impl<T> Receiver<T>
where
    T: Copy,
{
    /// Peeks at the next `n` elements of the buffer, blocking if there are fewer than `n` elements.
    ///
    /// The return value is [`Cow::Borrowed`] if the elements are contiguous in memory, else they
    /// are copied into a [`Vec`] and returned as [`Cow::Owned`] to make them contiguous.
    pub fn peek_cow(&self, n: usize) -> Result<Cow<'_, [T]>, usize> {
        todo!()
    }

    /// Peeks at the next `n` elements in the buffer.
    ///
    /// If there are fewer than `n` elements in the buffer, all of them will be returned. The return
    /// value is [`Cow::Borrowed`] if the elements are contiguous in memory, else they are copied
    /// into a [`Vec`] and returned as [`Cow::Owned`] to make them contiguous.
    pub fn peek_cow_up_to(&self, n: usize) -> Cow<'_, [T]> {
        todo!()
    }

    /// Tries to peek at the next `n` elements at the start of the buffer.
    ///
    /// Returns immediately with an error containing the number of currently available elements, if
    /// there are fewer than `n` elements in the buffer. The return value is [`Cow::Borrowed`] if
    /// the elements are contiguous in memory, else they are copied into a [`Vec`] and returned as
    /// [`Cow::Owned`] to make them contiguous.
    pub fn try_peek_cow(&self, n: usize) -> Result<Cow<'_, [T]>, usize> {
        todo!()
    }
}

impl<T> Sender<T> {
    pub fn len(&self) -> usize {
        unsafe { &*self.inner }.len.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        unsafe { &*self.inner }.capacity
    }

    pub fn space_to_end(&self) -> usize {
        let end_of_buf = unsafe { &*self.inner }.end_of_buf();
        let space_to_end = unsafe { end_of_buf.offset_from(self.write_pos) };
        debug_assert!(
            space_to_end >= 0,
            "space_to_end is negative ({}), end_of_buf: {:p}, write_pos: {:p}",
            space_to_end,
            end_of_buf,
            self.write_pos
        );
        space_to_end as usize
    }

    fn receiver_dropped(&self) -> bool {
        unsafe { &*self.inner }
            .counterpart_dropped
            .load(Ordering::Acquire)
    }

    /// Sends a value to the buffer, blocking until there is space.
    ///
    /// Also returns if the receiver has been dropped.
    ///
    /// blocking, lock-free
    // pub fn send(&mut self, t: T) -> Result<(), SendError<T>> {
    //     todo!()
    // }

    /// Sends a value to the buffer, immediately returning if there is no space.
    ///
    /// non-blocking and lock-free
    pub fn try_send(&mut self, t: T) -> Result<(), SendError<T>> {
        if self.receiver_dropped() {
            return Err(SendError::Dropped(t));
        }

        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        let capacity = unsafe { &*self.inner }.capacity;
        if len == capacity {
            return Err(SendError::Full(t));
        }

        // SAFETY: We know that there is space in the buffer.
        unsafe {
            std::ptr::write(self.write_pos, t);
        }

        self.write_pos = unsafe { self.write_pos.add(1) };
        if self.write_pos as *const _ == unsafe { &*self.inner }.end_of_buf() {
            // Wrap around at the end of the buffer.
            self.write_pos = unsafe { &*self.inner }.buf.cast_mut();
        }

        unsafe { &*self.inner }.len.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Sends a slice to the buffer, immediately returning `vec` if there is no space.
    // non-blocking, lock-free
    pub fn try_send_vec(&mut self, mut vec: Vec<T>) -> Result<(), SendError<Vec<T>>> {
        if self.receiver_dropped() {
            return Err(SendError::Dropped(vec));
        }

        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        let capacity = unsafe { &*self.inner }.capacity;
        let space_left = capacity - len;

        if vec.len() > space_left {
            return Err(SendError::Full(vec));
        }

        let mut written = vec.len().min(self.space_to_end());
        unsafe { std::ptr::copy_nonoverlapping(vec.as_ptr(), self.write_pos, written) };

        // If less has been written than is in the vec...
        if written < vec.len() {
            // ... write whatever remains starting from the start of the buf.
            let remaining = vec.len() - written;
            // SAFETY: We know there is enough space, because of the above check that guarantees
            //         the vec\s length to be less or equal to `capacity - len`.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    vec.as_ptr().add(written),
                    (*self.inner).buf.cast_mut(),
                    remaining,
                );
            }
            written += remaining;
            self.write_pos = unsafe { (*self.inner).buf.cast_mut().add(remaining) };
        } else if self.write_pos as *const _ == unsafe { &*self.inner }.end_of_buf() {
            // Wrap around at the end of the buffer.
            self.write_pos = unsafe { &*self.inner }.buf.cast_mut();
        }

        // Set the vec's length to 0 to prevent it from dropping the elements.
        unsafe { vec.set_len(0) };
        drop(vec);

        unsafe { &*self.inner }
            .len
            .fetch_add(written, Ordering::AcqRel);

        Ok(())
    }

    // non-blocking, lock-free
    pub fn try_send_iter<I>(&mut self, iter: I) -> Result<(), SendError<I::IntoIter>>
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        if self.receiver_dropped() {
            return Err(SendError::Dropped(iter.into_iter()));
        }

        let mut iter = iter.into_iter();
        let iter_len = iter.len();

        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        let capacity = unsafe { &*self.inner }.capacity;
        let space_left = capacity - len;

        if iter_len > space_left {
            return Err(SendError::Full(iter));
        }

        let write_to_end = iter_len.min(self.space_to_end());
        for offset in 0..write_to_end {
            unsafe {
                std::ptr::write(self.write_pos.add(offset), iter.next().unwrap());
            }
        }

        // If there's anything left in the iterator ...
        if iter.len() > 0 {
            // ... write whatever remains starting from the start of the buf.
            self.write_pos = unsafe { &*self.inner }.buf.cast_mut();
            for t in iter {
                // SAFETY: We know that there is space in the buffer.
                unsafe {
                    std::ptr::write(self.write_pos, t);
                }

                self.write_pos = unsafe { self.write_pos.add(1) };
            }
        } else {
            // There's nothing left to write, just update the write_pos to its final position.
            self.write_pos = unsafe { self.write_pos.add(write_to_end) };
            if self.write_pos as *const _ == unsafe { &*self.inner }.end_of_buf() {
                // Wrap around at the end of the buffer.
                self.write_pos = unsafe { &*self.inner }.buf.cast_mut();
            }
        }

        let prev_len = unsafe { &*self.inner }
            .len
            .fetch_add(iter_len, Ordering::AcqRel);
        debug_assert!(prev_len + iter_len <= capacity);

        Ok(())
    }
}

// TODO: impl std::io::Read for Receiver<u8>
// TODO: impl std::io::Write for Sender<u8>

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        // Check that all elements have been removed.
        debug_assert_eq!(
            self.len.load(Ordering::Relaxed),
            0,
            "Not all elements have been removed."
        );

        unsafe {
            std::alloc::dealloc(self.buf.cast_mut().cast(), Self::layout(self.capacity()));
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // Check if the writer has been dropped and set the flag if it hasn't.
        let counterpart_dropped = unsafe { &*self.inner }
            .counterpart_dropped
            .swap(true, Ordering::AcqRel);

        // If the writer has been dropped, it falls upon the reader to clean up the memory.
        if counterpart_dropped {
            // Drop elements in the buffer. This handles T being a ZST as well.
            self.clear();

            // SAFETY: The writer has been dropped and all elements have been removed, so it's safe
            //         to drop `inner`.
            let inner = unsafe { Box::from_raw(self.inner.cast_mut()) };

            drop(inner);
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Check if the reader has been dropped and set the flag if it hasn't.
        let counterpart_dropped = unsafe { &*self.inner }
            .counterpart_dropped
            .swap(true, Ordering::AcqRel);

        // If the reader has been dropped, it falls upon the writer to clean up the memory.
        if counterpart_dropped {
            // SAFETY: The reader has been dropped, so it's safe to drop `inner`.
            let inner = unsafe { Box::from_raw(self.inner.cast_mut()) };

            // Nothing else to drop if T is a ZST.
            if std::mem::size_of::<T>() == 0 {
                return;
            }

            // Drop elements in the buffer.

            // Use `Acquire` here to make sure we get the latest value of the buffers length.
            let len = inner.len.load(Ordering::Acquire);
            let offset_from_start = unsafe { self.write_pos.offset_from(inner.buf) };
            debug_assert!(
                offset_from_start >= 0,
                "offset_from_start is negative ({}), write_pos: {:p}, buf: {:p}",
                offset_from_start,
                self.write_pos,
                inner.buf
            );
            let offset_from_start = offset_from_start as usize;

            if offset_from_start >= len {
                // Case 1: All elements are contiguous in memory.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        self.write_pos.sub(len),
                        len,
                    ));
                }
                // Reduce length so it passes the check in `Inner::drop`. Use `Relaxed` here since
                // we're not racing with any other threads and already synchronized with the reader.
                inner.len.fetch_sub(len, Ordering::Relaxed);
            } else {
                // Case 2: Elements are split into two contiguous slices at the end of the buffer.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        inner.buf.cast_mut(),
                        offset_from_start,
                    ));
                }
                let remaining = len - offset_from_start;
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        inner.end_of_buf().sub(remaining).cast_mut(),
                        remaining,
                    ));
                }
                // Reduce length so it passes the check in `Inner::drop`. Use `Relaxed` here since
                // we're not racing with any other threads and already synchronized with the reader.
                inner
                    .len
                    .fetch_sub(offset_from_start + remaining, Ordering::Relaxed);
            }

            // Drop `inner` explicitly and let it take care of freeing the buffer's memory.
            drop(inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use testresult::TestResult;

    use super::*;

    #[test]
    fn create_bounded_works() {
        let (sender, receiver) = create_bounded::<i32>(4);
        assert_eq!(sender.capacity(), 4);
        assert_eq!(receiver.len(), 0);
    }

    #[test]
    fn drop_sender_then_receiver() {
        let (sender, receiver) = create_bounded::<i32>(4);
        drop(sender);
        assert_eq!(receiver.len(), 0);
        drop(receiver);
    }

    #[test]
    fn drop_receiver_then_sender() {
        let (sender, receiver) = create_bounded::<i32>(4);
        drop(receiver);
        assert_eq!(sender.len(), 0);
        drop(sender);
    }

    #[test]
    fn send_fails_after_receiver_drop() -> TestResult {
        let (mut sender, receiver) = create_bounded(4);
        sender.try_send(0)?;
        drop(receiver);

        assert_eq!(sender.try_send(1), Err(SendError::Dropped(1)));
        assert_eq!(
            sender.try_send_vec(vec![2, 3]),
            Err(SendError::Dropped(vec![2, 3]))
        );

        let err = sender.try_send_iter(vec![4, 5].into_iter());
        let Err(SendError::Dropped(iter)) = err else {
            panic!("Didn't error because the receiver was dropped");
        };
        assert_eq!(iter.collect::<Vec<_>>(), vec![4, 5]);

        assert_eq!(sender.len(), 1);

        Ok(())
    }

    #[test]
    fn elements_dropped_correctly() -> TestResult {
        let (mut sender, mut receiver) = create_bounded(4);

        let elems: Vec<Arc<_>> = [0, 1, 2, 3, 4, 5].into_iter().map(Arc::new).collect();

        sender.try_send_iter(elems[0..4].iter().cloned())?;
        assert!(elems[0..4].iter().all(|e| Arc::strong_count(e) == 2));

        receiver.recv_up_to(2);
        sender.try_send_iter(elems[4..].iter().cloned())?;
        assert!(elems[0..2].iter().all(|e| Arc::strong_count(e) == 1));
        assert!(elems[2..].iter().all(|e| Arc::strong_count(e) == 2));

        drop(receiver);
        assert!(elems[2..].iter().all(|e| Arc::strong_count(e) == 2));
        drop(sender);
        assert!(elems.iter().all(|e| Arc::strong_count(e) == 1));

        Ok(())
    }

    #[test]
    fn send_and_receive_entire_buffer() -> TestResult {
        let (mut sender, mut receiver) = create_bounded(3);
        sender.try_send_vec(vec![0, 1, 2])?;
        assert_eq!(receiver.recv_up_to(3), vec![0, 1, 2]);
        assert_eq!(receiver.len(), 0);
        assert_eq!(sender.len(), 0);

        Ok(())
    }

    #[test]
    fn failure_from_previous_impl() -> TestResult {
        let (mut s, mut r) = create_bounded::<u32>(3);
        // _ _ _
        s.try_send(0)?;
        // 0 _ _
        s.try_send(1)?;
        // 0 1 _
        s.try_send(2)?;
        // 0 1 2   len: 3
        assert_eq!(r.recv_up_to(1), vec![0]);
        // _ 1 2   len: 2
        s.try_send(3)?;
        // 3 1 2   len: 3
        assert_eq!(r.recv_up_to(1), vec![1]);
        // 3 _ 2   len: 2
        s.try_send(4)?;
        // 3 4 2   len: 3
        assert_eq!(r.recv_up_to(1), vec![2]);
        // 3 4 _   len: 2
        assert_eq!(r.recv_up_to(1), vec![3]);

        Ok(())
    }

    #[test]
    fn try_send_into_full_buffer_returns_error() -> TestResult {
        let (mut sender, _receiver) = create_bounded(2);

        // Fill the entire buffer
        sender.try_send(0)?;
        sender.try_send(1)?;
        assert_eq!(sender.len(), 2);

        // Test `try_send`
        let err = sender.try_send(3);
        assert_eq!(err, Err(SendError::Full(3)));

        Ok(())
    }

    #[test]
    fn try_send_vec_into_full_buffer_returns_error() -> TestResult {
        let (mut sender, _receiver) = create_bounded(2);

        // Fill the entire buffer
        sender.try_send(0)?;
        assert_eq!(sender.len(), 1);

        // Test `try_send_vec`
        let vec = vec![2, 3];
        let err = sender.try_send_vec(vec.clone());
        assert_eq!(err, Err(SendError::Full(vec)));

        Ok(())
    }

    #[test]
    fn try_send_iter_into_full_buffer_returns_error() -> TestResult {
        let (mut sender, _receiver) = create_bounded(2);

        // Fill the entire buffer
        sender.try_send(0)?;
        assert_eq!(sender.len(), 1);

        // Test `try_send_iter`
        let vec = vec![2, 3];
        let err = sender.try_send_iter(vec.clone()).unwrap_err();
        let SendError::Full(iter) = err else {
            panic!("Didn't error because the buffer is full");
        };
        assert_eq!(iter.into_iter().collect::<Vec<_>>(), vec);

        Ok(())
    }
}
