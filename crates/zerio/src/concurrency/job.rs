use std::{fmt::Display, ops, usize};

/// A handle reference to one asynchronous task.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct JobKey(pub usize, pub Option<&'static str>);

impl Display for JobKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1
            .map(|v| write!(f, "task({})", v))
            .unwrap_or_else(|| write!(f, "task({})", self.0))
    }
}

impl From<usize> for JobKey {
    fn from(value: usize) -> Self {
        Self(value, None)
    }
}

impl From<(usize, Option<&'static str>)> for JobKey {
    fn from(value: (usize, Option<&'static str>)) -> Self {
        Self(value.0, value.1)
    }
}

/// The state type of one task.
#[repr(usize)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum JobState {
    /// Ready to execute
    Ready = 1,
    /// Is executing.
    Active = 2,
    /// The task is waiting readiness events.
    Pending = 4,
    /// The task has been successfully completed.
    Completed = 8,
    /// The task is currently being cancelled.
    Cancelled = 16,
}

impl ops::BitOr for JobState {
    type Output = usize;

    fn bitor(self, rhs: Self) -> Self::Output {
        self as usize | rhs as usize
    }
}

impl ops::BitOr<JobState> for usize {
    type Output = usize;

    fn bitor(self, rhs: JobState) -> Self::Output {
        self | rhs as usize
    }
}

impl From<usize> for JobState {
    fn from(value: usize) -> Self {
        match value {
            1 => JobState::Ready,
            2 => JobState::Active,
            4 => JobState::Pending,
            8 => JobState::Completed,
            16 => JobState::Cancelled,
            _ => unreachable!("Invalid job state: {}", value),
        }
    }
}
