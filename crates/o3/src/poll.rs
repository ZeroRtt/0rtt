//! Utilities for handling `WouldBlock` like errors.

use std::{io::ErrorKind, task::Poll};

use crate::errors::Error;

/// A trait to filter `WouldBlock` like errors and convert them into `Poll::Pending`
pub trait WouldBlock<T> {
    type Error;

    fn would_block(self) -> Poll<Result<T, Self::Error>>;
}

impl<T> WouldBlock<T> for Result<T, std::io::Error> {
    type Error = std::io::Error;

    fn would_block(self) -> Poll<Result<T, Self::Error>> {
        match self {
            Err(err) if err.kind() == ErrorKind::WouldBlock => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}

impl<T> WouldBlock<T> for Result<T, quico::Error> {
    type Error = quico::Error;

    fn would_block(self) -> Poll<Result<T, Self::Error>> {
        match self {
            Err(quico::Error::Busy) | Err(quico::Error::Retry) => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}

impl<T> WouldBlock<T> for Result<T, Error> {
    type Error = Error;

    fn would_block(self) -> Poll<Result<T, Self::Error>> {
        match self {
            Err(Error::Retry) => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}
