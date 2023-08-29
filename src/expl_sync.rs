use std::{
    alloc::Layout,
    sync::{
        atomic::{AtomicPtr, AtomicUsize, Ordering},
        Arc,
    },
};

/// Only for types for which `mem::needs_drop` returns false.
#[derive(Debug)]
pub struct SliceBuf<T> {
    capacity: usize,
    data_layout: Layout,
    write_offset: AtomicUsize,
    read_offset: AtomicUsize,
    data_start: AtomicPtr<T>,
    next: (AtomicPtr<Self>, AtomicUsize),
}

impl<T> SliceBuf<T> {
    pub fn new() -> Self {
        Self::with_capacity(4)
    }

    // pub(crate) fn len(&self) -> usize {
    //     // TOOD: Handle multiple allocations and potential data races.
    //     self.write_offset.load(Ordering::Acquire) - self.read_offset.load(Ordering::Acquire)
    // }

    pub fn remaining_capacity(&self) -> usize {
        // TOOD: Handle multiple allocations and potential data races.
        self.capacity - self.write_offset.load(Ordering::Relaxed)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let size_of_t = std::mem::size_of::<T>();
        let align_of_t = std::mem::align_of::<T>();
        assert_ne!(capacity, 0, "capacity is 0 but must be at least 1");
        assert_ne!(size_of_t, 0, "zero sized types are currenty not supported");
        assert!(
            !std::mem::needs_drop::<T>(),
            "types that need to be dropped are currently not supported"
        );

        let data_layout = Layout::from_size_align(size_of_t * capacity, align_of_t).unwrap();
        let data_start = unsafe { std::alloc::alloc(data_layout) };
        if data_start.is_null() {
            // Abort if allocation failed, see `alloc` function for more information.
            std::alloc::handle_alloc_error(data_layout);
        }

        Self {
            capacity,
            data_layout,
            data_start: AtomicPtr::from(data_start.cast()),
            write_offset: AtomicUsize::new(0),
            read_offset: AtomicUsize::new(0),
            next: (AtomicPtr::new(std::ptr::null_mut()), AtomicUsize::new(0)),
        }
    }

    pub fn split(self) -> (SliceBufWriter<T>, SliceBufReader<T>) {
        let shared = Arc::new(self);
        (
            SliceBufWriter {
                shared: shared.clone(),
            },
            SliceBufReader { shared },
        )
    }
}

impl<T> Default for SliceBuf<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for SliceBuf<T> {
    fn drop(&mut self) {
        // TODO: Drop elements between read_offset and write_offset
        assert!(!std::mem::needs_drop::<T>());

        // Deallocate the memory used for this buffer.
        unsafe { std::alloc::dealloc(self.data_start.get_mut().cast(), self.data_layout) }

        // If a next allocation exists, try to drop that too.
        let next = self.next.0.load(Ordering::Relaxed);
        if !next.is_null() {
            _ = unsafe { Arc::from_raw(next) };
        }
    }
}

#[derive(Debug)]
pub struct SliceBufReader<T> {
    shared: Arc<SliceBuf<T>>,
}

impl<T> SliceBufReader<T> {
    pub fn synchronize(&mut self) {
        loop {
            let next_buf = self
                .shared
                .next
                .0
                .swap(std::ptr::null_mut(), Ordering::Acquire);

            if next_buf.is_null() {
                // No new instance to worry about.
                break;
            }

            // There's a new SliceBuf instance
            let next = unsafe { Arc::from_raw(next_buf) };
            // Get read_offset that was used when creating the new SliceBuf.
            let used_read_offset = self.shared.next.1.load(Ordering::Acquire);
            let read_offset_diff =
                self.shared.read_offset.load(Ordering::Relaxed) - used_read_offset;

            // Update read_offset in next SliceBuf to reflect possible changes after its creation.
            // TODO: Could this be relaxed?
            next.read_offset.store(read_offset_diff, Ordering::Release);
            self.shared = next;
        }
    }

    pub fn slice_to(&self, to: usize) -> Option<&[T]> {
        let read_offset = self.shared.read_offset.load(Ordering::Relaxed);
        let write_offset = self.shared.write_offset.load(Ordering::Acquire);
        let len = write_offset - read_offset;

        if to > len {
            return None;
        }

        let read_start = unsafe {
            self.shared
                .data_start
                .load(Ordering::Acquire)
                .add(read_offset)
        };

        Some(unsafe { std::slice::from_raw_parts(read_start, to) })
    }

