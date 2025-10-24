//! Core `poll` API implementation for zerortt `QUIC`.
#![cfg_attr(docsrs, feature(doc_cfg))]
mod conn;
mod readiness;
mod utils;

mod group;
pub use group::*;
