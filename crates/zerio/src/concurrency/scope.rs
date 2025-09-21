use std::fmt::Display;

use crate::concurrency::Dispatcher;

/// A handle reference to one [`Scope`] instance.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct ScopeKey(pub usize, pub Option<&'static str>);

impl Display for ScopeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1
            .map(|v| write!(f, "scope({})", v))
            .unwrap_or_else(|| write!(f, "scope({})", self.0))
    }
}

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

    /// Returns `ScopeKey` of this scope.
    #[inline]
    pub fn key(&self) -> ScopeKey {
        self.key
    }

    /// Create a `Future` to await the completion of all sub-tasks.
    #[inline]
    pub async fn wait_all(&self) {
        _ = self.dispatcher.scope_wait(self.key).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use crate::concurrency::{Dispatcher, Scope};

    struct Mock;

    #[allow(unused)]
    impl Dispatcher for Mock {
        fn scope_current(&self) -> super::ScopeKey {
            todo!()
        }

        fn scope_enter(
            &self,
            parent: super::ScopeKey,
            label: Option<&'static str>,
        ) -> super::ScopeKey {
            todo!()
        }

        fn scope_cancel(&self, key: super::ScopeKey) {
            todo!()
        }

        fn scope_wait(&self, key: super::ScopeKey) -> futures_channel::oneshot::Receiver<()> {
            todo!()
        }

        fn scope_leave(&self, key: super::ScopeKey) {
            todo!()
        }

        fn scope_spawn(
            &self,
            key: super::ScopeKey,
            task: futures_task::FutureObj<'static, ()>,
        ) -> Result<(), futures_task::SpawnError> {
            todo!()
        }
    }

    #[test]
    #[ignore = "compile-time test"]
    fn test() {
        static GLOBAL: OnceLock<Box<dyn Dispatcher + Send + Sync>> = OnceLock::new();

        let dispatcher: &'static dyn Dispatcher = GLOBAL.get_or_init(|| Box::new(Mock)).as_ref();

        let _: Scope<&'static dyn Dispatcher> = Scope::new(Some("hello"), dispatcher);
    }
}
