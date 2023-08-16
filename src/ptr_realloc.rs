// TODO: Actually change this

use std::{
    alloc::Layout,
    sync::{
        atomic::{AtomicPtr, AtomicUsize, Ordering},
        Arc,
    },
};

/// Only for types for which `mem::needs_drop` returns false.
pub struct SliceBuf<T> {
    capacity: usize,
    data_layout: Layout,
    write_offset: AtomicUsize,
    read_offset: AtomicUsize,
    data_start: AtomicPtr<T>,
}

impl<T> SliceBuf<T> {
    pub fn new() -> Self {
        Self::with_capacity(4)
    }

    // pub(crate) fn len(&self) -> usize {
    //     // TOOD: Handle multiple allocations and potential data races.
    //     self.write_offset.load(Ordering::Acquire) - self.read_offset.load(Ordering::Acquire)
    // }

    pub(crate) fn remaining_capacity(&self) -> usize {
        // TOOD: Handle multiple allocations and potential data races.
        self.capacity - self.write_offset.load(Ordering::Relaxed)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let size_of_t = std::mem::size_of::<T>();
        let align_of_t = std::mem::align_of::<T>();
        assert_ne!(size_of_t, 0);
        assert!(!std::mem::needs_drop::<T>());

        let data_layout = Layout::from_size_align(size_of_t * capacity, align_of_t).unwrap();
        let data_start = unsafe { std::alloc::alloc(data_layout) }.cast();
        Self {
            capacity,
            data_layout,
            data_start: AtomicPtr::from(data_start),
            write_offset: AtomicUsize::new(0),
            read_offset: AtomicUsize::new(0),
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
        // TODO: Drop elements between read_offset and
        assert!(!std::mem::needs_drop::<T>());
        unsafe { std::alloc::dealloc(self.data_start.get_mut().cast(), self.data_layout) }
    }
}

pub struct SliceBufReader<T> {
    shared: Arc<SliceBuf<T>>,
}

impl<T> SliceBufReader<T> {
    pub fn slice_to(&self, to: usize) -> Option<&[T]> {
        let read_offset = self.shared.read_offset.load(Ordering::Relaxed);
        let start = unsafe {
            self.shared
                .data_start
                .load(Ordering::Acquire)
                .add(read_offset)
        };
        let write_offset = self.shared.write_offset.load(Ordering::Acquire);
        let len = write_offset - read_offset;

        if to > len {
            return None;
        }

        Some(unsafe { std::slice::from_raw_parts(start, to) })
    }

    pub fn consume(&mut self, n: usize) {
        // TODO: How should this behave when multiple allocations exist?
        // TODO: Maybe change ordering when writer also accesses read_offset.

        let old_val = self.shared.read_offset.fetch_add(n, Ordering::Release);
        assert!(old_val + n <= self.shared.write_offset.load(Ordering::Acquire));
    }
}

pub struct SliceBufWriter<T> {
    shared: Arc<SliceBuf<T>>,
}

impl<T> SliceBufWriter<T> {
    pub fn push(&mut self, value: T) {
        assert!(self.shared.remaining_capacity() > 0);
        // TODO: Allocate new buf.

        unsafe {
            let offset = self.shared.write_offset.load(Ordering::Relaxed);
            self.shared
                .data_start
                .load(Ordering::Relaxed)
                .add(offset)
                .write(value);
            self.shared
                .write_offset
                .store(offset + 1, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SliceBuf;

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
            for i in 0..100 {
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
    }
}
