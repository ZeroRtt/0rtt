//! A reference to port object.

/// a reference to port object.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum Token {
    Mio(usize),
    Quic(u32),
    QuicStream(u32, u64),
}
