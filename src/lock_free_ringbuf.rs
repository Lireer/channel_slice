use std::{
    borrow::Cow,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub fn create_bounded<T>(capacity: usize) -> (Writer<T>, Reader<T>) {
    let inner = Inner::with_capacity(capacity);
    let buf_ptr = inner.buf;
    let inner = Box::into_raw(Box::new(inner));
    let writer = Writer {
        inner,
        write_pos: buf_ptr,
    };
    let reader = Reader {
        inner,
        read_pos: buf_ptr,
    };
    (writer, reader)
}

struct Inner<T> {
    buf: *const T,
    capacity: usize,
    len: AtomicUsize,
    /// Is set if either the [`Reader`] or [`Writer`] has been dropped.
    counterpart_dropped: AtomicBool,
}

pub struct Reader<T> {
    inner: *const Inner<T>,
    // inner: AtomicPtr<Inner<T>>,
    /// Points to the next element to read.
    ///
    /// Always points into the buffer.
    read_pos: *const T,
}

pub struct Writer<T> {
    inner: *const Inner<T>,
    /// Points to the next element to write.
    ///
    /// Always points into the buffer.
    write_pos: *const T,
}

impl<T> Inner<T> {
    /// Creates a new `Inner` with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is big enough to cause an the buffer's size to be bigger than
    /// `isize::MAX` if rounded up to the nearest multiple of `T`'s alignment.
    fn with_capacity(capacity: usize) -> Self {
        let layout = std::alloc::Layout::array::<T>(capacity).unwrap();
        let buf: *mut T = unsafe { std::alloc::alloc(layout) } as *mut T;
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

impl<T> Reader<T> {
    /// Returns the number of elements in the buffer.
    pub fn len(&self) -> usize {
        unsafe { &*self.inner }.len.load(Ordering::Acquire)
    }

    /// Returns the number of continguous elements starting from the [`Reader`]'s current position.
    ///
    /// In the following case, the length without wrap would be 5 since the values from `3` through
    /// `7` could be read contiguously:
    ///
    /// ```txt
    /// 8 _ _ 3 4 5 6 7
    ///       ^^^^^^^^^
    ///       ↑
    ///       Reader position
    /// ```
    pub fn len_without_wrap(&self) -> usize {
        let elements_to_end = unsafe { (*self.inner).end_of_buf().offset_from(self.read_pos) };
        assert!(
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
        assert!(
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
    pub fn read_exact(&self, n: usize) -> Result<&[T], usize> {
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
            assert!(
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
                        self.read_pos as *mut T,
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
                        self.read_pos as *mut T,
                        elems_to_end,
                    ))
                };

                let remaining = n - elems_to_end;

                // Drop the second part from the start of the buffer.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        (*self.inner).buf as *mut T,
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
    /// the [`Writer`] concurrently.
    pub fn clear(&mut self) {
        let len = unsafe { &*self.inner }.len.load(Ordering::Acquire);
        unsafe {
            self.remove_unchecked(len);
        }
    }
}

impl<T> Reader<T>
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

// TODO: impl std::io::Read for Receiver<u8>

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        // Check that all elements have been removed.
        assert_eq!(
            self.len.load(Ordering::Relaxed),
            0,
            "Not all elements have been removed."
        );

        let layout = std::alloc::Layout::array::<T>(self.capacity).unwrap();
        unsafe {
            std::alloc::dealloc(self.buf as *mut u8, layout);
        }
    }
}

impl<T> Drop for Reader<T> {
    fn drop(&mut self) {
        // Check if the writer has been dropped and set the flag if it hasn't.
        let counterpart_dropped = unsafe { &*self.inner }
            .counterpart_dropped
            .swap(true, Ordering::AcqRel);

        // If the writer has been dropped, it falls upon the reader to clean up the memory.
        if counterpart_dropped {
            // SAFETY: The writer has been dropped, so it's safe to drop `inner`.
            let inner = unsafe { Box::from_raw(self.inner as *mut Inner<T>) };

            // Drop elements in the buffer. This handles T being a ZST as well.
            self.clear();

            drop(inner);
        }
    }
}

impl<T> Drop for Writer<T> {
    fn drop(&mut self) {
        // Check if the reader has been dropped and set the flag if it hasn't.
        let counterpart_dropped = unsafe { &*self.inner }
            .counterpart_dropped
            .swap(true, Ordering::AcqRel);

        // If the reader has been dropped, it falls upon the writer to clean up the memory.
        if counterpart_dropped {
            // SAFETY: The reader has been dropped, so it's safe to drop `inner`.
            let inner = unsafe { Box::from_raw(self.inner as *mut Inner<T>) };

            // Nothing else to drop if T is a ZST.
            if std::mem::size_of::<T>() == 0 {
                return;
            }

            // Drop elements in the buffer.

            // Use `Acquire` here to make sure we get the latest value of the buffers length.
            let len = inner.len.load(Ordering::Acquire);
            let offset_from_start = unsafe { self.write_pos.offset_from(inner.buf) };
            assert!(
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
                        self.write_pos.sub(len) as *mut T,
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
                        inner.buf as *mut T,
                        offset_from_start,
                    ));
                }
                let remaining = len - offset_from_start;
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                        inner.end_of_buf().sub(remaining) as *mut T,
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
