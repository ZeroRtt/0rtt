use std::sync::atomic::{AtomicUsize, Ordering};

use futures_task::FutureObj;

use crate::concurrency::{JobKey, JobState};

/// Represents an asynchronous task.
pub struct Job {
    #[allow(unused)]
    key: JobKey,
    state: AtomicUsize,
    #[allow(unused)]
    task: FutureObj<'static, ()>,
}

unsafe impl Sync for Job {}

impl Job {
    pub fn new(key: JobKey, task: FutureObj<'static, ()>) -> Self {
        Self {
            key,
            task,
            state: AtomicUsize::new(JobState::Ready as usize),
        }
    }

    pub fn with_state(key: JobKey, task: FutureObj<'static, ()>, state: JobState) -> Self {
        Self {
            key,
            task,
            state: AtomicUsize::new(state as usize),
        }
    }

    #[inline]
    pub fn update(&self, state: JobState) -> Result<JobState, JobState> {
        match state {
            JobState::Ready => self.to_ready(),
            JobState::Active => self.active(),
            JobState::Pending => self.to_pending(),
            JobState::Completed => self.complete(),
            JobState::Cancelled => self.cancel(),
        }
    }

    #[inline]
    pub fn to_ready(&self) -> Result<JobState, JobState> {
        let pre_stats = JobState::Active | JobState::Pending;

        let mut current = JobState::Pending as usize;

        while current & pre_stats != 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Ready as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(current.into()),
                Err(state) => current = state,
            }
        }

        Err(current.into())
    }

    #[inline]
    pub fn to_pending(&self) -> Result<JobState, JobState> {
        let pre_stats = JobState::Active as usize;

        let mut current = JobState::Active as usize;

        while current & pre_stats != 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Pending as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(current.into()),
                Err(state) => current = state,
            }
        }

        Err(current.into())
    }

    #[inline]
    pub fn cancel(&self) -> Result<JobState, JobState> {
        let pre_stats = JobState::Cancelled | JobState::Completed;

        let mut current = JobState::Ready as usize;

        while current & pre_stats == 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Cancelled as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(current.into()),
                Err(state) => current = state,
            }
        }

        Err(current.into())
    }

    #[inline]
    pub fn active(&self) -> Result<JobState, JobState> {
        let pre_stats = JobState::Ready as usize;

        let mut current = JobState::Ready as usize;

        while current & pre_stats != 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Active as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(current.into()),
                Err(state) => current = state,
            }
        }

        Err(current.into())
    }

    #[inline]
    pub fn complete(&self) -> Result<JobState, JobState> {
        let pre_stats = JobState::Active | JobState::Ready;

        let mut current = JobState::Active as usize;

        while current & pre_stats != 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Completed as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(current.into()),
                Err(state) => current = state,
            }
        }

        Err(current.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::{concurrency::JobState, executor::Job};

    #[test]
    fn test_to_ready_success() {
        for state in [JobState::Pending, JobState::Active] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.to_ready(), Ok(state));
        }
    }

    #[test]
    fn test_to_ready_failed() {
        for state in [JobState::Ready, JobState::Cancelled, JobState::Completed] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.to_ready(), Err(state));
        }
    }

    #[test]
    fn test_to_pending_success() {
        for state in [JobState::Active] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.to_pending(), Ok(state));
        }
    }

    #[test]
    fn test_to_pending_failed() {
        for state in [
            JobState::Ready,
            JobState::Pending,
            JobState::Cancelled,
            JobState::Completed,
        ] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.to_pending(), Err(state));
        }
    }

    #[test]
    fn test_cancel_success() {
        for state in [JobState::Active, JobState::Ready, JobState::Pending] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.cancel(), Ok(state));
        }
    }

    #[test]
    fn test_cancel_failed() {
        for state in [JobState::Cancelled, JobState::Completed] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.cancel(), Err(state));
        }
    }

    #[test]
    fn test_active_success() {
        for state in [JobState::Ready] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.active(), Ok(state));
        }
    }

    #[test]
    fn test_active_failed() {
        for state in [
            JobState::Active,
            JobState::Pending,
            JobState::Cancelled,
            JobState::Completed,
        ] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.active(), Err(state));
        }
    }

    #[test]
    fn test_complete_success() {
        for state in [JobState::Active, JobState::Ready] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.complete(), Ok(state));
        }
    }

    #[test]
    fn test_complete_failed() {
        for state in [JobState::Pending, JobState::Cancelled, JobState::Completed] {
            let job = Job::with_state(0.into(), Box::new(async {}).into(), state);

            assert_eq!(job.complete(), Err(state));
        }
    }
}
