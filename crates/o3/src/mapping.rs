//! Utilities for port mapping.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    errors::{Error, Result},
    port::{BufPort, copy},
    token::Token,
};

/// Port mapping.
#[derive(Default)]
pub struct Mapping {
    /// Register ports.
    ports: HashMap<Token, Rc<RefCell<BufPort>>>,
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
                .insert(from.token(), Rc::new(RefCell::new(from)))
                .is_none()
        );
        assert!(
            self.ports
                .insert(to.token(), Rc::new(RefCell::new(to)))
                .is_none()
        );
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

        let source = self.ports.get(&from).cloned().expect("from port");
        let sink = self.ports.get(&to).cloned().expect("to port");

        let mut source = source.borrow_mut();
        let mut sink = sink.borrow_mut();

        match copy(&mut source, &mut sink) {
            Err(Error::Retry) => Err(Error::Retry),
            Err(err) => {
                _ = source.close();
                _ = sink.close();

                log::info!(
                    "deregister port mapping, from={}, to={}, ports={}, cause={}",
                    source.trace_id(),
                    sink.trace_id(),
                    self.ports.len() - 2,
                    err,
                );

                assert!(self.ports.remove(&from).is_some());
                assert!(self.ports.remove(&to).is_some());

                assert_eq!(self.mapping.remove(&from), Some(to));
                assert_eq!(self.mapping.remove(&to), Some(from));

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

    // fn get(&self, token: &Token) -> Option<&'static mut BufPort> {
    //     unsafe {
    //         self.ports
    //             .get(token)
    //             .map(|port| port.get().as_mut().unwrap())
    //     }
    // }
}
