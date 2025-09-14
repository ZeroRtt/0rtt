//! Poll for readiness events on masive quiche connecitons.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod conn;
mod registration;
mod utils;

mod errors;
pub use errors::*;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod validation;
#[cfg(feature = "server")]
pub use validation::*;

mod readiness;
pub use readiness::*;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod acceptor;
#[cfg(feature = "server")]
pub use acceptor::*;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
mod client;
#[cfg(feature = "client")]
pub use client::*;

mod group;
pub use group::*;

pub use quiche;
