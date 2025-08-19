//! `redirect` is a fast `quic/http3` reverse proxy implementation.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod pipe;

#[cfg(feature = "redirect")]
#[cfg_attr(docsrs, doc(cfg(feature = "redirect")))]
pub mod redirect;

#[cfg(feature = "agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "agent")))]
pub mod agent;

#[cfg(feature = "cli")]
#[cfg_attr(docsrs, doc(cfg(feature = "cli")))]
pub mod cli;
