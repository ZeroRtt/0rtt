use std::{collections::HashMap, pin::Pin, sync::Arc, task::Context, thread::spawn};

use crossbeam::channel::{Receiver, Sender, unbounded};
use futures_task::{FutureObj, waker_ref};
use parking_lot::Mutex;

use crate::{
    concurrency::JobState,
    executor::{Job, JobHandle},
};

#[derive(Default)]
struct JobPool {
    job_key_next: usize,
    jobs: HashMap<usize, Arc<JobHandle>>,
}

impl JobPool {
    fn new_job(&mut self, task: FutureObj<'static, ()>, sender: Sender<Job>) -> Job {
        loop {
            let key = self.job_key_next;

            (self.job_key_next, _) = self.job_key_next.overflowing_add(1);

            if self.jobs.contains_key(&key) {
                continue;
            }

            let job = Job::new(key.into(), task, sender);

            self.jobs.insert(key, job.to_handle());

            return job;
        }
    }
}

/// Asynchronous task scheduler based on thread pools.
pub struct ThreadPool {
    sender: Sender<Job>,
    job_pool: Mutex<JobPool>,
}

impl ThreadPool {
    /// Create a new `ThreadPool` instance with default parameters.
    pub fn new() -> Self {
        ThreadPoolBuilder::new().create()
    }

    /// Create a `builder` for [`ThreadPool`].
    pub fn build() -> ThreadPoolBuilder {
        ThreadPoolBuilder::new()
    }

    pub fn spawn(&self, task: FutureObj<'static, ()>) {
        let job = self.job_pool.lock().new_job(task, self.sender.clone());
        self.sender.send(job).expect("ThreadPool: dispatch.");
    }
}

/// A builder for [`ThreadPool`].
pub struct ThreadPoolBuilder {
    pool_size: usize,
}

impl ThreadPoolBuilder {
    fn new() -> Self {
        Self {
            pool_size: num_cpus::get(),
        }
    }

    pub fn create(self) -> ThreadPool {
        let (sender, receiver) = unbounded();

        for _ in 0..self.pool_size {
            let receiver = receiver.clone();
            spawn(|| thread_pool_schedule(receiver));
        }

        ThreadPool {
            sender,
            job_pool: Default::default(),
        }
    }
}

/// task scheduler.
fn thread_pool_schedule(_receiver: Receiver<Job>) {
    for mut job in _receiver.into_iter() {
        loop {
            let (mut future, handle, sender) = job.active().expect("Active job");

            let waker = waker_ref(&handle);
            let mut cx = Context::from_waker(&waker);

            match Pin::new(&mut future).poll(&mut cx) {
                std::task::Poll::Ready(_) => {
                    _ = handle.complete();
                }
                std::task::Poll::Pending => {}
            }

            match Job::to_pending(handle, future, sender) {
                Ok(_) => {
                    break;
                }
                Err((state, next)) => {
                    if state == JobState::Ready {
                        job = next;
                    } else {
                        break;
                    }
                }
            }
        }
    }
}
