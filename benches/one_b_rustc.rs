use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;

fn brc_benches(c: &mut Criterion) {
    let path = std::env::var("BRC_FILE").unwrap_or_else(|_| "measurements.txt".to_string());

    let mut group = c.benchmark_group("brc");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(20));

    group.bench_function("worker_no_output", |b| {
        b.iter(|| {
            let res = one_b_rustc::run_worker(black_box(&path), true).unwrap();
            black_box(res.checksum)
        });
    });

    group.bench_function("worker_with_output", |b| {
        b.iter(|| {
            let res = one_b_rustc::run_worker(black_box(&path), false).unwrap();
            black_box(res.output.as_ref().map(|o| o.len()).unwrap_or(0))
        });
    });

    group.finish();
}

criterion_group!(benches, brc_benches);
criterion_main!(benches);
