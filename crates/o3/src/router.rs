use std::collections::HashMap;

use crate::{port::BufPort, token::Token};

/// A router maintains a set of port-to-port bi-directional
/// data routing relationships
#[derive(Default)]
pub struct Router {
    ports: HashMap<Token, BufPort>,
    sinks: HashMap<Token, Token>,
    sources: HashMap<Token, Token>,
}

impl Router {
    /// Create a new `empty` routing table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a routing rule.
    pub fn register(&mut self, from: BufPort, to: BufPort) {
        let from_token = from.token();
        let to_token = to.token();

        assert!(
            self.ports.insert(from.token(), from).is_none(),
            "Register from twice."
        );

        assert!(
            self.ports.insert(to.token(), to).is_none(),
            "Register to twice."
        );

        self.sinks.insert(from_token, to_token);
        self.sources.insert(to_token, from_token);
    }
}
