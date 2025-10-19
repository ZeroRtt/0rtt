use std::time::Instant;

use divan::bench;

use metricrs::{global::set_global_registry, instrument, memory::MemoryRegistry};

fn main() {
    _ = set_global_registry(MemoryRegistry::default());
    divan::main();
}

#[instrument(
    kind = Counter,
    name = "test.mock_send",
    labels(
        name = "hello",
        color = "red"
    )
)]
fn mock_counter() -> usize {
    1
}

#[instrument(
    kind = Timer,
    name = "test.timer",
    labels(
        name = "pick"
    )
)]
fn mock_timer() -> usize {
    1
}

#[bench(threads = 0, sample_count = 10000)]
fn bench_counter() {
    mock_counter();
}

#[bench(threads = 0, sample_count = 10000)]
fn bench_timer() {
    mock_timer();
}

#[bench(threads = 0, sample_count = 10000)]
fn bench_instant_now() {
    _ = Instant::now().elapsed();
}
