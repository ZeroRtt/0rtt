//! A zerortt `QuicBind` implementation based on `mio` library.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod group;
pub use group::*;

mod buf;
mod udp;

#[cfg(feature = "futures")]
#[cfg_attr(docsrs, doc(cfg(feature = "futures")))]
pub mod futures;
