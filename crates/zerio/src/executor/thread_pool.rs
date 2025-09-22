use std::{
    mem::transmute,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::concurrency::TasKey;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[repr(usize)]
pub enum TaskState {
    Ready = 0,
    Active,
    Pending,
    Cancelling,
    Cancelled,
    Completed,
}

#[derive(Debug)]
pub struct TaskHandle {
    #[allow(unused)]
    key: TasKey,
    state: AtomicUsize,
}

impl TaskHandle {
    pub fn new(key: TasKey) -> Self {
        Self {
            key,
            state: Default::default(),
        }
    }

    /// Update task to `Ready` state.
    pub fn to_ready(&self) -> Result<(), TaskState> {
        let mut current = TaskState::Pending;

        while current == TaskState::Pending || current == TaskState::Active {
            match self.state.compare_exchange_weak(
                current as usize,
                TaskState::Ready as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(state) => current = unsafe { transmute(state) },
            }
        }

        Err(current)
    }

    /// Update task to `Active` state.
    pub fn to_active(&self) -> Result<(), TaskState> {
        self.state
            .compare_exchange(
                TaskState::Ready as usize,
                TaskState::Active as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .map(|_| ())
            .map_err(|v| unsafe { ::std::mem::transmute(v) })
    }

    /// Update task to `Active` state.
    pub fn to_pending(&self) -> Result<(), TaskState> {
        self.state
            .compare_exchange(
                TaskState::Active as usize,
                TaskState::Pending as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .map(|_| ())
            .map_err(|v| unsafe { ::std::mem::transmute(v) })
    }

    /// Update task to `Cancelling` state.
    pub fn to_cancel(&self) -> Result<(), TaskState> {
        let mut current = TaskState::Ready;

        while current != TaskState::Cancelled && current != TaskState::Completed {
            match self.state.compare_exchange_weak(
                current as usize,
                TaskState::Cancelling as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(state) => current = unsafe { transmute(state) },
            }
        }

        Err(current)
    }

    /// Update task to `Cancelling` state.
    pub fn to_finish(&self) -> Result<(), TaskState> {
        let mut current = TaskState::Active;

        loop {
            let finish_state = match current {
                TaskState::Active => TaskState::Completed,
                TaskState::Cancelling => TaskState::Cancelled,
                _ => {
                    break;
                }
            };

            match self.state.compare_exchange_weak(
                current as usize,
                finish_state as usize,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(state) => current = unsafe { transmute(state) },
            }
        }

        Err(current)
    }
}

pub struct ThreadPool;
