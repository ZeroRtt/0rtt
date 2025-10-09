//! Poll api for masive [`quiche::Connection`].
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod poll;
pub use quiche;

#[cfg(feature = "mio")]
#[cfg_attr(docsrs, doc(cfg(feature = "mio")))]
pub mod mio;

#[cfg(feature = "futures")]
#[cfg_attr(docsrs, doc(cfg(feature = "futures")))]
pub mod futures;
