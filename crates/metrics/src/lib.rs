//! A lightweight metrics facade for `Rust`.

#![cfg_attr(docsrs, feature(doc_cfg))]

mod registry;
pub use registry::*;

#[cfg(feature = "global")]
#[cfg_attr(docsrs, doc(cfg(feature = "global")))]
pub mod global;

#[cfg(feature = "memory")]
#[cfg_attr(docsrs, doc(cfg(feature = "memory")))]
pub mod memory;

#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use metricrs_derive::*;
