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
