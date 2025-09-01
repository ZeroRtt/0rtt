use std::{cell::UnsafeCell, collections::HashMap};

use crate::{
    errors::{Error, Result},
    port::{BufPort, copy},
    token::Token,
};

/// A router maintains a set of port-to-port bi-directional
/// data routing relationships
#[derive(Default)]
pub struct Router {
    ports: HashMap<Token, UnsafeCell<BufPort>>,
    sinks: HashMap<Token, Token>,
    sources: HashMap<Token, Token>,
}

impl Router {
    /// Create a new `empty` routing table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the router contains a `Port` for the specified token.
    #[inline]
    pub fn contains_port<T>(&self, token: T) -> bool
    where
        T: Into<Token>,
    {
        self.ports.contains_key(&token.into())
    }

    /// Register a routing rule.
    pub fn register(&mut self, from: BufPort, to: BufPort) {
        let from_token = from.token();
        let to_token = to.token();

        assert_ne!(
            from_token, to_token,
            "The source and sink ports are the same"
        );

        assert!(
            self.ports
                .insert(from.token(), UnsafeCell::new(from))
                .is_none(),
            "Register from twice."
        );

        assert!(
            self.ports.insert(to.token(), UnsafeCell::new(to)).is_none(),
            "Register to twice."
        );

        self.sinks.insert(from_token, to_token);
        self.sources.insert(to_token, from_token);

        self.sinks.insert(to_token, from_token);
        self.sources.insert(from_token, to_token);

        log::trace!("Register route, from={:?}, to={:?}", from_token, to_token);
    }

    /// Send data over port.
    pub fn send<T>(&mut self, token: T) -> Result<usize>
    where
        T: Into<Token>,
    {
        let sink_token = token.into();

        let source_token = self
            .sources
            .get_mut(&sink_token)
            .copied()
            .ok_or_else(|| Error::Source(sink_token))?;

        self.transfer(source_token, sink_token)
    }

    /// Recv data from port.
    pub fn recv<T>(&mut self, token: T) -> Result<usize>
    where
        T: Into<Token>,
    {
        let source_token = token.into();

        let sink_token = self
            .sinks
            .get_mut(&source_token)
            .copied()
            .ok_or_else(|| Error::Sink(source_token))?;

        self.transfer(source_token, sink_token)
    }

    fn transfer(&mut self, source_token: Token, sink_token: Token) -> Result<usize> {
        // Safety: The router guarantees that only one mutable reference to one port will be created at a time
        let source = unsafe { self.get(&source_token, "Source port.") };
        let sink = unsafe { self.get(&sink_token, "Sink port.") };

        match copy(source, sink) {
            Ok(transferred) => {
                log::trace!(
                    "Router transferred data, from={:?}, to={:?}, len={}",
                    source_token,
                    sink_token,
                    transferred
                );

                Ok(transferred)
            }
            Err(Error::Retry) => Err(Error::Retry),
            Err(_) => {
                assert_eq!(self.sources.remove(&sink_token), Some(source_token));
                assert_eq!(self.sources.remove(&source_token), Some(sink_token));
                assert_eq!(self.sinks.remove(&source_token), Some(sink_token));
                assert_eq!(self.sinks.remove(&sink_token), Some(source_token));
                Ok(0)
            }
        }
    }

    unsafe fn get(&mut self, token: &Token, msg: &'static str) -> &'static mut BufPort {
        unsafe { self.ports.get(token).expect(msg).get().as_mut().unwrap() }
    }
}
