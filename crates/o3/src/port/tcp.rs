use std::io::{ErrorKind, Read, Write};

use mio::net::TcpStream;

use crate::{port::Port, token::Token};

/// Port for `mio::TcpStream`
pub struct TcpStreamPort {
    token: Token,
    trace_id: String,
    stream: TcpStream,
}

impl TcpStreamPort {
    /// Create a new tcp stream port.
    pub fn new(stream: TcpStream, token: mio::Token) -> Self {
        Self {
            trace_id: format!("TCP({:?})", token),
            token: Token::Mio(token.0),
            stream,
        }
    }
}

impl Port for TcpStreamPort {
    fn trace_id(&self) -> &str {
        &self.trace_id
    }

    fn token(&self) -> Token {
        self.token
    }

    fn write(&mut self, buf: &[u8]) -> crate::errors::Result<usize> {
        let write_size = self.stream.write(buf).inspect_err(|err| {
            if err.kind() != ErrorKind::WouldBlock {
                log::error!("{} write data, err={}", self.trace_id(), err)
            }
        })?;

        Ok(write_size)
    }

    fn read(&mut self, buf: &mut [u8]) -> crate::errors::Result<usize> {
        let read_size = self.stream.read(buf).inspect_err(|err| {
            if err.kind() != ErrorKind::WouldBlock {
                log::error!("{} read data, err={}", self.trace_id(), err)
            }
        })?;

        Ok(read_size)
    }

    fn close(&mut self) -> crate::errors::Result<()> {
        self.stream
            .shutdown(std::net::Shutdown::Both)
            .inspect_err(|err| {
                if err.kind() != ErrorKind::WouldBlock {
                    log::trace!("{} shutdown, err={}", self.trace_id(), err)
                }
            })?;
        Ok(())
    }
}
