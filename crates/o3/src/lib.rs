//! `redirect` is a fast `quic/http3` reverse proxy implementation.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod buf;
pub mod errors;
pub mod port;
pub mod router;
pub mod token;

#[cfg(feature = "cli")]
#[cfg_attr(docsrs, doc(cfg(feature = "cli")))]
pub mod cli;
