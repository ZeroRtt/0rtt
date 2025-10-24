//! Integration with the mio library.

mod group;
pub use group::*;

mod buf;
mod udp;

pub(crate) mod would_block;

#[cfg(feature = "futures")]
#[cfg_attr(docsrs, doc(cfg(feature = "futures")))]
pub mod futures;
