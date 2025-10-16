use divan::bench;
use metricrs::instrument;

fn main() {
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
