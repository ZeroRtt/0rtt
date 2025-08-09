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
}

/// Short for `std::result::Result<T, crate::poll::Error>`
pub type Result<T> = std::result::Result<T, Error>;
