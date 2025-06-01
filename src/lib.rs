#![deny(unsafe_op_in_unsafe_fn)]

// pub mod bounded;
// pub mod expl_sync;
pub mod lock_free_ringbuf;

pub trait SliceBufRead<T> {
    type Slice<'a>: AsRef<[T]>
    where
        Self: 'a;

    /// View the elements in the index `range`, blocking if not enough elemnts are in the buffer.
    ///
    /// # Panics
    ///
    /// Implementors of this trait should panic if the end of the range is greater than the capacity
    /// of the buffer.
    fn slice(&mut self, range: RangeInclusive<usize>) -> Self::Slice<'_>;
    /// Pop the next `n` elements into a vector, blocking if not enough elements are in the buffer.
    fn pop_slice(&mut self, n: usize) -> Vec<T>;
    /// Consume the next `n` elements in the buffer.
    fn consume(&mut self, n: usize);

    fn try_slice(&mut self, range: RangeInclusive<usize>) -> Option<Self::Slice<'_>>;
    fn try_pop(&mut self, n: usize, buf: &mut Vec<T>) -> Result<(), usize>;
    /// Consume and drop the next `n` elements in the buffer.
    ///
    /// Returns `Err(usize)` if there are not enough elements in the buffer to consume. The value
    /// indicates how many more elements have to be in the buffer for the same call to succeed.
    fn try_consume_exact(&mut self, n: usize) -> Result<(), usize>;

    // Convenience methods to be added later
    // fn slice_next(&mut self, n: usize) -> Option<Self::Slice<'_>>;
    // Equivalent to `try_slice(0..n)`, so we ignore it for now.
    // fn try_slice_next(&mut self, n: usize) -> Option<Self::Slice<'_>>;
    // /// Returns the number of elements in the buffer.
    // fn try_pop(&mut self, n: usize) -> Result<Vec<T>, usize>;
    // ...
}

pub trait SliceBufWrite<T> {
    fn push(&mut self, data: T);
    fn append(&mut self, data: &[T])
    where
        T: Clone;
    fn append_vec(&mut self, data: Vec<T>);
}
