use std::{
    cmp,
    time::{Duration, Instant},
};

use divan::Bencher;
use quiche_poll::poll::deadline::Deadline;

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 1000)]
fn bench_deadline_insert(bencher: Bencher) {
    let mut deadline = Deadline::default();

    let now = Instant::now();
    let mut i = 0;
    bencher.bench_local(|| {
        deadline.insert(i, now + Duration::from_secs(i as u64));
        i += 1;
    });
}

#[divan::bench(sample_count = 1000)]
fn bench_deadline_insert_same_one(bencher: Bencher) {
    let mut deadline = Deadline::default();

    let now = Instant::now();
    let mut i = 0;
    bencher.bench_local(|| {
        deadline.insert(0, now + Duration::from_secs(i as u64));
        i += 1;
    });
}

#[divan::bench(sample_count = 1000)]
fn bench_deadline_timeout(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let mut deadline = Deadline::default();

            let now = Instant::now();

            let range_max = 1000;

            for i in 0..range_max {
                deadline.insert(i, now + Duration::from_secs(i as u64));
            }

            (deadline, now, range_max)
        })
        .bench_values(|(mut deadline, now, range_max)| {
            for _ in deadline.timeout(now + Duration::from_secs(cmp::min(100, range_max) as u64)) {}
        });
}
