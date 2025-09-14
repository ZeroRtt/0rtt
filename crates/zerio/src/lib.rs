//! Concurrent I/O beyond rust async runtime based on reactor pattern.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod device;
pub use device::*;

mod transport;
pub use transport::*;

mod token;
pub use token::*;

mod errors;
pub use errors::*;
