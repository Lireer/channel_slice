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
    // pub fn len(&self) -> usize {
    //     todo!()
    // }

    // pub fn peek(&self, n: usize) -> (&[T], &[T]) {
    //     todo!()
    // }

    /// Reads and removes up to `n` elements from the front of the queue.
    ///
    /// Lock-free and non-blocking.
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
                    (&*self.inner).buf,
                    output.as_mut_ptr().add(elems_to_end),
                    remaining,
                );
                output.set_len(elems_to_read);
            }

            self.read_pos = unsafe { (&*self.inner).buf.add(remaining) };
        }
        output
    }

    pub fn read_exact(&self, n: usize) -> Result<&[T], ()> {
        todo!()
    }

    // pub fn read_into(&self, n: usize, buf: &mut Vec<T>) {
    //     todo!()
    // }

    /// Removes and drops up to `n` elements from the front of the queue.
    ///
    /// Returns how many elements were removed.
    pub fn remove(&mut self, n: usize) -> usize {
        // Drop elements
        todo!()
    }
}

impl<T> Reader<T>
where
    T: Copy,
{
    /// Peeks at the next `n` elements in the queue.
    ///
    /// If there are fewer than `n` elements in the queue, all of them will be returned. The return
    /// value is [`Cow::Borrowed`] if the elements are contiguous in memory, else they are copied
    /// into a [`Vec`] and returned as [`Cow::Owned`] to make them contiguous.
    pub fn peek_cow(&self, n: usize) -> Cow<'_, [T]> {
        todo!()
    }

    /// Peeks at exactly `n` elements at the start of the queue.
    pub fn peek_cow_exact(&self, n: usize) -> Result<Cow<'_, [T]>, ()> {
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
            unsafe {
                _ = Box::from_raw(self.inner as *mut Inner<T>);
            }
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

            // TODO: Nothing else to drop if T is a ZST.
            if std::mem::size_of::<T>() == 0 {
                return;
            }

            // Drop elements in the buffer.

            // Can use `Relaxed` here since we're the only with access to `inner` at this point.
            let len = inner.len.load(Ordering::Relaxed);
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
                // Reduce length so it passes the check in `Inner::drop`.
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
                // Reduce length so it passes the check in `Inner::drop`.
                inner
                    .len
                    .fetch_sub(offset_from_start + remaining, Ordering::Relaxed);
            }

            // Drop `inner` explicitly and let it take care of freeing the buffer's memory.
            drop(inner);
        }
    }
}
