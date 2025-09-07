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
        log::trace!(
            "register port mapping, from={}, to={}",
            from.trace_id(),
            to.trace_id()
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

    /// Try transfer data from port for the specified `token`.
    /// this method will automatic deregister current port mapping,
    /// if [`Fin`](Error::Fin) error is raised by underlying object.
    ///
    /// When this method returns value `Ok(0)`, it indicates that the port mapping has been broken.
    pub fn transfer<T>(&mut self, token: T) -> Result<usize>
    where
        T: Into<Token>,
    {
        let from = token.into();

        let to = self
            .mapping
            .get(&from)
            .cloned()
            .ok_or_else(|| Error::Mapping)?;

        let mut source = self.get(&from).expect("from port");
        let mut sink = self.get(&to).expect("to port");

        match copy(&mut source, &mut sink) {
            Err(Error::Fin(_, _)) => {
                log::trace!(
                    "deregister port mapping, from={}, to={}",
                    source.trace_id(),
                    sink.trace_id()
                );

                assert_eq!(self.mapping.remove(&from), Some(to));
                assert_eq!(self.mapping.remove(&to), Some(from));

                Ok(0)
            }
            r => r,
        }
    }

    fn get(&self, token: &Token) -> Option<&'static mut BufPort> {
        unsafe {
            self.ports
                .get(token)
                .map(|port| port.get().as_mut().unwrap())
        }
    }
}
