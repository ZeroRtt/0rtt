//! Concurrent I/O beyond rust async runtime based on reactor pattern.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod device;
pub use device::*;

mod stack;
#[allow(unused)]
pub use stack::*;
