//! Extensions for client-side quic.

use std::net::SocketAddr;

use quiche::Config;

use crate::poll::{Group, Result, Token, utils::random_conn_id};

/// Extensions for client-side quic.
pub trait ClientGroup {
    /// Creates a new client-side connection.
    fn connect(
        &self,
        server_name: Option<&str>,
        local: SocketAddr,
        peer: SocketAddr,
        config: &mut Config,
    ) -> Result<Token>;
}

impl ClientGroup for Group {
    fn connect(
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
