//! Tiny and fixed length buffer for protocol parsing.

use fixedbuf::ArrayBuf;

/// Fixed-length buffer for quic protocol.
pub type QuicBuf = ArrayBuf<1330>;
