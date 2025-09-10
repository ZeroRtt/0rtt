use std::sync::Arc;

use quico::Group;

use crate::{errors::Error, port::Port, token::Token};

/// Port for `QuicStream`
pub struct QuicStreamPort {
    trace_id: String,
    conn_id: quico::Token,
    stream_id: u64,
    group: Arc<Group>,
    sent: u64,
}

impl QuicStreamPort {
    /// Create a new port for quic stream.
    pub fn new(group: Arc<Group>, conn_id: quico::Token, stream_id: u64) -> Self {
        Self {
            trace_id: format!("QUIC({},{})", conn_id.0, stream_id),
            conn_id,
            stream_id,
            group,
            sent: 0,
        }
    }
}

impl Drop for QuicStreamPort {
    fn drop(&mut self) {
        _ = self.group.stream_close(self.conn_id, self.stream_id);
    }
}

impl Port for QuicStreamPort {
    fn trace_id(&self) -> &str {
        &self.trace_id
    }

    fn sent(&self) -> u64 {
        self.sent
    }

    fn token(&self) -> crate::token::Token {
        Token::QuicStream(self.conn_id.0, self.stream_id)
    }

    fn write(&mut self, buf: &[u8]) -> crate::errors::Result<usize> {
        let write_size = self
            .group
            .stream_send(self.conn_id, self.stream_id, buf, false)?;

        self.sent += write_size as u64;

        Ok(write_size)
    }

    fn read(&mut self, buf: &mut [u8]) -> crate::errors::Result<usize> {
        let (read_size, fin) = self.group.stream_recv(self.conn_id, self.stream_id, buf)?;

        if fin {
            Err(Error::Fin(read_size, self.token()))
        } else {
            Ok(read_size)
        }
    }

    fn fin(&mut self) -> crate::errors::Result<()> {
        self.group
            .stream_send(self.conn_id, self.stream_id, b"", true)?;

        Ok(())
    }
}
