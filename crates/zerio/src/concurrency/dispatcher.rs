use futures_channel::oneshot;
use futures_task::{FutureObj, SpawnError};

use crate::concurrency::ScopeKey;

/// A trait to access scope functions.
pub trait Dispatcher {
    /// Returns the concurrent scope to which the current context belongs
    fn scope_current(&self) -> ScopeKey;
    /// Create a new `concurrent` scope with optional label name.
    fn scope_enter(&self, parent: ScopeKey, label: Option<&'static str>) -> ScopeKey;
    /// Cancel all existing/future sub-tasks.
    fn scope_cancel(&self, key: ScopeKey);
    /// Create a `Future` to await the completion of all sub-tasks.
    fn scope_wait(&self, key: ScopeKey) -> oneshot::Receiver<()>;
    /// Destroy scope and drop all existing children tasks.
    fn scope_leave(&self, key: ScopeKey);
    /// spawn a sub-task for a socpe.
    fn scope_spawn(&self, key: ScopeKey, task: FutureObj<'static, ()>) -> Result<(), SpawnError>;
}

impl Dispatcher for &'static dyn Dispatcher {
    #[inline]
    fn scope_current(&self) -> ScopeKey {
        Dispatcher::scope_current(*self)
    }

    #[inline]
    fn scope_enter(&self, parent: ScopeKey, label: Option<&'static str>) -> ScopeKey {
        Dispatcher::scope_enter(*self, parent, label)
    }

    #[inline]
    fn scope_cancel(&self, key: ScopeKey) {
        Dispatcher::scope_cancel(*self, key);
    }

    #[inline]
    fn scope_wait(&self, key: ScopeKey) -> oneshot::Receiver<()> {
        Dispatcher::scope_wait(*self, key)
    }

    #[inline]
    fn scope_leave(&self, key: ScopeKey) {
        Dispatcher::scope_leave(*self, key)
    }

    #[inline]
    fn scope_spawn(&self, key: ScopeKey, task: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        Dispatcher::scope_spawn(*self, key, task)
    }
}
