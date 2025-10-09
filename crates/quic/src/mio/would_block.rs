//! Utilities for handling `WouldBlock` like errors.

use std::{io::ErrorKind, task::Poll};

/// A trait to filter `WouldBlock` like errors and convert them into `Poll::Pending`
pub trait WouldBlock<T> {
    fn would_block(self) -> Poll<Result<T, std::io::Error>>;
}

impl<T> WouldBlock<T> for Result<T, std::io::Error> {
    fn would_block(self) -> Poll<Result<T, std::io::Error>> {
        match self {
            Err(err) if err.kind() == ErrorKind::WouldBlock => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}
