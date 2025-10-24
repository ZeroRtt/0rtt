//! Types for handle this crate's errors.

use std::task::Poll;

/// A `poll` error.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A quic protocol error.
    #[error(transparent)]
    Quiche(#[from] quiche::Error),

    /// The operation needs to block to complete, but the blocking operation was
    /// requested to not occur.
    ///
    /// Should retry this operation later.
    #[error("Retry this operation later.")]
    Retry,

    /// Resource is busy, should retry this operation later.
    #[error("Resource is busy.")]
    Busy,

    /// Resource is not found.
    #[error("Resource is not found.")]
    NotFound,

    #[error("Failed to validate client address.")]
    ValidateAddress,

    #[error("Maximumn currently opened streams.")]
    MaxStreams,
}

impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::Quiche(error) => std::io::Error::other(error),
            Error::Retry | Error::Busy => {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, value)
            }

            Error::NotFound => std::io::Error::new(std::io::ErrorKind::NotFound, value),
            _ => std::io::Error::other(value),
        }
    }
}

/// Short for `std::result::Result<T, crate::poll::Error>`
pub type Result<T> = std::result::Result<T, Error>;

/// A trait to filter `WouldBlock` like errors and convert them into `Poll::Pending`
pub trait WouldBlock<T> {
    fn would_block(self) -> Poll<Result<T>>;
}

impl<T> WouldBlock<T> for Result<T> {
    fn would_block(self) -> Poll<Result<T>> {
        match self {
            Err(Error::Busy) | Err(Error::Retry) => Poll::Pending,
            _ => Poll::Ready(self),
        }
    }
}
