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
