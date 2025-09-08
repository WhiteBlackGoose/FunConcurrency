use std::{
    sync::{atomic::AtomicUsize, Mutex},
    thread,
};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rst_test::AVec;

fn bench_push(c: &mut Criterion) {
    let el_count = 10000;
    for thread_count in [1, 4, 12] {
        for cap in [1, el_count] {
            let mut group = c.benchmark_group(format!("::push:{}@{}", cap, thread_count));
            group.bench_with_input(BenchmarkId::new("Mutex<Vec<T>>", ""), &(), |b, _| {
                b.iter(|| {
                    let vec = Mutex::new(Vec::with_capacity(cap * thread_count));
                    thread::scope(|s| {
                        for _ in 0..thread_count {
                            s.spawn(|| {
                                for i in 0..el_count {
                                    vec.lock().unwrap().push(i);
                                }
                            });
                        }
                    });
                })
            });

            group.bench_with_input(BenchmarkId::new("AVec<T>", ""), &(), |b, _| {
                b.iter(|| {
                    let vec = AVec::new(cap * thread_count);
                    thread::scope(|s| {
                        for _ in 0..thread_count {
                            s.spawn(|| {
                                for i in 0..el_count {
                                    vec.push(i);
                                }
                            });
                        }
                    });
                })
            });
            group.finish();
        }
    }
}

fn bench_get(c: &mut Criterion) {
    let el_count = 30000;

    let vec: Mutex<Vec<_>> = Mutex::new((0..el_count).collect());
    let avec = AVec::new(el_count);
    for i in 0..el_count {
        avec.push(i);
    }
    for thread_count in [1, 4, 12] {
        let mut group = c.benchmark_group(format!("::get:@{}", thread_count));
        group.bench_with_input(BenchmarkId::new("Mutex<Vec<T>>", ""), &(), |b, _| {
            b.iter(|| {
                let sum = AtomicUsize::new(0);
                thread::scope(|s| {
                    for _ in 0..thread_count {
                        s.spawn(|| {
                            for i in 0..el_count {
                                sum.fetch_add(
                                    vec.lock().unwrap()[i],
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                            }
                        });
                    }
                });
            })
        });

        group.bench_with_input(BenchmarkId::new("AVec<T>", ""), &(), |b, _| {
            b.iter(|| {
                let sum = AtomicUsize::new(0);
                thread::scope(|s| {
                    for _ in 0..thread_count {
                        s.spawn(|| {
                            for i in 0..el_count {
                                sum.fetch_add(
                                    *avec.get(i).unwrap(),
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                            }
                        });
                    }
                });
            })
        });
        group.finish();
    }
}

fn tuned() -> Criterion {
    Criterion::default().sample_size(300)
}

criterion_group! {
    name = benches;
    config = tuned();
    targets = bench_push, bench_get
}
criterion_main!(benches);
