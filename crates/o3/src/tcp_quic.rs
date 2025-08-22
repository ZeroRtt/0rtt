use std::io::Result;

use mio::net::TcpStream;
use quico::Group;
use ringbuff::RingBuf;

use crate::pipeline::Pipeline;

/// Pipeline between tcp and quic.
#[allow(unused)]
pub struct TcpQuicPipeline {
    mio_token: mio::Token,
    tcp_stream: TcpStream,
    quic_stream: (quico::Token, u64),
    forward_buff: RingBuf,
    backward_buff: RingBuf,
}

#[allow(unused)]
impl Pipeline for TcpQuicPipeline {
    type From = mio::Token;
    type To = (quico::Token, u64);

    fn from_key(&self) -> &Self::From {
        &self.mio_token
    }

    fn to_key(&self) -> &Self::To {
        &self.quic_stream
    }
}

#[allow(unused)]
impl TcpQuicPipeline {
    /// Create a new pipeline with caching buffer size.
    pub fn with_capacity(
        mio_token: mio::Token,
        tcp_stream: TcpStream,
        quic_conn_token: quico::Token,
        quic_stream_id: u64,
        capacity: usize,
    ) -> Self {
        Self {
            mio_token,
            tcp_stream,
            quic_stream: (quic_conn_token, quic_stream_id),
            forward_buff: RingBuf::with_capacity(capacity),
            backward_buff: RingBuf::with_capacity(capacity),
        }
    }

    pub fn forward(&mut self) -> Result<usize> {
        todo!()
    }

    pub fn backward(&mut self, group: &Group) -> Result<usize> {
        todo!()
    }
}
