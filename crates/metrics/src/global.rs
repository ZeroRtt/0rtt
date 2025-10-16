//! Funcs to handle global `Registry` instance.

use std::sync::OnceLock;

use crate::Registry;

static GLOBAL_REGISTRY: OnceLock<Box<dyn Registry>> = OnceLock::new();

struct NoopRegistry;

#[allow(unused)]
impl Registry for NoopRegistry {
    fn counter(&self, key: &str, tags: &[(&str, &str)]) -> crate::Counter {
        crate::Counter::Noop
    }

    fn gauge(&self, key: &str, tags: &[(&str, &str)]) -> crate::Gauge {
        crate::Gauge::Noop
    }

    fn histogam(&self, key: &str, tags: &[(&str, &str)]) -> crate::Histogram {
        crate::Histogram::Noop
    }
}

/// Set the **global** measuring instruments registry.
///
/// *You should call this function before calling any measuring funs.*
pub fn set_global_registry<R: Registry + 'static>(registry: R) -> Result<(), Box<dyn Registry>> {
    GLOBAL_REGISTRY.set(Box::new(registry))
}

/// Returns a reference to the `Registry`.
///
/// If a `Registry` has not been set, a no-op implementation is returned.
pub fn get_global_registry() -> &'static dyn Registry {
    GLOBAL_REGISTRY
        .get_or_init(|| Box::new(NoopRegistry))
        .as_ref()
}
