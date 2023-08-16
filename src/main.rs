// use std::{
//     collections::VecDeque,
//     slice::SliceIndex,
//     sync::{Mutex, MutexGuard},
// };

use std::{
    alloc::Layout,
    collections::VecDeque,
    sync::{
        atomic::{AtomicPtr, AtomicUsize, Ordering},
        Arc,
    },
};

fn main() {
    let buf = SliceBuf::with_capacity(100);
    let (mut writer, mut reader) = buf.split();

    for i in 0..100 {
        writer.push(i);
    }

    assert_eq!(reader.shared.remaining_capacity(), 0);
    reader.consume(60);
    assert_eq!(reader.slice_to(0).unwrap(), &[]);
    assert_eq!(reader.slice_to(3).unwrap(), &[60, 61, 62]);

    test_vec_buf();
}

fn test_vec_buf() {
    let mut buf = VecSliceBuf::new();

    for i in 0..100 {
        buf.push(i);
    }
    assert_eq!(buf.len(), 100);

    let values = buf.slice_to(50).unwrap();
    assert_eq!(values.len(), 50);
    let values = buf.slice_to(buf.len()).unwrap();
    assert_eq!(values.len(), buf.len());

    buf.consume(100);
    assert_eq!(buf.len(), 0);
}

pub struct SliceBuf<T> {
    capacity: usize,
    data_layout: Layout,
    data_start: AtomicPtr<T>,
    write_offset: AtomicUsize,
    read_offset: AtomicUsize,
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
                .store(offset + 1, Ordering::Relaxed);
        }
    }
}

pub struct VecSliceBuf<T> {
    max_capa: usize,
    buf: VecDeque<T>,
}

impl<T> VecSliceBuf<T> {
    pub fn new() -> Self {
        let max_capa = 1;
        Self {
            max_capa,
            buf: VecDeque::with_capacity(max_capa),
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn push(&mut self, value: T) {
        assert_eq!(self.buf.capacity(), self.max_capa);
        if self.buf.len() == self.buf.capacity() {
            // Max capacity reached, allocate a new buffer.
            self.buf.push_back(value);
            assert!(self.max_capa < self.buf.capacity());
            self.max_capa = self.buf.capacity();
            println!("New allocation, capacity is now {}", self.max_capa);
        } else {
            assert!(self.buf.len() < self.buf.capacity());
            self.buf.push_back(value);
        }

        self.buf.make_contiguous();
    }

    pub fn slice_to(&self, to: usize) -> Option<&[T]> {
        if to > self.buf.len() {
            return None;
        }
        let (pre_wrap, post_wrap) = self.buf.as_slices();
        assert_eq!(post_wrap.len(), 0);
        assert_eq!(pre_wrap.len(), self.buf.len());
        Some(&pre_wrap[..to])
    }

    pub fn consume(&mut self, n: usize) {
        _ = self.buf.drain(..n);
    }

    // pub fn push_within_capacity(&mut self, value: T) -> Result<(), T> {
    //     self.buf.push_within_capacity()
    // }
}

impl<T> Default for VecSliceBuf<T> {
    fn default() -> Self {
        Self::new()
    }
}

// pub trait SliceBufRead<'data, T> {
//     type Slice;
//     fn slice(&'data mut self, range: impl SliceIndex<usize>) -> Self::Slice;
//     fn take(&mut self, n: usize) -> Vec<T>;
// }

// pub trait SliceBufWrite<T> {
//     fn append(&mut self, data: &[T])
//     where
//         T: Clone;
//     fn append_vec(&mut self, data: Vec<T>);
// }

// pub struct MutexVecDeque<T> {
//     // guard: Option<MutexGuard<'a, VecDeque<T>>>,
//     buf: Mutex<VecDeque<T>>,
// }

// impl<'data, T: 'data> SliceBufRead<'data, T> for MutexVecDeque<T> {
//     type Slice = (MutexGuard<'data, VecDeque<T>>, &'data [T]);

//     fn slice(&'data mut self, range: impl SliceIndex<usize>) -> Self::Slice {
//         let mut guard = self.buf.lock().unwrap();
//         let slice = guard.make_contiguous();
//         (guard, slice)
//     }

//     fn take(&mut self, n: usize) -> Vec<T> {
//         self.buf.lock().unwrap().drain(..n).collect()
//     }
// }

// impl<T> SliceBufWrite<T> for Arc<Mutex<VecDeque<T>>> {
//     fn append(&mut self, slice: &[T])
//     where
//         T: Clone,
//     {
//         let mut buf = self.lock().unwrap();
//         for elem in slice {
//             buf.push_back(elem.clone())
//         }
//     }

//     fn append_vec(&mut self, data: Vec<T>) {
//         let mut buf = self.lock().unwrap();
//         for elem in data.into_iter() {
//             buf.push_back(elem)
//         }
//     }
// }
