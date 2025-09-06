//! `redirect` is a fast `quic/http3` reverse proxy implementation.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "cli")]
#[cfg_attr(docsrs, doc(cfg(feature = "cli")))]
pub mod cli;

pub mod buf;
pub mod errors;
pub mod poll;
pub mod udp;
