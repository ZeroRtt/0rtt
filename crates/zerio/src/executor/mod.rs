//! Built-in executors and related tools.

#[cfg(all(feature = "thread-pool", not(target_arch = "wasm32")))]
#[cfg_attr(docsrs, doc(cfg(feature = "thread-pool")))]
mod thread_pool;

#[cfg(all(feature = "thread-pool", not(target_arch = "wasm32")))]
pub use thread_pool::*;

#[allow(unused)]
mod tree;