    pub fn consume(&mut self, n: usize) {
        // TODO: How should this behave if multiple allocations exist?
        // TODO: Maybe change ordering if writer also accesses read_offset.

        let old_val = self.shared.read_offset.fetch_add(n, Ordering::Release);

        let write_offset = self.shared.write_offset.load(Ordering::Acquire);
        if old_val + n > write_offset {
            panic!(
                "old_val + n is greater than self.shared.write_offset: {} > {}",
                dbg!(old_val) + dbg!(n),
                write_offset
            );
        }
    }
}

#[derive(Debug)]
pub struct SliceBufWriter<T> {
    shared: Arc<SliceBuf<T>>,
}

impl<T> SliceBufWriter<T> {
    pub fn push(&mut self, value: T) {
        let mut offset = self.shared.write_offset.load(Ordering::Relaxed);

        if offset >= self.shared.capacity {
            // New allocation is needed.

            // Create a new SliceBuf from the previous instance.

            // TODO: smarter new capacity
            let mut new = SliceBuf::with_capacity(self.shared.capacity * 2);
            // Use read_offset from old alloc so the reader can use that during syncronization.
            let old_read_offset = self.shared.read_offset.load(Ordering::Acquire);
            // Copy data after read_offset to new buffer.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.shared
                        .data_start
                        .load(Ordering::Relaxed)
                        .add(old_read_offset),
                    *new.data_start.get_mut(),
                    offset - old_read_offset,
                )
            };

            // Reduce the write_offset by the number of elements consumed by the reader.
            offset -= old_read_offset;
            new.write_offset = AtomicUsize::new(offset);

            let new = Arc::new(new);

            // Update old slicebuf with information for the reader.

            // Store .1 first since the reader will always check .0 first.
            self.shared.next.1.store(old_read_offset, Ordering::Release);
            self.shared
                .next
                .0
                .store(Arc::into_raw(new.clone()).cast_mut(), Ordering::Release);

            // Update the writers instance to the newly allocated SliceBuf.

