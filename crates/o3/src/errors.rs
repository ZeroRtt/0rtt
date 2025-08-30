use quico::quiche;

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

    /// Returns by `copy` function, the sink port is full.
    #[error("Sink port is would block, retry later.")]
    Sink,

    /// Returns by `copy` function, the source port is empty.
    #[error("Source port is would block, retry later.")]
    Source,
}

/// Sort of type `std::result::Result<T, Error>`
pub type Result<T> = std::result::Result<T, Error>;
