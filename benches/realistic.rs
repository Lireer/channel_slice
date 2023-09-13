use std::{collections::VecDeque, sync::Arc, thread};

use parking_lot::Mutex;
use slicebuf::expl_sync;

// The use case we're benchmarking here is processing of audio samples. Example
// audio format is mono channel, 16kHz and bit depth of 32. Buffers will
// initially allocate memory for half a second of audio, i.e. 8000 samples/32000 bytes.
// Samples will be written in blocks of 40ms / 640 samples

pub const SAMPLE_RATE: usize = 16000;
const INITIAL_BUFFER_SIZE: usize = SAMPLE_RATE / 2;
pub const WRITE_SIZE: usize = SAMPLE_RATE / 25;
const READ_SIZE: usize = 1600;

pub fn std_vecdeque(samples: &mut Vec<Vec<f32>>) {
    let buf = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(
        INITIAL_BUFFER_SIZE,
    )));

    let reader_handle = thread::spawn({
        let mut drained = 0;
        let limit = samples.len() / READ_SIZE;
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

pub fn explicit_sync(samples: &mut Vec<Vec<f32>>) {
    let (mut writer, mut reader) =
        expl_sync::SliceBuf::<f32>::with_capacity(INITIAL_BUFFER_SIZE).split();

    let reader_handle = thread::spawn({
        let mut drained = 0;
        let limit = samples.len() / READ_SIZE;
        move || loop {
            if let Some(slice) = reader.slice_to(READ_SIZE) {
                drained += slice.len();
                reader.consume(slice.len());
                if drained >= limit {
                    break;
                }
            } else {
                reader.synchronize();
            }
        }
    });

    thread::scope({
        |scope| {
            scope
                .spawn(|| {
                    while let Some(block) = samples.pop() {
                        writer.push_vec(block);
                    }
                })
                .join()
                .unwrap();
        }
    });

    reader_handle.join().unwrap();
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
        let limit = samples.len() * WRITE_SIZE - READ_SIZE;
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

pub fn explicit_sync_do_work(samples: &mut Vec<Vec<f32>>) {
    let (mut writer, mut reader) =
        expl_sync::SliceBuf::<f32>::with_capacity(INITIAL_BUFFER_SIZE).split();

    let reader_handle = thread::spawn({
        // Try to process as many samples as possible but subtract READ_SIZE to avoid edge cases.
        let limit = samples.len() * WRITE_SIZE - READ_SIZE;

        let mut expensive_mode = false;
        let mut read = 0;
        let mut drained = 0;

        move || loop {
            // Here we simulate some expensive processing of the samples.
            if expensive_mode {
                spin_sleep::sleep(std::time::Duration::from_micros(40));
                let read_until = read + READ_SIZE / 2;
                if let Some(buf) = reader.slice_to(read_until) {
                    // Check if the average is now below -0.75, ...
                    if avg(&buf[read..read_until]) < -0.5 {
                        // if so, switch to non-expensive mode, draining all samples until read_until.
                        spin_sleep::sleep(std::time::Duration::from_micros(100));
                        expensive_mode = !expensive_mode;
                        let len = buf.len();
                        drained += len;
                        reader.consume(len);
                        read = 0;
                    } else {
                        // else just increase read.
                        read = read_until;
                    }
                } else {
                    reader.synchronize();
                }
            } else {
                // read is always 0 at this point because it's reset after switching and when already in this mode it's
                // never increased.
                if let Some(buf) = reader.slice_to(READ_SIZE) {
                    spin_sleep::sleep(std::time::Duration::from_micros(10));
                    // Check if the average is above 0.75, ...
                    if avg(buf) > 0.4 {
                        // if so, switch to expensive mode and mark these samples as already read.
                        expensive_mode = !expensive_mode;
                        read = READ_SIZE;
                    } else {
                        // else advance the window by half of READ_SIZE.
                        let len = READ_SIZE / 2;
                        drained += len;
                        reader.consume(len);
                    }
                } else {
                    reader.synchronize();
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
                        writer.push_vec(block);
                    }
                })
                .join()
                .unwrap();
        }
    });

    reader_handle.join().unwrap();
}
