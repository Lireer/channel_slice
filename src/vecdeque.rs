use std::{collections::VecDeque, sync::Arc};

use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};

use crate::{SliceBufRead, SliceBufWrite};

pub struct MutexVecDeque<T> {
    buf: Mutex<VecDeque<T>>,
}

impl<T> MutexVecDeque<T> {
    pub fn new() -> Self {
        Self {
            buf: Default::default(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }
}

impl<T> SliceBufRead<T> for Arc<MutexVecDeque<T>> {
    type Slice<'data> = MappedMutexGuard<'data, [T]> where T: 'data;

    fn slice_to(&self, to: usize) -> Option<Self::Slice<'_>> {
        let guard = self.buf.lock();
        if to > guard.len() {
            return None;
        }
        Some(MutexGuard::map(guard, |deque| {
            &mut deque.make_contiguous()[..to]
        }))
    }

    fn consume(&mut self, n: usize) {
        self.buf.lock().drain(..n);
    }
}

impl<T> SliceBufWrite<T> for Arc<MutexVecDeque<T>> {
    fn push(&mut self, value: T) {
        self.buf.lock().push_back(value)
    }

    fn append(&mut self, slice: &[T])
    where
        T: Clone,
    {
        let mut buf = self.buf.lock();
        for elem in slice {
            buf.push_back(elem.clone())
        }
    }

    fn append_vec(&mut self, data: Vec<T>) {
        let mut buf = self.buf.lock();
        for elem in data.into_iter() {
            buf.push_back(elem)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::MutexVecDeque;
    use crate::{SliceBufRead, SliceBufWrite};

    #[test]
    fn single_thread_slice_buf() {
        let mut reader = Arc::new(MutexVecDeque::with_capacity(100));
        let mut writer = reader.clone();

        for i in 0..100 {
            writer.push(i);
        }

        reader.consume(60);
        assert_eq!(&*reader.slice_to(0).unwrap(), &[]);
        assert_eq!(&*reader.slice_to(3).unwrap(), &[60, 61, 62]);
    }

    #[test]
    fn multi_thread_slice_buf() {
        let n = 100;
        let mut reader = Arc::new(MutexVecDeque::with_capacity(n));
        let mut writer = reader.clone();

        let handle = std::thread::spawn(move || {
            for i in 0..n {
                writer.push(i);
            }
            writer
        });

        let mut next_expected = 0;

        loop {
            let mut consume = false;
            if let Some(slice) = reader.slice_to(5).as_deref() {
                assert_eq!(slice.len(), 5);
                assert_eq!(slice[0], next_expected);
                next_expected += 4;
                assert_eq!(slice[4], next_expected);
                next_expected += 1;
                consume = true;
            }

            if consume {
                reader.consume(5);
            }

            if next_expected >= n {
                break;
            }
        }

        let mut writer = handle.join().unwrap();

        writer.push(1000);
        assert_eq!(&*reader.slice_to(1).unwrap(), &[1000]);
    }

    #[test]
    fn new_allocs() {
        let n = 2;
        let mut reader = Arc::new(MutexVecDeque::with_capacity(n));
        let mut writer = reader.clone();

        for i in 0..16 {
            writer.push(i);
        }

        assert_eq!(&*reader.slice_to(2).unwrap(), &[0, 1]);
        reader.consume(2);
        assert_eq!(&*reader.slice_to(1).unwrap(), &[2]);

        assert_eq!(reader.slice_to(15).as_deref(), None);
        let full_slice = reader.slice_to(14).unwrap();
        assert_eq!(full_slice[0], 2);
        assert_eq!(full_slice[13], 15);
    }
}
