// mod multi_threaded;
// mod realistic;
// mod single_threaded;

use std::collections::VecDeque;

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, PlotConfiguration,
};
use slicebuf::SliceChannelSender;
// use slicebuf::lock_free_ringbuf;

// fn single_threaded(c: &mut Criterion) {
//     let mut group = c.benchmark_group("single threaded");
//     group.sample_size(1000);

//     let n = 10000;
//     group.bench_function("std::VecDeque (sa)", |b| {
//         b.iter(|| single_threaded::single_alloc_std_vecdeque(black_box(n)))
//     });
//     group.bench_function("lf ringbuf (sa)", |b| {
//         b.iter(|| single_threaded::single_alloc_lf_ringbuf(black_box(n)))
//     });
//     group.bench_function("std::VecDeque", |b| {
//         b.iter(|| single_threaded::std_vecdeque(black_box(n)))
//     });
//     group.finish();
// }

// fn multi_threaded(c: &mut Criterion) {
//     let mut group = c.benchmark_group("multi threaded");

//     for n in [1_000, 10_000, 1_000_000] {
//         group.bench_with_input(
//             BenchmarkId::new("Mutex<std::VecDeque> (sa)", n),
//             &n,
//             |b, &n| b.iter(|| multi_threaded::single_alloc_std_vecdeque(black_box(n))),
//         );
//         group.bench_with_input(BenchmarkId::new("lf ringbuf (sa)", n), &n, |b, &n| {
//             b.iter(|| multi_threaded::single_alloc_lf_ringbuf(black_box(n)))
//         });
//         group.bench_with_input(BenchmarkId::new("Mutex<std::VecDeque>", n), &n, |b, &n| {
//             b.iter(|| multi_threaded::std_vecdeque(black_box(n)))
//         });
//     }

//     group.finish();
// }

// fn realistic(c: &mut Criterion) {
//     let mut group = c.benchmark_group("realistic");
//     // group.sample_size(1000);

//     // Create samples
//     let seconds = 10;
//     let samples_size = seconds * realistic::SAMPLE_RATE;
//     let mut samples: Vec<Vec<f32>> = vec![Vec::new()];

//     let base_range = -1000..1000;
//     base_range
//         .clone()
//         .chain(base_range.rev())
//         .map(|s| s as f32 / 1000.0)
//         .cycle()
//         .take(samples_size)
//         // no chunks or window methods on Iterator :'(
//         .for_each(|sample| {
//             if samples.last().unwrap().len() < realistic::INPUT_BLOCK_SIZE {
//                 samples.last_mut().unwrap().push(sample);
//             } else {
//                 let mut new_vec = Vec::with_capacity(realistic::INPUT_BLOCK_SIZE);
//                 new_vec.push(sample);
//                 samples.push(new_vec);
//             }
//         });

//     // Reads don't do any actual work. They just drain the buffer.

//     group.bench_function("Mutex<std::VecDeque>", |b| {
//         b.iter_batched(
//             || samples.clone(),
//             |mut samples| {
//                 realistic::std_vecdeque(&mut samples);
//                 samples
//             },
//             criterion::BatchSize::SmallInput,
//         )
//     });
//     group.bench_function("lf ringbuf", |b| {
//         b.iter_batched(
//             || samples.clone(),
//             |mut samples| {
//                 realistic::lf_ringbuf(&mut samples);
//                 samples
//             },
//             criterion::BatchSize::SmallInput,
//         )
//     });

//     // Read bytes are actually processed, which should increase the time spent on the read thread.

//     group.bench_function("Mutex<std::VecDeque> simulate work", |b| {
//         b.iter_batched(
//             || samples.clone(),
//             |mut samples| {
//                 realistic::std_vecdeque_do_work(&mut samples);
//                 samples
//             },
//             criterion::BatchSize::SmallInput,
//         )
//     });
//     group.bench_function("explicit sync simulate work", |b| {
//         b.iter_batched(
//             || samples.clone(),
//             |mut samples| {
//                 realistic::lf_ringbuf_do_work(&mut samples);
//                 samples
//             },
//             criterion::BatchSize::SmallInput,
//         )
//     });

