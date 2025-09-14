use std::time::Duration;

use divan::Bencher;
use quiche::Config;
use zrquic::Group;

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 10000, threads = 10)]
fn bench_poll(bencher: Bencher) {
    let group = Group::new();

    bencher.bench(|| {
        let mut events = vec![];
        group.poll(&mut events, Some(Duration::ZERO)).unwrap();
    });
}

#[divan::bench(sample_count = 10000)]
fn bench_poll_local(bencher: Bencher) {
    let group = Group::new();

    bencher.bench_local(|| {
        let mut events = vec![];
        group.poll(&mut events, Some(Duration::ZERO)).unwrap();
    });
}

#[divan::bench(sample_count = 10000, threads = 10)]
fn bench_send(bencher: Bencher) {
    let group = Group::new();
    let token = group
        .connect(
            None,
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
            &mut Config::new(quiche::PROTOCOL_VERSION).unwrap(),
        )
        .unwrap();

    bencher.bench(|| {
        let mut buf = vec![0; 1300];
        let _ = group.send(token, &mut buf);
    });
}

#[divan::bench(sample_count = 10000)]
fn bench_send_local(bencher: Bencher) {
    let group = Group::new();
    let token = group
        .connect(
            None,
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
            &mut Config::new(quiche::PROTOCOL_VERSION).unwrap(),
        )
        .unwrap();

    bencher.bench_local(|| {
        let mut buf = vec![0; 1300];
        let _ = group.send(token, &mut buf);
    });
}
