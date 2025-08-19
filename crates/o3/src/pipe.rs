use std::io::Result;

use mio::net::TcpStream;
use quico::Group;
use ringbuff::RingBuf;

#[allow(unused)]
pub struct TcpQuicPipe {
    tcp_stream: TcpStream,
    quic_conn_token: quico::Token,
    quic_stream_id: u64,
    tcp_stream_buff: RingBuf,
    quic_stream_buff: RingBuf,
}

#[allow(unused)]
impl TcpQuicPipe {
    pub fn new(
        tcp_stream: TcpStream,
        quic_conn_token: quico::Token,
        quic_stream_id: u64,
        buffsize: usize,
    ) -> Self {
        Self {
            tcp_stream,
            quic_conn_token,
            quic_stream_id,
            tcp_stream_buff: RingBuf::with_capacity(buffsize),
            quic_stream_buff: RingBuf::with_capacity(buffsize),
        }
    }

    /// forward data from tcp stream to quic stream.
    pub fn copy_from_tcp_stream(&mut self, group: &Group) -> Result<usize> {
        todo!()
    }

    /// forward data from quic stream to tcp stream.
    pub fn copy_from_quic_stream(&mut self, group: &Group) -> Result<usize> {
        todo!()
    }
}
