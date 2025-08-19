//! A ringbuff bridge between `std::io` and `futures::io`.

#![cfg_attr(docsrs, feature(doc_cfg))]

mod ringbuf;
pub use ringbuf::*;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod pipe;
