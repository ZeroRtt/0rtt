use std::time::Instant;

use divan::bench;
use metricrs::{Token, instrument};
// use metricrs::{Token, global::set_global_registry, instrument, memory::MemoryRegistry};

fn main() {
    // _ = set_global_registry(MemoryRegistry);
    divan::main();
}

#[instrument(counter, "test.mock_send", name = "hello", color = "red")]
fn mock_counter() -> usize {
    1
}

#[instrument(timer, "test.mock_send", name = "hello")]
fn mock_timer() -> usize {
    1
}

#[bench]
fn bench_counter() {
    mock_counter();
}

#[bench]
fn bench_timer() {
    mock_timer();
}

#[bench]
fn bench_key() {
    Token::new("hello", &[("name", "hello"), ("module", "derive")]);
}

#[bench]
fn bench_instant_now() {
    _ = Instant::now().elapsed();
}
