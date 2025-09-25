use std::{
    cell::UnsafeCell,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    usize,
};

use crossbeam::channel::Sender;
use futures_task::{ArcWake, FutureObj};

use crate::concurrency::{JobKey, JobState};

/// Represents an asynchronous task.
pub struct JobHandle {
    key: JobKey,
    state: AtomicUsize,
    job: UnsafeCell<Option<Job>>,
}

unsafe impl Sync for JobHandle {}

impl Debug for JobHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobHandle")
            .field("key", &self.key())
            .field("state", &JobState::from(self.state.load(Ordering::Relaxed)))
            .finish_non_exhaustive()
    }
}

impl JobHandle {
    pub fn new(key: JobKey) -> Self {
        Self {
            key,
            state: AtomicUsize::new(JobState::Ready as usize),
            job: UnsafeCell::new(None),
        }
    }

    pub fn with_state(key: JobKey, state: JobState) -> Self {
        Self {
            key,
            state: AtomicUsize::new(state as usize),
            job: UnsafeCell::new(None),
        }
    }

    #[inline]
    pub fn key(&self) -> JobKey {
        self.key
    }

    #[inline]
    pub fn update(&self, state: JobState) -> Result<JobState, JobState> {
        match state {
            JobState::Active => self.active(),
            JobState::Completed => self.complete(),
            JobState::Cancelled => self.cancel(),
            _ => {
                unreachable!()
            }
        }
    }

    #[inline]
    pub fn to_ready(&self) -> Result<(JobState, Option<Job>), JobState> {
        let pre_stats = JobState::Active | JobState::Pending;

        let mut current = JobState::Pending as usize;

        while current & pre_stats != 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Ready as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    let job = unsafe { (*self.job.get()).take() };

                    if current == JobState::Pending as usize {
                        return Ok((current.into(), Some(job.expect("Job"))));
                    } else {
                        assert!(job.is_none(), "Unexpect job.");
                        return Ok((current.into(), None));
                    }
                }

                Err(state) => current = state,
            }
        }

        Err(current.into())
    }

    #[inline]
    pub fn to_pending(&self, job: Job) -> Result<JobState, (JobState, Job)> {
        unsafe {
            *self.job.get() = Some(job);
        }

        let pre_stats = JobState::Active as usize;
        let mut current = JobState::Active as usize;

        while current & pre_stats != 0 {
            match self.state.compare_exchange_weak(
                current,
                JobState::Pending as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(current.into());
                }
                Err(state) => current = state,
            }
        }

        let job = unsafe { (*self.job.get()).take().unwrap() };

        Err((current.into(), job))
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

/// A job created by `ThreadPool`.

pub struct Job {
    future: FutureObj<'static, ()>,
    handle: Arc<JobHandle>,
    sender: Sender<Job>,
}

impl Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("key", &self.handle.key())
            .field(
                "state",
                &JobState::from(self.handle.state.load(Ordering::Relaxed)),
            )
            .finish_non_exhaustive()
    }
}

impl Job {
    /// Create a new `Job` for `ThreadPool`
    pub fn new(key: JobKey, future: FutureObj<'static, ()>, sender: Sender<Job>) -> Self {
        Self {
            future,
            handle: Arc::new(JobHandle::new(key)),
            sender,
        }
    }

    #[inline]
    pub fn to_pending(
        handle: Arc<JobHandle>,
        future: FutureObj<'static, ()>,
        sender: Sender<Job>,
    ) -> Result<JobState, (JobState, Job)> {
        handle.clone().to_pending(Self {
            future,
            handle,
            sender,
        })
    }

    #[inline]
    pub fn to_handle(&self) -> Arc<JobHandle> {
        self.handle.clone()
    }

    #[inline]
    pub fn active(
        self,
    ) -> Result<(FutureObj<'static, ()>, Arc<JobHandle>, Sender<Job>), (JobState, Self)> {
        if let Err(err) = self.handle.active() {
            return Err((err, self));
        }

        Ok((self.future, self.handle, self.sender))
    }
}

impl ArcWake for JobHandle {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        match arc_self.to_ready() {
            Ok((state, Some(job))) => match job.sender.clone().send(job) {
                Ok(_) => {
                    log::trace!("{:?}, wake job, state={:?}", arc_self.as_ref(), state);
                }
                Err(err) => {
                    log::error!(
                        "{:?}, wake job, state={:?}, err={}",
                        arc_self.as_ref(),
                        state,
                        err
                    );
                }
            },
            Ok((state, None)) => {
                log::trace!(
                    "{:?}, failed to wake, state({:?}), job is none",
                    arc_self.as_ref(),
                    state
                );
            }
            Err(state) => {
                log::trace!(
                    "{:?}, failed to wake, state({:?})",
                    arc_self.as_ref(),
                    state
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossbeam::channel::unbounded;

    use crate::{
        concurrency::JobState,
        executor::{Job, JobHandle},
    };

    #[test]
    fn test_to_ready() {
        let handle = JobHandle::with_state(0.into(), JobState::Active);

        let (state, job) = handle.to_ready().expect("");

        assert_eq!(state, JobState::Active);

        assert!(job.is_none());

        let handle = Arc::new(JobHandle::with_state(0.into(), JobState::Active));

        let (sender, _) = unbounded();

        assert_eq!(
            handle
                .to_pending(Job {
                    handle: handle.clone(),
                    future: Box::new(async {}).into(),
                    sender
                })
                .unwrap(),
            JobState::Active
        );

        let (state, job) = handle.to_ready().expect("");

        assert_eq!(state, JobState::Pending);

        assert!(job.is_some());
    }

    #[test]
    fn test_cancel_success() {
        for state in [JobState::Active, JobState::Ready, JobState::Pending] {
            let job = JobHandle::with_state(0.into(), state);

            assert_eq!(job.cancel(), Ok(state));
        }
    }

    #[test]
    fn test_cancel_failed() {
        for state in [JobState::Cancelled, JobState::Completed] {
            let job = JobHandle::with_state(0.into(), state);

            assert_eq!(job.cancel(), Err(state));
        }
    }

    #[test]
    fn test_active_success() {
        for state in [JobState::Ready] {
            let job = JobHandle::with_state(0.into(), state);

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
            let job = JobHandle::with_state(0.into(), state);

            assert_eq!(job.active(), Err(state));
        }
    }

    #[test]
    fn test_complete_success() {
        for state in [JobState::Active, JobState::Ready] {
            let job = JobHandle::with_state(0.into(), state);

            assert_eq!(job.complete(), Ok(state));
        }
    }

    #[test]
    fn test_complete_failed() {
        for state in [JobState::Pending, JobState::Cancelled, JobState::Completed] {
            let job = JobHandle::with_state(0.into(), state);

            assert_eq!(job.complete(), Err(state));
        }
    }
}