            self.shared = new;
        }

    pub fn push_exact<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        let iter = iter.into_iter();
        let iter_len = iter.len();

        let mut data_start = self.shared.data_start.load(Ordering::Relaxed);
        let mut write_offset = self.shared.write_offset.load(Ordering::Relaxed);

        // TODO: Maybe panic if the addition results in an overflow or the
        //       result is bigger than isize::MAX?
        if write_offset + iter_len > self.shared.capacity {
            // Not enough space left in current SliceBuf, a new allocation is
            // required.

            // ++ Create a new SliceBuf from the previous instance.

            // Use read_offset from old alloc, so it's available for the reader
            // when switching to the next alloc.
            let old_read_offset = self.shared.read_offset.load(Ordering::Acquire);
            write_offset -= old_read_offset;

            let new_len = write_offset + iter_len;
            let new_capacity = if new_len > self.shared.capacity / 2 {
                // Capacity of new buf should be at least twice the number of
                // it's initial elements.
                self.shared.capacity * 2
            } else {
                self.shared.capacity
            }
            .max(new_len); // at least enough for all elements
            let mut new = SliceBuf::with_capacity(new_capacity);

            // Copy data after read_offset to new buffer.
        unsafe {
                std::ptr::copy_nonoverlapping(
                    data_start.add(old_read_offset),
                    *new.data_start.get_mut(),
                    write_offset,
                )
            };
            data_start = *new.data_start.get_mut();

            new.write_offset = AtomicUsize::new(write_offset);

            let new = Arc::new(new);

            // ++ Update old slicebuf with information for the reader.

            // Store .1 first since the reader will always check .0 first.
            self.shared.next.1.store(old_read_offset, Ordering::Release);
            self.shared
                .next
                .0
                .store(Arc::into_raw(new.clone()).cast_mut(), Ordering::Release);

            // ++ Update the writers instance to the newly allocated SliceBuf.
            self.shared = new;
        }

        let mut next_write_addr = unsafe { data_start.add(write_offset) };

        for val in iter {
            unsafe { next_write_addr.write(val) };
            next_write_addr = unsafe { next_write_addr.add(1) };
        }

        self.shared
            .write_offset
            .store(write_offset + iter_len, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::SliceBuf;

    /// Macro to compare SliceBufs.
    ///
    /// It's a macro so the error messages point to the line where it's used.
    macro_rules! compare_bufs {
        ($left:expr, $right:expr) => {
            let left = $left;
            let right = $right;

            let SliceBuf {
                capacity: l_capacity,
                data_layout: l_data_layout,
                write_offset: l_write_offset,
                read_offset: l_read_offset,
                data_start: l_data_start,
                next: l_next,
            } = $left;
            let SliceBuf {
                capacity: r_capacity,
                data_layout: r_data_layout,
                write_offset: r_write_offset,
                read_offset: r_read_offset,
                data_start: r_data_start,
                next: r_next,
            } = $right;

            assert_eq!(
                l_capacity, r_capacity,
                "\nleft: {left:#?}\nright: {right:#?}"
            );
            assert_eq!(
                l_data_layout, r_data_layout,
                "\nleft: {left:#?}\nright: {right:#?}"
            );
            assert_eq!(
                l_write_offset.load(Ordering::Acquire),
                r_write_offset.load(Ordering::Acquire),
                "\nleft: {left:#?}\nright: {right:#?}"
            );
            assert_eq!(
                l_read_offset.load(Ordering::Acquire),
                r_read_offset.load(Ordering::Acquire),
                "\nleft: {left:#?}\nright: {right:#?}"
            );
            assert!(
                l_data_start.load(Ordering::Acquire).is_null()
                    == r_data_start.load(Ordering::Acquire).is_null(),
                "\nleft: {left:#?}\nright: {right:#?}"
            );
            assert!(
                l_next.0.load(Ordering::Acquire).is_null()
                    == r_next.0.load(Ordering::Acquire).is_null(),
                "\nleft: {left:#?}\nright: {right:#?}"
            );
            assert_eq!(
                l_next.1.load(Ordering::Acquire),
                r_next.1.load(Ordering::Acquire),
                "\nleft: {left:#?}\nright: {right:#?}"
            );
        };
    }

    #[test]
    #[should_panic(expected = "capacity is 0 but must be at least 1")]
    fn zero_initial_capacity() {
        let vec: Vec<u32> = Vec::with_capacity(0);
        assert_eq!(vec.capacity(), 0);
        SliceBuf::<u32>::with_capacity(0);
    }

    #[test]
    fn single_thread_slice_buf() {
        let buf = SliceBuf::with_capacity(100);
        let (mut writer, mut reader) = buf.split();

        for i in 0..100 {
            writer.push(i);
        }

        assert_eq!(reader.shared.remaining_capacity(), 0);
        reader.consume(60);
        assert_eq!(reader.slice_to(0).unwrap(), &[]);
        assert_eq!(reader.slice_to(3).unwrap(), &[60, 61, 62]);
    }

    #[test]
    fn multi_thread_slice_buf() {
        let n = 100;
        let buf = SliceBuf::with_capacity(n);
        let (mut writer, mut reader) = buf.split();

        let handle = std::thread::spawn(move || {
            for i in 0..n {
                writer.push(i);
            }
            writer
        });

        let mut next_expected = 0;

        loop {
            if let Some(slice) = reader.slice_to(5) {
                assert_eq!(slice.len(), 5);
                assert_eq!(slice[0], next_expected);
                next_expected += 4;
                assert_eq!(slice[4], next_expected);
                next_expected += 1;
                reader.consume(5);
            }

            if next_expected >= n {
                break;
            }
        }

        let mut writer = handle.join().unwrap();

        writer.push(1000);
        assert_eq!(reader.slice_to(1), None);

        reader.synchronize();
        assert_eq!(reader.slice_to(1).unwrap(), &[1000]);
    }

    #[test]
    pub fn read_until_end_of_alloc() {
        let buf = SliceBuf::with_capacity(100);
        let (mut writer, mut reader) = buf.split();

        for i in 0..100 {
            writer.push(i);
        }

        reader.consume(100);
    }

    #[test]
    pub fn alloc_after_full_read() {
        let init_capa = 2;
        let (mut writer, mut reader) = SliceBuf::with_capacity(init_capa).split();

        for i in 0..init_capa {
            writer.push(i);
        }

        assert_eq!(reader.slice_to(init_capa).unwrap(), &[0, 1]);

        reader.consume(init_capa);
        assert_eq!(reader.slice_to(1), None);

        let new_elem = init_capa + 1;
        writer.push(new_elem);
        assert_eq!(reader.slice_to(1), None);

        reader.synchronize();
        assert_eq!(reader.slice_to(1).unwrap(), &[new_elem]);

        reader.consume(1);
        assert_eq!(reader.slice_to(1), None);
    }

    #[test]
    fn consume_too_many() {
        let len = 10;
        let buf = SliceBuf::with_capacity(len);
        let (mut writer, mut reader) = buf.split();

        for i in 0..len {
            writer.push(i);
        }

        assert_eq!(reader.slice_to(10).unwrap().last(), Some(&9));
        reader.consume(10);
        assert_eq!(reader.slice_to(1), None);
        reader.consume(1);
    }

    #[test]
    fn alloc_chain() {
        let n = 2;
        let buf = SliceBuf::with_capacity(n);
        let (mut writer, mut reader) = buf.split();

        for i in 0..16 {
            writer.push(i);
        }

        assert_eq!(reader.slice_to(2).unwrap(), &[0, 1]);
        reader.consume(2);
        assert_eq!(reader.slice_to(1), None);

        reader.synchronize();

        assert_eq!(reader.slice_to(15), None);
        let full_slice = reader.slice_to(14).unwrap();
        assert_eq!(full_slice[0], 2);
        assert_eq!(full_slice[13], 15);
    }

    #[test]
    fn single_alloc_drop_reader() {
        let n = 10;
        let buf = SliceBuf::with_capacity(n);
        let (mut writer, mut reader) = buf.split();

        for i in 0..n {
            writer.push(i);
        }

        reader.consume(2);

        drop(reader);
    }

    #[test]
    fn single_alloc_drop_writer() {
        let n = 10;
        let buf = SliceBuf::with_capacity(n);
        let (mut writer, mut reader) = buf.split();

        for i in 0..n {
            writer.push(i);
        }

        reader.consume(2);

        drop(writer);
    }

    #[test]
    fn multi_alloc_drop_reader() {
        let capa = 2;
        let len = 20;
        let buf = SliceBuf::with_capacity(capa);
        let (mut writer, reader) = buf.split();

        for i in 0..len {
            writer.push(i);
        }

        assert_eq!(reader.shared.write_offset.load(Ordering::SeqCst), 2);

        drop(reader);

        for _ in 0..40 {
            writer.push(len);
        }
    }

    #[test]
    fn multi_alloc_drop_writer() {
        let capa = 2;
        let len = 20;
        let buf = SliceBuf::with_capacity(capa);
        let (mut writer, mut reader) = buf.split();

        for i in 0..len {
            writer.push(i);
        }

        assert_eq!(reader.shared.write_offset.load(Ordering::SeqCst), 2);

        reader.consume(2);

        drop(writer);

        assert_ne!(
            reader.shared.next.0.load(Ordering::SeqCst),
            std::ptr::null_mut()
        );

        reader.synchronize();

        assert_eq!(
            reader.shared.next.0.load(Ordering::SeqCst),
            std::ptr::null_mut()
        );

        assert_eq!(reader.slice_to(19), None);
        assert_eq!(reader.slice_to(18).unwrap().last(), Some(&19));
    }

    #[test]
    fn push_exact_is_the_same_as_push() {
        let (mut writer, mut reader) = SliceBuf::<usize>::with_capacity(2).split();
        let (mut exp_writer, mut exp_reader) = SliceBuf::<usize>::with_capacity(2).split();

        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());

        let n = 1;
        writer.push_exact(vec![n]);
        exp_writer.push(n);

        assert_eq!(reader.slice_to(n), exp_reader.slice_to(n));
        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());

        let n = 2;
        writer.push_exact(vec![n]);
        exp_writer.push(n);

        assert_eq!(reader.slice_to(n), exp_reader.slice_to(n));
        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());

        let n = 3;
        writer.push_exact(vec![n]);
        exp_writer.push(n);

        assert_eq!(writer.shared.capacity, 4);
        assert_eq!(reader.slice_to(n), None);
        assert_eq!(reader.slice_to(n), exp_reader.slice_to(n));

        reader.synchronize();
        exp_reader.synchronize();

        assert_eq!(reader.slice_to(n).unwrap(), &[1, 2, 3]);
        assert_eq!(reader.slice_to(n), exp_reader.slice_to(n));
        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());

        let n = 4;
        writer.push_exact(vec![n]);
        exp_writer.push(n);

        assert_eq!(writer.shared.capacity, 4);
        assert_eq!(reader.slice_to(n), exp_reader.slice_to(n));
        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());

        reader.consume(2);
        exp_reader.consume(2);

        writer.push_exact(vec![5, 6]);
        exp_writer.push(5);
        exp_writer.push(6);

        assert_eq!(writer.shared.capacity, 8);
        assert_eq!(reader.slice_to(4), None);
        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());

        reader.synchronize();
        exp_reader.synchronize();

        assert_eq!(reader.slice_to(4), exp_reader.slice_to(4));
        compare_bufs!(writer.shared.as_ref(), exp_writer.shared.as_ref());
        compare_bufs!(reader.shared.as_ref(), exp_reader.shared.as_ref());
    }

    #[test]
    fn push_exact_capacity_growth() {
        // Start with capacity of 1

        let (mut writer, _) = SliceBuf::with_capacity(1).split();
        writer.push_exact(0..0);
        assert_eq!(writer.shared.capacity, 1);

        let (mut writer, _) = SliceBuf::with_capacity(1).split();
        writer.push_exact(0..1);
        assert_eq!(writer.shared.capacity, 1);

        let (mut writer, _) = SliceBuf::with_capacity(1).split();
        writer.push_exact(0..2);
        assert_eq!(writer.shared.capacity, 2);

        let (mut writer, _) = SliceBuf::with_capacity(1).split();
        writer.push_exact(0..3);
        assert_eq!(writer.shared.capacity, 3);

        // Start with capacity of 2

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..0);
        assert_eq!(writer.shared.capacity, 2);

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..1);
        assert_eq!(writer.shared.capacity, 2);

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..2);
        assert_eq!(writer.shared.capacity, 2);

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..3);
        assert_eq!(writer.shared.capacity, 2 * 2);

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..4);
        assert_eq!(writer.shared.capacity, 2 * 2);

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..5);
        assert_eq!(writer.shared.capacity, 5);

        let (mut writer, _) = SliceBuf::with_capacity(2).split();
        writer.push_exact(0..11);
        assert_eq!(writer.shared.capacity, 11);
    }

    #[test]
    fn push_exact_after_consume() {
        let basic_setup = || {
            let (mut writer, mut reader) = SliceBuf::with_capacity(4).split();
            writer.push_exact(0..3);
            assert_eq!(writer.shared.capacity, 4);

            reader.consume(2);
            assert_eq!(reader.slice_to(1).unwrap(), &[2]);
            writer
        };

        let mut writer = basic_setup();
        assert_eq!(writer.shared.capacity, 4);
        writer.push_exact(0..0);
        assert_eq!(writer.shared.capacity, 4);

        let mut writer = basic_setup();
        writer.push_exact(0..1);
        assert_eq!(writer.shared.capacity, 4);

        let mut writer = basic_setup();
        writer.push_exact(0..2); // len = 3
        assert_eq!(writer.shared.capacity, 8);

        let mut writer = basic_setup();
        writer.push_exact(0..3); // len = 4
        assert_eq!(writer.shared.capacity, 8);

        let mut writer = basic_setup();
        writer.push_exact(0..4); // len = 5
        assert_eq!(writer.shared.capacity, 8);

        let mut writer = basic_setup();
        writer.push_exact(0..5); // len = 6
        assert_eq!(writer.shared.capacity, 8);

        let mut writer = basic_setup();
        writer.push_exact(0..6); // len = 7
        assert_eq!(writer.shared.capacity, 8);

        let mut writer = basic_setup();
        writer.push_exact(0..7); // len = 8
        assert_eq!(writer.shared.capacity, 8);

        let mut writer = basic_setup();
        writer.push_exact(0..8); // len = 9
        assert_eq!(writer.shared.capacity, 9);

        let mut writer = basic_setup();
        writer.push_exact(0..9); // len = 10
        assert_eq!(writer.shared.capacity, 10);
    }
}
