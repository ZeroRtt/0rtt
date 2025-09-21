use std::fmt::Display;

use futures_channel::oneshot;
use futures_task::{FutureObj, SpawnError};

/// A handle to one scope instance.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct ScopeKey(pub usize, pub Option<&'static str>);

impl Display for ScopeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1
            .map(|v| write!(f, "scope({})", v))
            .unwrap_or_else(|| write!(f, "scope({})", self.0))
    }
}

/// A trait to access scope functions.
pub trait Dispatcher {
    /// Returns the concurrent scope to which the current context belongs
    fn scope_current(&self) -> ScopeKey;
    /// Create a new `concurrent` scope with optional label name.
    fn scope_enter(&self, parent: ScopeKey, label: Option<&'static str>) -> ScopeKey;
    /// Cancel all sub-tasks.
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

/// Dispatcher for io tasks.
pub struct ZerIODispatcher;

#[allow(unused)]
impl Dispatcher for ZerIODispatcher {
    fn scope_current(&self) -> ScopeKey {
        ScopeKey(0, Some("MainScope"))
    }

    fn scope_enter(&self, parent: ScopeKey, label: Option<&'static str>) -> ScopeKey {
        ScopeKey(parent.0 + 1, label)
    }

    fn scope_cancel(&self, key: ScopeKey) {
        todo!()
    }

    fn scope_wait(&self, key: ScopeKey) -> oneshot::Receiver<()> {
        todo!()
    }

    fn scope_leave(&self, key: ScopeKey) {
        todo!()
    }

    fn scope_spawn(&self, key: ScopeKey, task: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        todo!()
    }
}

#[cfg(feature = "global")]
#[cfg_attr(docsrs, doc(cfg(feature = "global")))]
mod global {
    use std::sync::OnceLock;

    use crate::concurrency::{Dispatcher, ZerIODispatcher};

    static IO_DISPATCHER: OnceLock<Box<dyn Dispatcher + Sync + Send>> = OnceLock::new();

    /// Get global `io` dispatcher instance.
    pub fn io_dispatcher() -> &'static dyn Dispatcher {
        IO_DISPATCHER
            .get_or_init(|| Box::new(ZerIODispatcher))
            .as_ref()
    }

    /// Set global `io` dispatcher to a `dispatcher: D`.
    ///
    /// This function may only be called once in the lifetime of a program.
    /// calling after the call to `io_dispatcher` will panic.
    pub fn set_io_dispatcher<D>(dispatcher: D)
    where
        D: Dispatcher + Sync + Send + 'static,
    {
        if IO_DISPATCHER.set(Box::new(dispatcher)).is_err() {
            panic!("The `IO_DISPATCHER` already exists.")
        }
    }
}

#[cfg(feature = "global")]
pub use global::*;
