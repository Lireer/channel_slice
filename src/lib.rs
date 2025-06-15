#![deny(unsafe_op_in_unsafe_fn)]

use std::ops::RangeInclusive;

// pub mod bounded;
// pub mod expl_sync;
pub mod lock_free_ringbuf;
pub mod rtrb;
pub mod simple_internals;

pub trait SliceChannelReceiver<T> {
    type Slice<'a>: AsRef<[T]>
    where
        Self: 'a;

    /// View the elements in the index `range`, blocking if not enough elemindexents are available.
    ///
    /// # Panics
    ///
    /// Implementors of this trait should panic if the end of the range is greater than `Self`'s
    /// capacity.
    // TODO: Replace `Range` with `std::ops::RangeBounds`.
    fn slice(&mut self, range: Range<usize>) -> Self::Slice<'_>;
    /// Remove the next `n` elements and append them to `buf`, blocking if not enough elements are
    /// available.
    ///
    /// # Panics
    ///
    /// Implementors of this trait should panic if `n` is greater than `Self`'s capacity.
    fn pop(&mut self, n: usize, buf: &mut Vec<T>);
    /// Consume the next `n` elements, blocking if less than `n` elements are available.
    ///
    /// # Panics
    ///
    /// Implementors of this trait should panic if `n` is greater than `Self`'s capacity.
    fn consume_exact(&mut self, n: usize);

    /// View the elements in the index `range`, immediately returning if not enough elements are
    /// available.
    ///
    /// Returns `Err(usize)` if there are not enough elements in `Self` with the value indicating
    /// how many more elements have to be in `Self` for the same call to succeed.
    // TODO: Replace `Range` with `std::ops::RangeBounds`.
    fn try_slice(&mut self, range: Range<usize>) -> Result<Self::Slice<'_>, usize>;
    fn try_pop(&mut self, n: usize, buf: &mut Vec<T>) -> Result<(), usize>;
    /// Consume and drop the next `n` elements in the buffer.
    ///
    /// Returns `Err(usize)` if there are not enough elements in the buffer to consume. The value
    /// indicates how many more elements have to be in `Self` for the same call to succeed.
    fn try_consume_exact(&mut self, n: usize) -> Result<(), usize>;

    // Convenience methods to be added later
    // fn slice_next(&mut self, n: usize) -> Option<Self::Slice<'_>>;
    // Equivalent to `try_slice(0..n)`, so we ignore it for now.
    // fn try_slice_next(&mut self, n: usize) -> Option<Self::Slice<'_>>;
    // /// Returns the number of elements in the buffer.
    // fn try_pop(&mut self, n: usize) -> Result<Vec<T>, usize>;
    // ...
}

pub trait SliceChannelSender<T> {
    /// Append the elements in `data` to the channel, blocking if not enough space is available.
    ///
    /// # Panics
    ///
    /// Implementors should panic if `data` contains more elements than the channel's capacity.
    fn append<D>(&mut self, data: D)
    where
        D: IntoIterator<Item = T>,
        D::IntoIter: ExactSizeIterator;

    /// Try to append the elements in `data` to the channel, returning an error if not enough space
    /// is available.
    ///
    /// Returns `Err(usize)` if there is not enough space in the channel to append the elements. The
    /// value indicates how many more elements have to be in the channel for the same call to
    /// succeed.
    ///
    /// # Panics
    ///
    /// Implementors should panic if `data` contains more elements than the channel's capacity.
    fn try_append<D>(&mut self, data: D) -> Result<(), usize>
    where
        D: IntoIterator<Item = T>,
        D::IntoIter: ExactSizeIterator;
}

#[cfg(test)]
mod integration_tests {
    use crate::simple_internals::create_simple_channel;
    use crate::{SliceChannelReceiver, SliceChannelSender};

    #[test]
    fn test_simple_channel_integration() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(5);

        // Test sending and receiving
        sender.append(vec![1, 2, 3]);

        let slice = receiver.try_slice(0..=2).unwrap();
        assert_eq!(slice.as_ref(), &[1, 2, 3]);

        receiver.consume_exact(3);
    }
}
