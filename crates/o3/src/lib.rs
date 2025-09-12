//! `redirect` is a fast `quic/http3` reverse proxy implementation.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "cli")]
#[cfg_attr(docsrs, doc(cfg(feature = "cli")))]
pub mod cli;

#[cfg(feature = "agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "agent")))]
pub mod agent;

#[cfg(feature = "o3")]
#[cfg_attr(docsrs, doc(cfg(feature = "o3")))]
pub mod redirect;

pub mod connector;
pub mod errors;
pub mod udp;

mod buf;
mod mapping;
mod mertics;
mod port;
mod registry;
mod token;
mod would_block;
