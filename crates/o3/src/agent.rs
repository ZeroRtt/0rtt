use std::{
    collections::{HashMap, HashSet},
    io::Result,
    net::SocketAddr,
};

use mio::{Interest, Token, net::TcpListener};
use quico::{Group, quiche};

use crate::pipe::TcpQuicPipe;

/// O3 forward agent, passes tcp traffic through quic tunnel.
#[allow(unused)]
pub struct Agent {
    /// remote quic server addresses.
    raddrs: Vec<SocketAddr>,
    /// shared quiche configuration for client connection.
    config: quiche::Config,
    /// mio token generator.
    mio_token_next: usize,
    /// Network poll
    poll: mio::Poll,
    /// non-blocking tcp server-side socket.
    listener: TcpListener,
    /// Map tcp stream to pipe.
    tcp_streams: HashMap<mio::Token, u64>,
    /// quico I/O group.
    group: quico::Group,
    /// quic connection pool.
    quic_conns: HashSet<quico::Token>,
    /// Map quic stream to pipe.
    quic_streams: HashMap<(quico::Token, u64), u64>,
    /// Alive pipes.
    pipes: HashMap<u64, TcpQuicPipe>,
}

impl Agent {
    /// Create a new `Agent` instance.
    pub fn new(laddr: SocketAddr, raddrs: Vec<SocketAddr>, config: quiche::Config) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut listener = TcpListener::bind(laddr)?;

        poll.registry()
            .register(&mut listener, Token(0), Interest::READABLE)?;

        let group = Group::new();

        Ok(Self {
            config,
            raddrs,
            mio_token_next: 1,
            poll,
            listener,
            tcp_streams: Default::default(),
            group,
            quic_conns: Default::default(),
            quic_streams: Default::default(),
            pipes: Default::default(),
        })
    }
}
