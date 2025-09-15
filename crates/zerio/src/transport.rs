use std::any::Any;

use crate::{Buffer, Result, Token};

/// Transport for stream protocols.
pub trait Transport {
    /// Returns id of this `transport`.
    fn id(&self) -> &'static str;

    /// Returns the max length of send buffer.
    fn send_buffer_size_hint(&self) -> usize;

    /// Receive data from underlying transport or device.
    fn transport_recv(&self, buf: &mut Buffer) -> Result<()>;

    /// Send data to underlying transport or device.
    fn transport_send(&self, buf: &mut Buffer) -> Result<()>;

    /// Create a new client-side connection.
    fn connect(&self, parms: Box<dyn Any>) -> Result<Token>;

    /// Create a new client-side connection.
    fn bind(&self, parms: Box<dyn Any>) -> Result<Token>;

    /// Send data over this stream.
    fn write(&self, token: Token, buf: &[u8]) -> Result<usize>;

    /// Receive data via this stream.
    fn read(&self, token: Token, buf: &mut [u8]) -> Result<usize>;
}
