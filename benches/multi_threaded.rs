use std::{collections::VecDeque, sync::Arc, thread};

use parking_lot::Mutex;
use slicebuf::lock_free_ringbuf;

pub fn std_vecdeque(n: usize) {
    let buf = Arc::new(Mutex::new(VecDeque::with_capacity(100)));

    thread::spawn({
        let buf = buf.clone();
        move || {
            for i in 0..n {
                buf.lock().push_back(i);
            }
        }
    });

    let mut drained = 0;
    let block_size = 100;
    loop {
        let mut buf = buf.lock();
        if buf.len() >= block_size {
            buf.drain(0..block_size);
            drained += block_size;
            if drained >= n {
                break;
            }
        }
    }
}

pub fn single_alloc_std_vecdeque(n: usize) {
    let buf = Arc::new(Mutex::new(VecDeque::with_capacity(n)));

    thread::spawn({
        let buf = buf.clone();
        move || {
            for i in 0..n {
                buf.lock().push_back(i);
            }
        }
    });

    let mut drained = 0;
    let block_size = 100;
    loop {
        let mut buf = buf.lock();
        if buf.len() >= block_size {
            buf.drain(0..block_size);
            drained += block_size;
            if drained >= n {
                break;
            }
        }
    }
}

pub fn single_alloc_lf_ringbuf(n: usize) {
    let (mut writer, mut reader) = lock_free_ringbuf::create_bounded(n);

    thread::spawn(move || {
        for i in 0..n {
            // can't fail cause the buffer has the same capacity as the number of elements that will
            // be pushed into it.
            _ = writer.try_send(i);
        }
    });

    let mut drained = 0;
    let block_size = 100;
    loop {
        if reader.len() >= block_size {
            reader.read(block_size);
            drained += block_size;
            if drained >= n {
                break;
            }
        }
    }
}
