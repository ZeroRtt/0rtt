use std::time::Instant;

use divan::Bencher;
use quico::{Event, EventKind, Readiness, Token};

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 10000)]
fn bench_insert_without_delay(bencher: Bencher) {
    let mut readiness = Readiness::default();

    bencher.bench_local(|| {
        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: false,
                token: Token(0),
                stream_id: 0,
            },
            None,
        );
    });
}

#[divan::bench(sample_count = 10000)]
fn bench_insert_with_delay(bencher: Bencher) {
    let mut readiness = Readiness::default();

    let delay_to = Instant::now();

    bencher.bench_local(|| {
        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: false,
                token: Token(0),
                stream_id: 0,
            },
            Some(delay_to),
        );
    });
}

#[divan::bench(sample_count = 10000)]
fn bench_poll_empty(bencher: Bencher) {
    let mut readiness = Readiness::default();

    let mut events = vec![];

    bencher.bench_local(|| {
        readiness.poll(&mut events);
    });
}

#[divan::bench(sample_count = 10000)]
fn bench_poll(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let mut readiness = Readiness::default();

            readiness.insert(
                Event {
                    kind: EventKind::Send,
                    is_server: false,
                    token: Token(0),
                    stream_id: 0,
                },
                Some(Instant::now()),
            );

            let events = vec![];

            (events, readiness)
        })
        .bench_local_values(|(mut events, mut readiness)| {
            readiness.poll(&mut events);
        });
}
