use crate::{Result, Token};

/// Readiness event kind for transports.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransportEventKind {
    Send,
    Recv,
    StreamOpen,
    StreamAccept,
    StreamSend,
    StreamRecv,
    StreamClosed,
}

/// Transport for stream protocols.
pub trait Transport {
    /// Returns id of this `transport`.
    fn id(&self) -> &'static str;

    /// Returns the max length of send buffer.
    fn send_buffer_size_hint(&self) -> usize;

    /// Receive data from underlying transport or device.
    fn recv(&self, buf: &mut [u8]) -> Result<usize>;

    /// Send data to underlying transport or device.
    fn send(&self, buf: &mut [u8]) -> Result<usize>;

    /// Create a new client-side connection.
    fn stream_open(&self, raddr: &str) -> Result<Token>;

    /// Send data over this stream.
    fn stream_send(&self, token: Token, buf: &mut [u8]) -> Result<usize>;

    /// Receive data via this stream.
    fn stream_recv(&self, token: Token, buf: &mut [u8]) -> Result<usize>;
}
