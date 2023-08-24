use std::{collections::VecDeque, sync::Arc, thread};

use parking_lot::Mutex;
use slicebuf::expl_sync;

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

pub fn explicit_sync(n: usize) {
    let buf = expl_sync::SliceBuf::with_capacity(100);
    let (mut writer, mut reader) = buf.split();

    thread::spawn(move || {
        for i in 0..n {
            writer.push(i);
        }
    });

    let mut drained = 0;
    let block_size = 100;
    loop {
        if let Some(_) = reader.slice_to(block_size) {
            reader.consume(block_size);
            drained += block_size;
            if drained >= n {
                break;
            }
        } else {
            reader.synchronize();
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

pub fn single_alloc_explicit_sync(n: usize) {
    let buf = expl_sync::SliceBuf::with_capacity(n);
    let (mut writer, mut reader) = buf.split();

    thread::spawn(move || {
        for i in 0..n {
            writer.push(i);
        }
    });

    let mut drained = 0;
    let block_size = 100;
    loop {
        if let Some(_) = reader.slice_to(block_size) {
            reader.consume(block_size);
            drained += block_size;
            if drained >= n {
                break;
            }
        } else {
            reader.synchronize();
        }
    }
}
