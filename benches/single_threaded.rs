use std::collections::VecDeque;

use slicebuf::expl_sync;

pub fn std_vecdeque(n: usize) {
    let mut buf = VecDeque::with_capacity(100);

    for i in 0..n {
        buf.push_back(i);
    }

    for _ in 0..(n / 100) {
        _ = buf.drain(0..100);
    }
}

pub fn explicit_sync(n: usize) {
    let buf = expl_sync::SliceBuf::with_capacity(100);
    let (mut writer, mut reader) = buf.split();

    for i in 0..n {
        writer.push(i);
    }

    reader.synchronize();

    for _ in 0..(n / 100) {
        let len = 100;
        assert!(reader.slice_to(len).is_some());
        reader.consume(len);
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

pub fn single_alloc_explicit_sync(n: usize) {
    let buf = expl_sync::SliceBuf::with_capacity(n);
    let (mut writer, mut reader) = buf.split();

    for i in 0..n {
        writer.push(i);
    }

    reader.synchronize();

    for _ in 0..(n / 100) {
        let len = 100;
        assert!(reader.slice_to(len).is_some());
        reader.consume(len);
    }
}
