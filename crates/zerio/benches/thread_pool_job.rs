use divan::Bencher;
use rand::seq::SliceRandom;
use zerio::{concurrency::JobState, executor::Job};

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 50000, threads = num_cpus::get())]
fn bench_thread_pool_job_state(bencher: Bencher) {
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
            let job = Job::with_state(0.into(), Box::new(async {}).into(), states[0]);
            states.shuffle(&mut rand::rng());

            (job, states[0])
        })
        .bench_values(|(job, state)| job.update(state));
}
