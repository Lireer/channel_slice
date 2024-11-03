pub mod bounded;
pub mod expl_sync;
pub mod lock_free_ringbuf;

pub trait SliceBufRead<T> {
    type Slice<'data>
    where
        Self: 'data;
    fn slice_to(&self, to: usize) -> Option<Self::Slice<'_>>;
    fn consume(&mut self, n: usize);
}

pub trait SliceBufWrite<T> {
    fn push(&mut self, data: T);
    fn append(&mut self, data: &[T])
    where
        T: Clone;
    fn append_vec(&mut self, data: Vec<T>);
}
