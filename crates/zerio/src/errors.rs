/// The error type used by `zerio`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {}

/// Short for `std::result::Result<T, zerio::Error>`
pub type Result<T> = std::result::Result<T, Error>;