//     group.finish();
// }

/// Benchmarking basic operations on `SliceChannelReceiver` and `SliceChannelSender`
/// implementations.
///
/// Includes benchmarks for `std::collections::VecDeque` as a baseline. Everything is run in a
/// single thread and care is take to measure just the operations, not any setup or teardown
/// costs.
fn basic_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("basic operations");
    group.sample_size(1000);

    // Bench insertions of multiple elements
    for i in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("std::VecDeque::push", i), &i, |b, &i| {
            b.iter_batched(
                || (VecDeque::with_capacity(1000), (0..i).collect::<Vec<_>>()),
                |(mut deque, data)| {
                    deque.extend(data);
                    deque
                },
                criterion::BatchSize::SmallInput,
            )
        });

        bench_sender_append(&mut group, "SimpleSender", i, || {
            slicebuf::simple_internals::create_simple_channel(1000)
        });
        bench_sender_append(&mut group, "Rtrb", i, || {
            slicebuf::rtrb::create_bounded(1000)
        });
    }
    // group.bench_function("slice std::VecDeque", |b| {
    //     b.iter_batched(
    //         || VecDeque::from_iter(0..100),
    //         |mut deque| {
    //             let s = &deque.make_contiguous()[0..20];
    //             std::hint::black_box(s);
    //             deque
    //         },
    //         criterion::BatchSize::SmallInput,
    //     )
    // });
    // group.bench_function("pop std::VecDeque", |b| {
    //     b.iter_batched(
    //         || VecDeque::from_iter(0..10),
    //         |mut deque| {
    //             let a = deque.pop_front();
    //             std::hint::black_box(a);
    //             deque
    //         },
    //         criterion::BatchSize::SmallInput,
    //     )
    // });
    // group.bench_function("push lf_ringbuf::Writer", |b| {
    //     b.iter_batched(
    //         || lock_free_ringbuf::create_bounded(1),
    //         |(mut sender, recv)| {
    //             let s = sender.try_send(black_box(1.0));
    //             std::hint::black_box(s);
    //             (sender, recv)
    //         },
    //         criterion::BatchSize::SmallInput,
    //     )
    // });
    // group.bench_function("slice lf_ringbuf::Writer", |b| {
    //     b.iter_batched(
    //         || {
    //             let n = 100;
    //             let (mut sender, recv) = lock_free_ringbuf::create_bounded(n);
    //             for i in 0..n {
    //                 sender.try_send(i);
    //             }
    //             (sender, recv)
    //         },
    //         |(_sender, recv)| {
    //             let s = recv.peek_cow_exact(std::hint::black_box(20));
    //             std::hint::black_box(s);
    //             (_sender, recv)
    //         },
    //         criterion::BatchSize::SmallInput,
    //     )
    // });
    // group.bench_function("pop lf_ringbuf::Writer", |b| {
    //     b.iter_batched(
    //         || {
    //             let n = 10;
    //             let (mut sender, recv) = lock_free_ringbuf::create_bounded(n);
    //             sender.try_send_iter(0..n);
    //             (sender, recv)
    //         },
    //         |(_sender, mut recv)| {
    //             let a = recv.recv_up_to(1);
    //             std::hint::black_box(a);
    //             (_sender, recv)
    //         },
    //         criterion::BatchSize::SmallInput,
    //     )
    // });

    group.finish();
}

// Bench appending to an empty `slicebuf::SliceChannelSender`
fn bench_sender_append<R, S>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    input_size: usize,
    create: impl Fn() -> (S, R),
) where
    S: slicebuf::SliceChannelSender<i32>,
    R: slicebuf::SliceChannelReceiver<i32>,
{
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::append"), input_size),
        &input_size,
        |b, &input_size| {
            b.iter_batched(
                || (create(), (0..input_size as i32).collect::<Vec<i32>>()),
                |((mut send, _recv), data)| {
                    send.append(black_box(data));
                    (send, _recv)
                },
                criterion::BatchSize::SmallInput,
            )
        },
    );
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::try_append"), input_size),
        &input_size,
        |b, &input_size| {
            b.iter_batched(
                || (create(), (0..input_size as i32).collect::<Vec<_>>()),
                |((mut send, _recv), data)| {
                    send.try_append(black_box(data)).unwrap();
                    (send, _recv)
                },
                criterion::BatchSize::SmallInput,
            )
        },
    );
}

