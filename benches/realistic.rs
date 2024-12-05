use std::{collections::VecDeque, sync::Arc, thread};

use parking_lot::Mutex;
use slicebuf::lock_free_ringbuf::{self, SendError};

// The use case we're benchmarking here is processing of audio samples.
//
// # Example
//
// - audio format is mono channel, 16kHz and bit depth of 32
// - Buffers will initially allocate memory for half a second of audio, i.e. 8000 samples or 32000 bytes
// - Samples will be written in blocks of 40ms / 640 samples

pub const SAMPLE_RATE: usize = 16000;
const INITIAL_BUFFER_SIZE: usize = SAMPLE_RATE / 2;
pub const INPUT_BLOCK_SIZE: usize = SAMPLE_RATE / 25; // 25 Blocks per second
const READ_SIZE: usize = 1600;

pub fn std_vecdeque(samples: &mut Vec<Vec<f32>>) {
    let buf = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(
        INITIAL_BUFFER_SIZE,
    )));

    let reader_handle = thread::spawn({
        let mut drained = 0;
        let limit = samples.len() * INPUT_BLOCK_SIZE;
        let buf = buf.clone();
        move || loop {
            let mut buf = buf.lock();
            if buf.len() >= READ_SIZE {
                drained += buf.drain(0..READ_SIZE).len();
                if drained >= limit {
                    break;
                }
            }
        }
    });

    thread::scope({
        |scope| {
            scope
                .spawn(|| {
                    while let Some(block) = samples.pop() {
                        buf.lock().extend(block);
                    }
                })
                .join()
                .unwrap();
        }
    });

    reader_handle.join().unwrap();
}

pub fn lf_ringbuf(samples: &mut Vec<Vec<f32>>) {
    let (mut sender, mut recv) = lock_free_ringbuf::create_bounded(INITIAL_BUFFER_SIZE);

    let recv_handle = thread::spawn({
        let mut drained = 0;
        let limit = samples.len() * INPUT_BLOCK_SIZE;
        move || {
            while drained < limit {
                if recv.len() >= READ_SIZE {
                    drained += recv.recv_up_to(READ_SIZE).len();
                }
            }
        }
    });

    thread::scope({
        |scope| {
            scope
                .spawn(|| {
                    while let Some(mut block) = samples.pop() {
                        loop {
                            match sender.try_send_vec(block) {
                                Ok(_) => break,
                                Err(SendError::Full(b)) => block = b,
                                Err(SendError::Dropped(_)) => {
                                    unreachable!("Receiver disconnected");
                                }
                            }
                        }
                    }
                })
                .join()
                .unwrap();
        }
    });

    recv_handle.join().unwrap();
}

fn avg(samples: &[f32]) -> f32 {
    // dbg!(samples[0], samples[samples.len() - 1]);
    samples.iter().sum::<f32>() / samples.len() as f32
}

pub fn std_vecdeque_do_work(samples: &mut Vec<Vec<f32>>) {
    let buf = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(
        INITIAL_BUFFER_SIZE,
    )));

    let reader_handle = thread::spawn({
        // Try to process as many samples as possible but subtract READ_SIZE to avoid edge cases.
        let limit = samples.len() * INPUT_BLOCK_SIZE - READ_SIZE;
        let buf = buf.clone();

        let mut expensive_mode = false;
        let mut read = 0;
        let mut drained = 0;

        move || loop {
            // Here we simulate some expensive processing of the samples.
            let mut guard = buf.lock();
            let buf = guard.make_contiguous();

            if expensive_mode {
                spin_sleep::sleep(std::time::Duration::from_micros(40));
                let read_until = read + READ_SIZE / 2;
                if buf.len() >= read_until {
                    // Check if the average is now below -0.75, ...
                    if avg(&buf[read..read_until]) < -0.5 {
                        // if so, switch to non-expensive mode, draining all samples until read_until.
                        spin_sleep::sleep(std::time::Duration::from_micros(100));
                        expensive_mode = !expensive_mode;
                        drained += guard.drain(0..read_until).len();
                        read = 0;
                    } else {
                        // else just increase read.
                        read = read_until;
                    }
                }
            } else {
                // read is always 0 at this point because it's reset after switching and when already in this mode it's
                // never increased.
                if buf.len() >= READ_SIZE {
                    spin_sleep::sleep(std::time::Duration::from_micros(10));
                    // Check if the average is above 0.75, ...
                    if avg(&buf[..READ_SIZE]) > 0.4 {
                        // if so, switch to expensive mode and mark these samples as already read.
                        expensive_mode = !expensive_mode;
                        read = READ_SIZE;
                    } else {
                        // else advance the window by half of READ_SIZE.
                        let n = READ_SIZE / 2;
                        drained += guard.drain(0..n).len();
                    }
                }
            }
            if drained >= limit {
                break;
            }
        }
    });

    thread::scope({
        |scope| {
            scope
                .spawn(|| {
                    while let Some(block) = samples.pop() {
                        buf.lock().extend(block);
                    }
                })
                .join()
                .unwrap();
        }
    });

    reader_handle.join().unwrap();
}

pub fn lf_ringbuf_do_work(samples: &mut Vec<Vec<f32>>) {
    let (mut sender, mut receiver) = lock_free_ringbuf::create_bounded(INITIAL_BUFFER_SIZE);

    let reader_handle = thread::spawn({
        // Try to process as many samples as possible but subtract READ_SIZE to avoid edge cases.
        let limit = samples.len() * INPUT_BLOCK_SIZE - READ_SIZE;

        let mut expensive_mode = false;
        let mut read = 0;
        let mut drained = 0;

        move || loop {
            // Here we simulate some expensive processing of the samples.
            if expensive_mode {
                spin_sleep::sleep(std::time::Duration::from_micros(40));
                let read_until = read + READ_SIZE / 2;
                if let Ok(buf) = receiver.peek_cow_exact(read_until) {
                    // Check if the average is now below -0.75, ...
                    if avg(&buf[read..read_until]) < -0.5 {
                        // if so, switch to non-expensive mode, draining all samples until read_until.
                        spin_sleep::sleep(std::time::Duration::from_micros(100));
                        expensive_mode = !expensive_mode;
                        let len = buf.len();
                        drained += len;
                        _ = receiver.read_exact(len);
                        read = 0;
                    } else {
                        // else just increase read.
                        read = read_until;
                    }
                }
            } else {
                // read is always 0 at this point because it's reset after switching and when already in this mode it's
                // never increased.
                if let Ok(buf) = receiver.peek_cow_exact(READ_SIZE) {
                    spin_sleep::sleep(std::time::Duration::from_micros(10));
                    // Check if the average is above 0.75, ...
                    if avg(buf.as_ref()) > 0.4 {
                        // if so, switch to expensive mode and mark these samples as already read.
                        expensive_mode = !expensive_mode;
                        read = READ_SIZE;
                    } else {
                        // else advance the window by half of READ_SIZE.
                        let len = READ_SIZE / 2;
                        drained += len;
                        while let Err(_) = receiver.read_exact(len) {}
                    }
                }
            }
            if drained >= limit {
                break;
            }
        }
    });

    thread::scope({
        |scope| {
            scope
                .spawn(|| {
                    while let Some(mut block) = samples.pop() {
                        while let Err(SendError::Full(b)) = sender.try_send_vec(block) {
                            block = b;
                        }
                    }
                })
                .join()
                .unwrap();
        }
    });

    reader_handle.join().unwrap();
}
