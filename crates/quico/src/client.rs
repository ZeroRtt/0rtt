use std::{collections::HashSet, net::SocketAddr};

use quiche::Config;
use rand::seq::SliceRandom;

use crate::{Error, Group, Result, Token, utils::random_conn_id};

impl Group {
    /// Creates a new client-side connection.
    pub fn connect(
        &self,
        server_name: Option<&str>,
        local: SocketAddr,
        peer: SocketAddr,
        config: &mut Config,
    ) -> Result<Token> {
        let conn = quiche::connect(server_name, &random_conn_id(), local, peer, config)?;

        let token = self.register(conn)?;

        Ok(token)
    }
}

/// Client `QUIC` connection pool.
pub struct Connector {
    /// Quic local bound address.
    local_addr: SocketAddr,
    /// connect to the specified addresses.
    targets: Vec<SocketAddr>,
    /// Max size of `QUIC` conection pool.
    max_pool_size: usize,
    /// active `QUIC` connections.
    conns: HashSet<Token>,
    /// handshaking connections.
    handshaking_conns: HashSet<Token>,
    /// shared `QUIC` configuration.
    config: quiche::Config,
}

impl Connector {
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

    /// Move established `connection` from `handshaking` pool into `active` pool.
    pub fn connected(&mut self, token: Token) {
        log::info!("put `QUIC` connection {:?}.", token);

        assert!(self.handshaking_conns.remove(&token));
        assert!(self.conns.insert(token));
    }

    /// Remove closed connection.
    pub fn closed(&mut self, token: Token) {
        log::info!("remove `QUIC` connection {:?}.", token);

        if !self.handshaking_conns.remove(&token) {
            self.conns.remove(&token);
        }
    }

    /// Select a connection at random from the pool and establish a
    /// bidirectional data stream over that connection.
    pub fn stream_open(&mut self, group: &Group) -> Result<(Token, u64)> {
        let mut conns = self.conns.iter().cloned().collect::<Vec<_>>();

        conns.shuffle(&mut rand::rng());

        for conn_id in conns {
            match group.stream_open(conn_id) {
                Ok(stream_id) => return Ok((conn_id, stream_id)),
                Err(Error::Busy) | Err(Error::Retry) => {}
                Err(_) => {
                    log::info!("Remove quic connection {:?}", conn_id);
                    self.conns.remove(&conn_id);
                }
            }
        }

        if self.conns.len() + self.handshaking_conns.len() < self.max_pool_size {
            // Create new `QUIC` connection.
            self.targets.shuffle(&mut rand::rng());

            let token = group.connect(None, self.local_addr, self.targets[0], &mut self.config)?;

            log::info!("`QUIC` connect to {}, token={:?}", self.targets[0], token);

            self.handshaking_conns.insert(token);
        }

        Err(Error::Retry)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, EventKind, Group};

    #[test]
    fn test_connect_events() {
        let group = Group::new();

        let token = group
            .connect(
                None,
                "127.0.0.1:1".parse().unwrap(),
                "127.0.0.1:2".parse().unwrap(),
                &mut quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap(),
            )
            .unwrap();

        let mut events = vec![];

        group.poll(&mut events, None).unwrap();

        assert_eq!(
            events,
            vec![Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token,
                stream_id: 0
            }]
        );
    }
}
