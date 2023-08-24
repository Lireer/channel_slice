mod multi_threaded;
mod realistic;
mod single_threaded;

use std::collections::VecDeque;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use slicebuf::expl_sync;

fn single_threaded(c: &mut Criterion) {
    let mut group = c.benchmark_group("single threaded");
    group.sample_size(1000);

    let n = 10000;
    group.bench_function("std::VecDeque (sa)", |b| {
        b.iter(|| single_threaded::single_alloc_std_vecdeque(black_box(n)))
    });
    group.bench_function("explicit sync (sa)", |b| {
        b.iter(|| single_threaded::single_alloc_explicit_sync(black_box(n)))
    });
    group.bench_function("std::VecDeque", |b| {
        b.iter(|| single_threaded::std_vecdeque(black_box(n)))
    });
    group.bench_function("explicit sync", |b| {
        b.iter(|| single_threaded::explicit_sync(black_box(n)))
    });
    group.finish();
}

fn multi_threaded(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi threaded");
    group.sample_size(1000);

    for n in [1_000, 10_000, 1_000_000] {
        group.bench_with_input(
            BenchmarkId::new("Mutex<std::VecDeque> (sa)", n),
            &n,
            |b, &n| b.iter(|| multi_threaded::single_alloc_std_vecdeque(black_box(n))),
        );
        group.bench_with_input(BenchmarkId::new("explicit sync (sa)", n), &n, |b, &n| {
            b.iter(|| multi_threaded::single_alloc_explicit_sync(black_box(n)))
        });
        group.bench_with_input(BenchmarkId::new("Mutex<std::VecDeque>", n), &n, |b, &n| {
            b.iter(|| multi_threaded::std_vecdeque(black_box(n)))
        });
        group.bench_with_input(BenchmarkId::new("explicit sync", n), &n, |b, &n| {
            b.iter(|| multi_threaded::explicit_sync(black_box(n)))
        });
    }

    group.finish();
}

fn realistic(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic");
    group.sample_size(1000);

    // Create samples
    let seconds = 10;
    let samples_size = seconds * realistic::SAMPLE_RATE;
    let samples: Vec<f32> = (-1000..1000)
        .map(|s| s as f32 / 1000.0)
        .cycle()
        .take(samples_size)
        .collect();

    group.bench_function("Mutex<std::VecDeque>", |b| {
        b.iter(|| realistic::std_vecdeque(black_box(&samples)))
    });
    group.bench_function("explicit sync", |b| {
        b.iter(|| realistic::explicit_sync(black_box(&samples)))
    });

    group.finish();
}

fn basic_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("basic operations");
    group.sample_size(1000);

    group.bench_function("push std::VecDeque", |b| {
        b.iter_batched(
            || VecDeque::with_capacity(1),
            |mut deque| {
                deque.push_back(black_box(1.0));
                deque
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.bench_function("slice std::VecDeque", |b| {
        b.iter_batched(
            || VecDeque::from_iter(0..100),
            |mut deque| {
                _ = deque.make_contiguous()[0..20];
                deque
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.bench_function("pop std::VecDeque", |b| {
        b.iter_batched(
            || VecDeque::from_iter(0..10),
            |mut deque| {
                deque.pop_front();
                deque
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.bench_function("push expl_sync::SliceBuf", |b| {
        b.iter_batched(
            || expl_sync::SliceBuf::with_capacity(1).split(),
            |(mut writer, reader)| {
                writer.push(black_box(1.0));
                (writer, reader)
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.bench_function("slice expl_sync::SliceBuf", |b| {
        b.iter_batched(
            || {
                let n = 100;
                let (mut writer, reader) = expl_sync::SliceBuf::with_capacity(n).split();
                for i in 0..n {
                    writer.push(i);
                }
                (writer, reader)
            },
            |(_writer, reader)| {
                _ = reader.slice_to(20);
                (_writer, reader)
            },
            criterion::BatchSize::SmallInput,
        )
    });
    group.bench_function("pop expl_sync::SliceBuf", |b| {
        b.iter_batched(
            || {
                let n = 10;
                let (mut writer, reader) = expl_sync::SliceBuf::with_capacity(n).split();
                for i in 0..n {
                    writer.push(i);
                }
                (writer, reader)
            },
            |(_writer, mut reader)| {
                reader.consume(1);
                (_writer, reader)
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish()
}

criterion_group!(
    benches,
    basic_operations,
    single_threaded,
    multi_threaded,
    realistic
);
criterion_main!(benches);