// Bench slicing and popping from a `slicebuf::SliceChannelReceiver`
// TODO: These aren't done, should be obvious which
fn bench_sender_receiver<R, S>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    out_size: usize,
    create: impl Fn(usize) -> (S, R),
) where
    S: slicebuf::SliceChannelSender<i32>,
    R: slicebuf::SliceChannelReceiver<i32>,
{
    let create_full = |n| {
        let (mut send, recv) = create(n);
        send.append(0..n as i32);
        (send, recv)
    };

    // slice
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::"), out_size),
        &out_size,
        |b, &out_size| {
            b.iter_batched(
                || create_full(out_size),
                |(_send, mut recv)| {
                    let s = recv.slice(black_box(0..out_size));
                    black_box(s);
                },
                criterion::BatchSize::SmallInput,
            )
        },
    );

    // try_slice
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::"), out_size),
        &out_size,
        |b, &out_size| {
            b.iter_batched(
                || create_full(out_size),
                |(_send, recv)| {},
                criterion::BatchSize::SmallInput,
            )
        },
    );

    // pop
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::"), out_size),
        &out_size,
        |b, &out_size| {
            b.iter_batched(
                || create_full(out_size),
                |(_send, recv)| {},
                criterion::BatchSize::SmallInput,
            )
        },
    );

    // try_pop
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::"), out_size),
        &out_size,
        |b, &out_size| {
            b.iter_batched(
                || create_full(out_size),
                |(_send, recv)| {},
                criterion::BatchSize::SmallInput,
            )
        },
    );

    // consume_exact
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::"), out_size),
        &out_size,
        |b, &out_size| {
            b.iter_batched(
                || create_full(out_size),
                |(_send, recv)| {},
                criterion::BatchSize::SmallInput,
            )
        },
    );

    // try_consume_exact
    group.bench_with_input(
        BenchmarkId::new(format!("{name}::"), out_size),
        &out_size,
        |b, &out_size| {
            b.iter_batched(
                || create_full(out_size),
                |(_send, recv)| {},
                criterion::BatchSize::SmallInput,
            )
        },
    );
}

// fn push_multiple(c: &mut Criterion) {
//     let plot_config = PlotConfiguration::default().summary_scale(criterion::AxisScale::Logarithmic);
//     let mut group = c.benchmark_group("push multiple");
//     group.plot_config(plot_config);

//     for n in [100, 10_000, 1_000_000] {
//         group.bench_with_input(
//             BenchmarkId::new("push lf_ringbuf::Sender", n),
//             &n,
//             |b, &n| {
//                 b.iter_batched(
//                     || {
//                         let (sender, receiver) = lock_free_ringbuf::create_bounded(n);
//                         let input = (0..n).collect::<Vec<_>>();
//                         (sender, receiver, input)
//                     },
//                     |(mut sender, _receiver, input)| {
//                         _ = sender.try_send_vec(input);
//                         (sender, _receiver)
//                     },
//                     criterion::BatchSize::SmallInput,
//                 )
//             },
//         );
//         group.bench_with_input(BenchmarkId::new("push std::VecDeque", n), &n, |b, &n| {
//             b.iter_batched(
//                 || {
//                     let deque = VecDeque::with_capacity(n);
//                     let input = (0..n).collect::<Vec<_>>();
//                     (deque, input)
//                 },
//                 |(mut deque, input)| {
//                     deque.extend(input);
//                     deque
//                 },
//                 criterion::BatchSize::SmallInput,
//             )
//         });
//     }

//     group.finish()
// }

criterion_group!(
    benches,
    basic_operations, // push_multiple,
                      // single_threaded,
                      // multi_threaded,
                      // realistic
);
criterion_main!(benches);
