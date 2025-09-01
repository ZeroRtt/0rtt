use std::io::ErrorKind;

use quico::quiche;

use crate::{buf::QuicBuf, token::Token};

/// Error type used by `o3`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Unhandled `std::io::Error`
    #[error(transparent)]
    Io(std::io::Error),

    /// Unhandled `quiche::Error`
    #[error(transparent)]
    Quiche(quiche::Error),

    /// I/O operation can't be complete immediately.
    #[error("Retry this operation later.")]
    Retry,

    /// Sink port is not found.
    #[error("Sink port of {0:?} is not found.")]
    Sink(Token),

    /// Source port is not found.
    #[error("Source port of {0:?} is not found.")]
    Source(Token),

    #[error("Port is not found.")]
    Port,

    #[error("Failed to validate quic client address.")]
    ValidateAddress,

    #[error("Quic socket send queue is full.")]
    IsFull(QuicBuf),
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            ErrorKind::WouldBlock => Self::Retry,
            _ => Self::Io(value),
        }
    }
}

impl From<quico::Error> for Error {
    fn from(value: quico::Error) -> Self {
        match value {
            quico::Error::Quiche(error) => Self::Quiche(error),
            quico::Error::Retry | quico::Error::Busy => Self::Retry,
            quico::Error::NotFound => Self::Port,
            quico::Error::ValidateAddress => Self::ValidateAddress,
        }
    }
}

/// Sort of type `std::result::Result<T, Error>`
pub type Result<T> = std::result::Result<T, Error>;
