pub mod expl_sync;
pub mod ptr_next;
pub mod ptr_realloc;
pub mod vec_internal;
pub mod vecdeque;

fn main() {}

pub trait SliceBufRead<T> {
    type Slice<'data>
    where
        Self: 'data;
    fn slice_to<'data>(&'data self, to: usize) -> Option<Self::Slice<'data>>;
    fn consume(&mut self, n: usize);
}

pub trait SliceBufWrite<T> {
    fn push(&mut self, data: T);
    fn append(&mut self, data: &[T])
    where
        T: Clone;
    fn append_vec(&mut self, data: Vec<T>);
}
