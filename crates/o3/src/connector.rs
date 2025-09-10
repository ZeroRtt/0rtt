//! Utilities for `QUIC`/`TCP` client.

use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, task::Poll};

use mio::{Interest, net::TcpStream};

use rand::seq::SliceRandom;

use crate::errors::Result;

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

pub type QuicConnector = quico::client::Connector;
