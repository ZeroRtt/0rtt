//! An experimental structured concurrency runtime for Rust.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod concurrency;

#[cfg(feature = "executor")]
#[cfg_attr(docsrs, doc(cfg(feature = "executor")))]
pub mod executor;
