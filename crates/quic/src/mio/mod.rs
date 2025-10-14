//! Integration with the mio library.

mod group;
pub use group::*;

mod buf;
mod udp;

pub(crate) mod would_block;
