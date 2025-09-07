//! `redirect` is a fast `quic/http3` reverse proxy implementation.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "cli")]
#[cfg_attr(docsrs, doc(cfg(feature = "cli")))]
pub mod cli;

pub mod buf;
pub mod connector;
pub mod errors;
pub mod mapping;
pub mod poll;
pub mod port;
pub mod token;
pub mod udp;
