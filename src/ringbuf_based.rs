use ringbuf::traits::{Producer as _, Split};

fn create_ringbuf_channel<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let (prod, cons) = ringbuf::HeapRb::<T>::new(capacity).split();
    (Producer { inner: prod }, Consumer { inner: cons })
}

struct Producer<T> {
    inner: ringbuf::HeapProd<T>,
}

struct Consumer<T> {
    inner: ringbuf::HeapCons<T>,
}

impl<T> crate::SliceBufRead<T> for Consumer<T>
where
    T: Clone,
{
    type Slice<'a>
        = ringbuf::storage::Slice<'a, T>
    where
        Self: 'a;

    fn slice(&mut self, range: std::ops::RangeInclusive<usize>) -> Self::Slice<'_> {
        self.inner.slice(range)
    }

    fn consume(&mut self, n: usize) {
        self.inner.consume(n);
    }

    fn try_slice(&mut self, range: std::ops::RangeInclusive<usize>) -> Option<Self::Slice<'_>> {
        self.inner.try_slice(range)
    }

    fn try_pop_into_vec(&mut self, n: usize) -> Option<Vec<T>> {
        self.inner.try_pop_into_vec(n)
    }

    fn try_consume(&mut self, n: usize) {
        self.inner.try_consume(n);
    }
}
