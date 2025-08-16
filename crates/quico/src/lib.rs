//! Poll for readiness events on masive quiche connecitons.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[allow(unused)]
mod utils;

mod errors;
pub use errors::*;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod validation;

mod readiness;
pub use readiness::*;

#[allow(unused)]
mod conn;
