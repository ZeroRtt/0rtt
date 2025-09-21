use crate::concurrency::{Dispatcher, ScopeKey};

/// A concurrency scope instance.
pub struct Scope<D>
where
    D: Dispatcher,
{
    key: ScopeKey,
    dispatcher: D,
}

impl<D> Drop for Scope<D>
where
    D: Dispatcher,
{
    fn drop(&mut self) {
        self.dispatcher.scope_leave(self.key);
    }
}

impl<D> Scope<D>
where
    D: Dispatcher,
{
    /// Create a new concurrency `scope` instance with specific `parent`.
    pub fn with_context(label: Option<&'static str>, parent: ScopeKey, dispatcher: D) -> Self {
        let key = dispatcher.scope_enter(parent, label);

        Self { key, dispatcher }
    }

    /// Create a new concurrency `scope`.
    #[inline]
    pub fn new(label: Option<&'static str>, dispatcher: D) -> Self {
        Self::with_context(label, dispatcher.scope_current(), dispatcher)
    }

    /// Cancel all sub-tasks.
    #[inline]
    pub fn cancel(&self) {
        self.dispatcher.scope_cancel(self.key);
    }

    /// Create a `Future` to await the completion of all sub-tasks.
    #[inline]
    pub async fn wait_all(&self) {
        _ = self.dispatcher.scope_wait(self.key).await;
    }
}
