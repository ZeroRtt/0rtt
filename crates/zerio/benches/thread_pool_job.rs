use divan::Bencher;
use rand::seq::SliceRandom;
use zerio::{concurrency::JobState, executor::JobHandle};

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 50000, threads = num_cpus::get())]
fn update_state(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let mut states = [
                JobState::Ready,
                JobState::Active,
                JobState::Pending,
                JobState::Cancelled,
                JobState::Completed,
            ];

            states.shuffle(&mut rand::rng());
            let job = JobHandle::with_state(0.into(), states[0]);
            let mut states = [JobState::Active, JobState::Cancelled, JobState::Completed];
            states.shuffle(&mut rand::rng());

            (job, states[0])
        })
        .bench_values(|(job, state)| job.update(state));
}
