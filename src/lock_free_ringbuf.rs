use std::{
    borrow::Cow,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError<T> {
    /// There's not enough space in the buffer to send the value.
    // #[error("Not enough space in the buffer to send the value.")]
    Full(T),
    /// The receiver has been dropped.
    Dropped(T),
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
        let layout = std::alloc::Layout::array::<T>(capacity).unwrap();
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

    /// Returns the number of continguous elements starting from the [`Receiver`]'s current position.
    ///
    /// In the following case, the length without wrap would be 5 since the values from `3` through
    /// `7` could be read contiguously:
    ///
    /// ```txt
    /// 8 _ _ 3 4 5 6 7
    ///       ^^^^^^^^^
    ///       ↑
    ///       Receiver position
    /// ```
    pub fn len_without_wrap(&self) -> usize {
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

    /// Retunrs whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // TODO: Instead maybe switch to these methods:
    // read -> 0 to n elements
    // read_exact -> exactly n elements, blocking until they are available
    // try_read_exact -> exactly n elements, non-blocking
    //
    // peek -> 0 to n elements
    // peek_exact -> exactly n elements, blocking until they are available or return an error if impossible
    // try_peek_exact -> exactly n elements, non-blocking, return an error if impossible
    //
    //
    // read_into -> read into a buffer
    // read_into_exact -> read exactly n elements into a buffer, blocking until they are available
    // try_read_into_exact -> read exactly n elements into a buffer, non-blocking
    // read_into_unchecked -> read exactly n elements into a buffer without checking the reader's length
    //
    // clear
    // remove -> remove up to n elements
    // remove_exact -> remove exactly n elements, blocking until they are available
    // try_remove_exact -> remove exactly n elements, non-blocking
    // remove_unchecked -> remove exactly n elements without checking the buffer's length

    /// Reads and removes up to `n` elements from the front of the buffer.
    ///
    /// Lock-free and non-blocking.
    // TODO: Check if the writer part has been dropped.
    pub fn read(&mut self, n: usize) -> Vec<T> {
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

    /// Reads and removes exactly `n` elements from the front of the buffer.
    ///
    /// Returns
    pub fn read_exact(&mut self, n: usize) -> Result<&[T], usize> {
        todo!()
    }

    // pub fn peek(&self, n: usize) -> (&[T], &[T]) {
    //     todo!()
    // }

    // pub fn read_into(&self, n: usize, buf: &mut Vec<T>) {
    //     todo!()
    // }

    /// Removes up to `n` elements from the front of the buffer.
    ///
    /// Returns the number of elements removed.
    pub fn remove(&mut self, n: usize) -> usize {
        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        let elems_to_remove = n.min(len);
        unsafe {
            self.remove_unchecked(elems_to_remove);
        }
        elems_to_remove
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
    /// Peeks at the next `n` elements in the buffer.
    ///
    /// If there are fewer than `n` elements in the buffer, all of them will be returned. The return
    /// value is [`Cow::Borrowed`] if the elements are contiguous in memory, else they are copied
    /// into a [`Vec`] and returned as [`Cow::Owned`] to make them contiguous.
    pub fn peek_cow(&self, n: usize) -> Cow<'_, [T]> {
        todo!()
    }

    /// Peeks at exactly `n` elements at the start of the buffer.
    ///
    /// If there are fewer than `n` elements in the buffer, their number is returned as an error.
    pub fn peek_cow_exact(&self, n: usize) -> Result<Cow<'_, [T]>, usize> {
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

        let layout = std::alloc::Layout::array::<T>(self.capacity).unwrap();
        unsafe {
            std::alloc::dealloc(self.buf.cast_mut().cast(), layout);
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
            // SAFETY: The writer has been dropped, so it's safe to drop `inner`.
            let inner = unsafe { Box::from_raw(self.inner.cast_mut()) };

            // Drop elements in the buffer. This handles T being a ZST as well.
            self.clear();

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
            if offset_from_start <= len {
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
