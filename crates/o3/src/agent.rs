use std::{io::Result, net::SocketAddr};

use mio::{
    Interest,
    net::{TcpListener, UdpSocket},
};

use crate::{pipeline::Pipelines, tcp_quic::TcpQuicPipeline};

static LISTENER_TOKEN: mio::Token = mio::Token(0);
static UDP_TOKEN: mio::Token = mio::Token(1);

/// Forward agent, passes tcp traffic through quic tunnel.
#[allow(unused)]
pub struct Agent {
    /// `mio::Token` generator for tcp streams.
    token_next: usize,
    /// Incoming tcp stream listener.
    listener: TcpListener,
    /// udp socket for quic connections.
    udp_socket: UdpSocket,
    /// remote o3 server addresses.
    remote_addrs: Vec<SocketAddr>,
    /// aliving pipelines.
    pipelines: Pipelines<TcpQuicPipeline>,
}

#[allow(unused)]
impl Agent {
    fn next_tcp_stream_token(&mut self) -> mio::Token {
        let token = mio::Token(self.token_next);

        let overflow;
        (self.token_next, overflow) = self.token_next.overflowing_add(1);

        // `0` and `1` are reserved for `Listener` and `Quic socket`
        if overflow {
            self.token_next = 2;
        }

        token
    }
}

impl Agent {
    /// Create a new agent instance.
    pub fn new(laddr: SocketAddr, raddrs: Vec<SocketAddr>) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut listener = TcpListener::bind(laddr)?;

        poll.registry()
            .register(&mut listener, LISTENER_TOKEN, Interest::READABLE)?;

        let mut udp_socket = UdpSocket::bind("[::]:0".parse().unwrap())?;

        poll.registry().register(
            &mut udp_socket,
            UDP_TOKEN,
            Interest::READABLE.add(Interest::WRITABLE),
        )?;

        Ok(Self {
            token_next: 2,
            listener,
            udp_socket,
            remote_addrs: raddrs,
            pipelines: Default::default(),
        })
    }
}
