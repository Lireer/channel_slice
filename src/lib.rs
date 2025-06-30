#![deny(unsafe_op_in_unsafe_fn)]

use std::ops::{Range, RangeInclusive};

// pub mod bounded;
// pub mod expl_sync;
pub mod lock_free_ringbuf;
pub mod rtrb;
pub mod simple_internals;
// pub mod ringbuf_based;
// pub mod wrapping;

pub fn try_send_iter(
    mut s: lock_free_ringbuf::Sender<u32>,
    data: Vec<u32>,
) -> Result<(), lock_free_ringbuf::SendError<Vec<u32>>> {
    // let (mut s, mut r) = lock_free_ringbuf::create_bounded(4);
    // s.try_send_iter([0, 1]).unwrap();
    // assert_eq!(vec![0, 1], r.read(2));
    s.try_send_vec(std::hint::black_box(data))
}

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
mod tests {
    // #[test]
    // fn atomics_test() {
    //     loom::model(|| {
    //         println!("\nStarting atomics test");
    //         let a = loom::sync::Arc::new(loom::sync::atomic::AtomicUsize::new(0));

    //         let thread1 = loom::thread::spawn({
    //             let a = a.clone();
    //             move || {
    //                 for _ in 0..2 {
    //                     // Fails, i.e., becomes an infinite loop if thread2 never runs, which is
    //                     // something that can happen in the real world. Reasons for that could be
    //                     // that thread2 is waiting for some event to happen, or that thread2 is
    //                     // waiting for some resource to become available. The ordering becomes
    //                     // irrelevant in such a case.
    //                     loop {
    //                         let v = a.load(loom::sync::atomic::Ordering::SeqCst);
    //                         println!("Thread 1: loaded = {v}, will increment by 1 if > 1");
    //                         if v > 1 {
    //                             a.store(v + 1, loom::sync::atomic::Ordering::Relaxed);
    //                             break;
    //                         }
    //                     }
    //                     // println!(
    //                     //     "Thread 1: loaded = {}, will increment by 1",
    //                     //     a.fetch_add(1, loom::sync::atomic::Ordering::Relaxed)
    //                     // );
    //                 }
    //             }
    //         });

    //         let thread2 = loom::thread::spawn({
    //             let a = a.clone();
    //             move || {
    //                 for _ in 0..2 {
    //                     println!(
    //                         "Thread 2: loaded = {}, will increment by 1",
    //                         a.fetch_add(1, loom::sync::atomic::Ordering::SeqCst)
    //                     );
    //                 }
    //             }
    //         });

    //         thread1.join().unwrap();
    //         thread2.join().unwrap();

    //         assert_eq!(a.load(loom::sync::atomic::Ordering::SeqCst), 4);
    //     });
    // }
}
