/// Readiness event kind for transports.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    Readable,
    Writable,
    Accept,
    /// Transport sendable.
    Transport,
}
