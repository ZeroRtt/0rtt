//! Utilities for port mapping.

use std::{cell::UnsafeCell, collections::HashMap};

use crate::{
    errors::{Error, Result},
    metrics::Metrics,
    port::{BufPort, copy},
    registry::QuicRegistry,
    token::Token,
};

/// Port mapping.
#[derive(Default)]
pub struct Mapping {
    /// Register ports.
    ports: HashMap<Token, UnsafeCell<BufPort>>,
    /// bidirection port mapping.
    mapping: HashMap<Token, Token>,
    /// Registry for quic streams.
    quic_registry: QuicRegistry,
}

impl Mapping {
    /// Register a new mapping.
    pub fn register(&mut self, from: BufPort, to: BufPort) {
        log::info!(
            "register port mapping, from={}, to={}, ports={}",
            from.trace_id(),
            to.trace_id(),
            self.ports.len() + 2
        );

        if let Token::QuicStream(token, stream_id) = from.token() {
            self.quic_registry.register(quico::Token(token), stream_id);
        } else if let Token::QuicStream(token, stream_id) = to.token() {
            self.quic_registry.register(quico::Token(token), stream_id);
        }

        assert!(self.mapping.insert(from.token(), to.token()).is_none());
        assert!(self.mapping.insert(to.token(), from.token()).is_none());

        assert!(
            self.ports
                .insert(from.token(), UnsafeCell::new(from))
                .is_none()
        );
        assert!(self.ports.insert(to.token(), UnsafeCell::new(to)).is_none());
    }

    pub fn on_quic_closed(&mut self, token: quico::Token) {
        if let Some(streams) = self.quic_registry.close(token) {
            for stream_id in streams {
                let from = Token::QuicStream(token.0, stream_id);
                let to = self.mapping.get(&from).copied().unwrap();

                let source = self.get(&from).expect("from port");
                let sink = self.get(&to).expect("to port");

                log::info!(
                    "deregister port mapping, from={}, to={}, sent={}, recv={}, ports={}",
                    source.trace_id(),
                    sink.trace_id(),
                    source.sent(),
                    sink.sent(),
                    self.ports.len() - 2,
                );

                assert!(self.ports.remove(&from).is_some());
                assert!(self.ports.remove(&to).is_some());

                assert_eq!(self.mapping.remove(&from), Some(to));
                assert_eq!(self.mapping.remove(&to), Some(from));
            }
        }
    }

    /// Returns true if the `Mapping` contains a port for the specified `token`.
    #[inline]
    pub fn contains_port<T>(&self, token: T) -> bool
    where
        T: Into<Token>,
    {
        self.ports.contains_key(&token.into())
    }

    /// Try transfer data between ports.
    /// this method will automatic deregister current port mapping,
    /// if [`Fin`](Error::Fin) error is raised by underlying object.
    ///
    /// When this method returns value `Ok(0)`, it indicates that the port mapping has been broken.
    pub fn transfer<F, T>(&mut self, from: F, to: T, metrics: &mut Metrics) -> Result<usize>
    where
        F: Into<Token>,
        T: Into<Token>,
    {
        let from = from.into();
        let to = to.into();

        let source = self.get(&from).expect("from port");
        let sink = self.get(&to).expect("to port");

        match copy(source, sink) {
            Err(Error::Retry) => Err(Error::Retry),
            Err(_) => {
                _ = source.close();
                _ = sink.close();

                log::info!(
                    "deregister port mapping, from={}, to={}, sent={}, recv={}, ports={}",
                    source.trace_id(),
                    sink.trace_id(),
                    source.sent(),
                    sink.sent(),
                    self.ports.len() - 2,
                );

                assert!(self.ports.remove(&from).is_some());
                assert!(self.ports.remove(&to).is_some());

                assert_eq!(self.mapping.remove(&from), Some(to));
                assert_eq!(self.mapping.remove(&to), Some(from));

                if let Token::QuicStream(token, stream_id) = from {
                    self.quic_registry
                        .deregister(quico::Token(token), stream_id);
                } else if let Token::QuicStream(token, stream_id) = to {
                    self.quic_registry
                        .deregister(quico::Token(token), stream_id);
                }

                Ok(0)
            }
            Ok(transferred) => {
                metrics.traffic_add(from, to, transferred);

                Ok(transferred)
            }
        }
    }

    /// Try transfer data from port for the specified `token`.
    /// this method will automatic deregister current port mapping,
    /// if [`Fin`](Error::Fin) error is raised by underlying object.
    ///
    /// When this method returns value `Ok(0)`, it indicates that the port mapping has been broken.
    pub fn transfer_from<T>(&mut self, token: T, metrics: &mut Metrics) -> Result<usize>
    where
        T: Into<Token>,
    {
        let from = token.into();

        let to = self
            .mapping
            .get(&from)
            .cloned()
            .ok_or_else(|| Error::Mapping)?;

        self.transfer(from, to, metrics)
    }

    /// Try transfer data to port for the specified `token`.
    /// this method will automatic deregister current port mapping,
    /// if [`Fin`](Error::Fin) error is raised by underlying object.
    ///
    /// When this method returns value `Ok(0)`, it indicates that the port mapping has been broken.
    pub fn transfer_to<T>(&mut self, token: T, metrics: &mut Metrics) -> Result<usize>
    where
        T: Into<Token>,
    {
        let to = token.into();

        let from = self
            .mapping
            .get(&to)
            .cloned()
            .ok_or_else(|| Error::Mapping)?;

        self.transfer(from, to, metrics)
    }

    fn get(&self, token: &Token) -> Option<&'static mut BufPort> {
        unsafe {
            self.ports
                .get(token)
                .map(|port| port.get().as_mut().unwrap())
        }
    }
}
