//! Built-in executors and related tools.

#[cfg(feature = "thread-pool")]
#[cfg_attr(docsrs, doc(cfg(feature = "thread-pool")))]
mod thread_pool;

#[cfg(feature = "thread-pool")]
pub use thread_pool::*;

#[allow(unused)]
mod tree;
