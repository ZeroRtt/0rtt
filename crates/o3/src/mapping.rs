//! Utilities for port mapping.

use std::{cell::UnsafeCell, collections::HashMap};

use crate::{
    errors::{Error, Result},
    port::{BufPort, copy},
    token::Token,
};

/// Port mapping.
#[derive(Default)]
pub struct Mapping {
    /// Register ports.
    ports: HashMap<Token, UnsafeCell<BufPort>>,
    /// bidirection port mapping.
    mapping: HashMap<Token, Token>,
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

        assert!(self.mapping.insert(from.token(), to.token()).is_none());
        assert!(self.mapping.insert(to.token(), from.token()).is_none());

        assert!(
            self.ports
                .insert(from.token(), UnsafeCell::new(from))
                .is_none()
        );
        assert!(self.ports.insert(to.token(), UnsafeCell::new(to)).is_none());
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
    pub fn transfer<F, T>(&mut self, from: F, to: T) -> Result<usize>
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
                _ = copy(sink, source);
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

                self.mapping.remove(&from);
                self.mapping.remove(&to);

                Ok(0)
            }
            Ok(transferred) => Ok(transferred),
        }
    }

    /// Try transfer data from port for the specified `token`.
    /// this method will automatic deregister current port mapping,
    /// if [`Fin`](Error::Fin) error is raised by underlying object.
    ///
    /// When this method returns value `Ok(0)`, it indicates that the port mapping has been broken.
    pub fn transfer_from<T>(&mut self, token: T) -> Result<usize>
    where
        T: Into<Token>,
    {
        let from = token.into();

        let to = self
            .mapping
            .get(&from)
            .cloned()
            .ok_or_else(|| Error::Mapping)?;

        self.transfer(from, to)
    }

    /// Try transfer data to port for the specified `token`.
    /// this method will automatic deregister current port mapping,
    /// if [`Fin`](Error::Fin) error is raised by underlying object.
    ///
    /// When this method returns value `Ok(0)`, it indicates that the port mapping has been broken.
    pub fn transfer_to<T>(&mut self, token: T) -> Result<usize>
    where
        T: Into<Token>,
    {
        let to = token.into();

        let from = self
            .mapping
            .get(&to)
            .cloned()
            .ok_or_else(|| Error::Mapping)?;

        self.transfer(from, to)
    }

    fn get(&self, token: &Token) -> Option<&'static mut BufPort> {
        unsafe {
            self.ports
                .get(token)
                .map(|port| port.get().as_mut().unwrap())
        }
    }
}
