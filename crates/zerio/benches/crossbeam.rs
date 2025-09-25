use std::sync::Arc;

use crossbeam::channel::unbounded;
use divan::Bencher;

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 50000, threads = num_cpus::get())]
fn clone(bencher: Bencher) {
    let (sender, _receiver) = unbounded::<()>();

    bencher.bench(|| sender.clone());
}

#[divan::bench(sample_count = 50000, threads = num_cpus::get())]
fn arc_clone(bencher: Bencher) {
    let (sender, _receiver) = unbounded::<()>();

    let sender = Arc::new(sender);

    bencher.bench(|| sender.clone());
}
