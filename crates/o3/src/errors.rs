use std::io::ErrorKind;

use crate::buf::QuicBuf;

/// Error type used by `o3`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Unhandled `std::io::Error`
    #[error(transparent)]
    Io(std::io::Error),

    /// Unhandled `quico::Error`
    #[error(transparent)]
    Quico(quico::Error),

    /// I/O operation can't be complete immediately.
    #[error("Retry this operation later.")]
    Retry,

    /// Quic socket sending queue is full.
    #[error("Quic socket sending queue is full.")]
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
            quico::Error::Retry | quico::Error::Busy => Self::Retry,
            _ => Self::Quico(value),
        }
    }
}

/// Sort of type `std::result::Result<T, Error>`
pub type Result<T> = std::result::Result<T, Error>;
