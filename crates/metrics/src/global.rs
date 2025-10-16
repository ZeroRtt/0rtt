//! Funcs to handle global `Registry` instance.

use std::sync::OnceLock;

use crate::Registry;

static GLOBAL_REGISTRY: OnceLock<Box<dyn Registry>> = OnceLock::new();

/// Set the **global** measuring instruments registry.
///
/// *You should call this function before calling any measuring funs.*
pub fn set_global_registry<R: Registry + 'static>(registry: R) -> Result<(), Box<dyn Registry>> {
    GLOBAL_REGISTRY.set(Box::new(registry))
}

/// Returns a reference to the `Registry`.
///
/// If a `Registry` has not been set, a no-op implementation is returned.
pub fn get_global_registry() -> Option<&'static dyn Registry> {
    GLOBAL_REGISTRY.get().map(|v| v.as_ref())
}
