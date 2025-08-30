use std::io::{Read, Write};

use mio::net::TcpStream;

use crate::port::Port;

/// Port for `mio::TcpStream`
pub struct TcpStreamPort {
    trace_id: String,
    stream: TcpStream,
}

impl TcpStreamPort {
    /// Create a new tcp stream port.
    pub fn new(stream: TcpStream, token: mio::Token) -> Self {
        Self {
            trace_id: format!("TCP({:?})", token),
            stream,
        }
    }
}

impl Port for TcpStreamPort {
    fn trace_id(&self) -> &str {
        &self.trace_id
    }

    fn write(&mut self, buf: &[u8]) -> crate::errors::Result<usize> {
        let write_size = self.stream.write(buf)?;

        Ok(write_size)
    }

    fn read(&mut self, buf: &mut [u8]) -> crate::errors::Result<usize> {
        let read_size = self.stream.read(buf)?;

        Ok(read_size)
    }

    fn close(&mut self) -> crate::errors::Result<()> {
        self.stream.shutdown(std::net::Shutdown::Both)?;
        Ok(())
    }
}
