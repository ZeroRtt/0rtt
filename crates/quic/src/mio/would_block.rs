//! Utilities for handling `WouldBlock` like errors.

use std::{io::ErrorKind, task::Poll};

use crate::mio::udp::QuicSocketError;

/// A trait to filter `WouldBlock` like errors and convert them into `Poll::Pending`
pub(crate) trait WouldBlock<T> {
    type Error;
    fn would_block(self) -> Poll<Result<T, Self::Error>>;
}

impl<T> WouldBlock<T> for Result<T, std::io::Error> {
    type Error = std::io::Error;

    fn would_block(self) -> Poll<Result<T, std::io::Error>> {
        match self {
            Err(err) if err.kind() == ErrorKind::WouldBlock => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}

impl<T> WouldBlock<T> for Result<T, QuicSocketError> {
    type Error = QuicSocketError;

    fn would_block(self) -> Poll<Result<T, QuicSocketError>> {
        match self {
            Err(QuicSocketError::IO(err)) if err.kind() == ErrorKind::WouldBlock => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}

impl<T> WouldBlock<T> for Result<T, crate::poll::Error> {
    type Error = crate::poll::Error;

    fn would_block(self) -> Poll<Result<T, crate::poll::Error>> {
        match self {
            Err(crate::poll::Error::Busy) | Err(crate::poll::Error::Retry) => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}
