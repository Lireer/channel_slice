use std::collections::VecDeque;

use slicebuf::lock_free_ringbuf;

pub fn std_vecdeque(n: usize) {
    let mut buf = VecDeque::with_capacity(100);

    for i in 0..n {
        buf.push_back(i);
    }

    for _ in 0..(n / 100) {
        _ = buf.drain(0..100);
    }
}

pub fn single_alloc_lf_ringbuf(n: usize) {
    let (mut sender, mut reader) = lock_free_ringbuf::create_bounded(n);

    for i in 0..n {
        while let Err(_) = sender.try_send(i) {}
    }

    for _ in 0..(n / 100) {
        let len = 100;
        assert_eq!(reader.recv_up_to(len).len(), len);
    }
}

pub fn single_alloc_std_vecdeque(n: usize) {
    let mut buf = VecDeque::with_capacity(n);

    for i in 0..n {
        buf.push_back(i);
    }

    for _ in 0..(n / 100) {
        _ = buf.drain(0..100);
    }
}
