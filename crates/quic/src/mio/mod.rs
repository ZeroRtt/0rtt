//! Integration with the mio library.

mod group;
pub use group::*;

mod buf;
mod udp;
mod would_block;
