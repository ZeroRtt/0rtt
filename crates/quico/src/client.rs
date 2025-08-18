use std::net::SocketAddr;

use quiche::Config;

use crate::{Group, Result, Token, utils::random_conn_id};

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

        self.register(conn)
    }
}
