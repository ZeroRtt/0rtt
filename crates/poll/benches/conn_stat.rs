use divan::Bencher;
use quiche::Config;
use quiche_poll::poll::conn::{ConnState, Lockind};

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 10000)]
fn bench_lock(bencher: Bencher) {
    let scid = quiche::ConnectionId::from_ref(b"");
    let mut state = ConnState::new(
        0,
        quiche::connect(
            None,
            &scid,
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
            &mut Config::new(quiche::PROTOCOL_VERSION).unwrap(),
        )
        .unwrap(),
    );

    bencher.bench_local(|| {
        let guard = state.try_lock(Lockind::Send).unwrap();
        state.unlock(guard.lock_count, |_| {});
    });
}
