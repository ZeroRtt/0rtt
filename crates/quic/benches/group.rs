use divan::Bencher;
use zerortt::poll::Group;

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 10000, threads = 10)]
fn bench_poll(bencher: Bencher) {
    let group = Group::new();

    bencher.bench(|| {
        let mut events = vec![];
        group.poll(&mut events);
    });
}
