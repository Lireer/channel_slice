pub mod arc_next;
pub mod ptr_realloc;
pub mod vec_internal;

fn main() {}

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
