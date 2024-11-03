use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
        Arc,
    },
};

pub fn create_bounded<T>(capacity: usize) -> (Writer<T>, Reader<T>) {
    let inner = Inner::with_capacity(capacity);
    let buf_ptr = inner.buf;
    let inner = Box::into_raw(Box::new(inner));
    let writer = Writer {
        inner,
        write_pos: buf_ptr,
        counterpart_dropped: AtomicBool::new(false),
    };
    let reader = Reader {
        inner,
        read_pos: buf_ptr,
        counterpart_dropped: AtomicBool::new(false),
    };
    (writer, reader)
}

struct Inner<T> {
    buf: *const T,
    capacity: usize,
    len: AtomicUsize,
}

pub struct Reader<T> {
    inner: *const Inner<T>,
    // inner: AtomicPtr<Inner<T>>,
    /// Points to the next element to read.
    ///
    /// Always points into the buffer.
    read_pos: *const T,
    /// Is set if the [`Writer`] has been dropped.
    counterpart_dropped: AtomicBool,
}

pub struct Writer<T> {
    inner: *const Inner<T>,
    /// Points to the next element to write.
    // TODO: What happens at the end of the buffer? Point to the first element outside the buffer or
    //       to the first element in the buffer?
    write_pos: *const T,
    /// Is set if the [`Reader`] has been dropped.
    counterpart_dropped: AtomicBool,
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
                std::ptr::copy_nonoverlapping(self.read_pos, output.as_mut_ptr(), elems_to_read)
            };

            // TODO: how to handle and change read_pos when len == n?
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
            todo!();
        }
        output
    }

    pub fn read_exact(&self, n: usize) -> Result<&[T], ()> {
        todo!()
    }

    // pub fn read_into(&self, n: usize, buf: &mut Vec<T>) {
    //     todo!()
    // }

    /// Removes up to `n` elements from the front of the queue.
    ///
    /// Returns how many elements were removed.
    pub fn remove(&mut self, n: usize) -> usize {
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

impl<T> Drop for Reader<T> {
    fn drop(&mut self) {
        todo!()
    }
}

impl<T> Drop for Writer<T> {
    fn drop(&mut self) {
        todo!()
    }
}
