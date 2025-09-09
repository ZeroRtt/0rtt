//! Utilities for `QUIC`/`TCP` client.

use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::SocketAddr,
    task::Poll,
};

use mio::{Interest, net::TcpStream};
use quico::{Group, quiche};
use rand::seq::SliceRandom;

use crate::{errors::Result, poll::WouldBlock};

/// Utility for client-side `TCP` connecting.
pub struct TcpConnector {
    /// connect to the specified addresses.
    targets: Vec<SocketAddr>,
    /// sockets connection cannot be completed immediately
    handshakings: HashMap<mio::Token, (TcpStream, SocketAddr)>,
}

impl TcpConnector {
    /// Create a new `TcpConnector` with target address list.
    pub fn new(targets: Vec<SocketAddr>) -> Self {
        Self {
            targets,
            handshakings: Default::default(),
        }
    }

    /// Create a new TCP stream and issue a non-blocking connect to one of targets address.
    pub fn connect(&mut self, registry: &mio::Registry, token: mio::Token) -> Result<()> {
        self.targets.shuffle(&mut rand::rng());

        let mut stream = TcpStream::connect(self.targets[0])?;

        registry.register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;

        self.handshakings.insert(token, (stream, self.targets[0]));

        Ok(())
    }

    /// Check if the `TCP` stream referenced by `token` is unconnected.
    #[inline]
    pub fn is_connecting(&self, token: &mio::Token) -> bool {
        self.handshakings.contains_key(token)
    }

    /// Check if the stream is connected.
    pub fn is_ready(&mut self, token: &mio::Token) -> Poll<Result<(TcpStream, SocketAddr)>> {
        let Some((tcp_stream, _)) = self.handshakings.get(&token) else {
            return Poll::Pending;
        };

        match tcp_stream.take_error() {
            Ok(None) => {}
            Ok(Some(err)) | Err(err) => {
                self.handshakings.remove(token).expect("TcpStream");

                log::error!("TcpStream is failed to connect to server, {}", err);

                return Poll::Ready(Err(err.into()));
            }
        }

        match tcp_stream.peer_addr() {
            Ok(_) => {
                let (tcp_stream, addr) = self.handshakings.remove(&token).expect("TcpStream");

                Poll::Ready(Ok((tcp_stream, addr)))
            }
            Err(err)
                if err.kind() == ErrorKind::NotConnected
                    || err.raw_os_error() == Some(libc::EINPROGRESS) =>
            {
                Poll::Pending
            }
            Err(err) => {
                let (_, addr) = self.handshakings.remove(&token).expect("TcpStream");

                log::error!("Tcp connect to {}, err={}", addr, err);

                Poll::Ready(Err(err.into()))
            }
        }
    }
}

/// Utility for client-side `QUIC` connection.
pub struct QuicConnector {
    /// Quic local bound address.
    local_addr: SocketAddr,
    /// connect to the specified addresses.
    targets: Vec<SocketAddr>,
    /// Max size of `QUIC` conection pool.
    max_pool_size: usize,
    /// active `QUIC` connections.
    conns: HashSet<quico::Token>,
    /// handshaking connections.
    handshaking_conns: HashSet<quico::Token>,
    /// shared `QUIC` configuration.
    config: quiche::Config,
}

impl QuicConnector {
    /// Create a new `QuicConnector` with target address list.
    pub fn new(
        local_addr: SocketAddr,
        targets: Vec<SocketAddr>,
        config: quiche::Config,
        max_pool_size: usize,
    ) -> Self {
        assert!(max_pool_size > 0, "Invalid `max_pool_size` value.");
        Self {
            max_pool_size,
            local_addr,
            targets,
            conns: Default::default(),
            handshaking_conns: Default::default(),
            config,
        }
    }

    /// Returns local bound socket address.
    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Register new established connection.
    pub fn connected(&mut self, token: quico::Token) {
        log::info!("put `QUIC` connection {:?}.", token);

        assert!(self.handshaking_conns.remove(&token));
        assert!(self.conns.insert(token));
    }

    /// Deregister close connection.
    pub fn closed(&mut self, token: quico::Token) {
        log::info!("remove `QUIC` connection {:?}.", token);

        if !self.handshaking_conns.remove(&token) {
            self.conns.remove(&token);
        }
    }

    /// Open a new outbound stream to target server.
    pub fn stream_open(&mut self, group: &Group) -> Poll<Result<(quico::Token, u64)>> {
        let mut conns = self.conns.iter().cloned().collect::<Vec<_>>();

        conns.shuffle(&mut rand::rng());

        for conn_id in conns {
            match group.stream_open(conn_id).would_block() {
                Poll::Ready(Ok(stream_id)) => return Poll::Ready(Ok((conn_id, stream_id))),
                Poll::Ready(Err(_)) => {
                    log::trace!("Remove quic connection {:?}", conn_id);
                    self.conns.remove(&conn_id);
                }
                Poll::Pending => {}
            }
        }

        if self.conns.len() + self.handshaking_conns.len() < self.max_pool_size {
            // Create new `QUIC` connection.

            self.targets.shuffle(&mut rand::rng());

            let token = group.connect(None, self.local_addr, self.targets[0], &mut self.config)?;

            log::info!("`QUIC` connect to {}, token={:?}", self.targets[0], token);

            self.handshaking_conns.insert(token);
        }

        Poll::Pending
    }
}
