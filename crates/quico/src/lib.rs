//! Poll for readiness events on masive quiche connecitons.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod utils;

mod errors;
pub use errors::*;

mod conn;
mod readiness;

mod events;
pub use events::*;

mod group;
pub use group::*;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod address_validation;
#[cfg(feature = "server")]
pub use address_validation::*;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod acceptor;
#[cfg(feature = "server")]
pub use acceptor::*;
