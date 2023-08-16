use std::collections::VecDeque;

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

#[cfg(test)]
mod tests {
    use super::VecSliceBuf;

    #[test]
    fn vec_slice_buf() {
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
}
