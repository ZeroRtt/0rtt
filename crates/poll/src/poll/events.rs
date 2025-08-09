/// Readiness event type.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum EventKind {
    /// Connection is readable.
    Send,
    /// Connection is writable.
    Recv,
    /// Quic connection is established.
    Established,
    /// Connection is closed.
    Closed,
    /// Stream is writable.
    StreamSend,
    /// Stream is readable.
    StreamRecv,
    /// Retry to open new local stream.
    StreamOpen,
}

/// An I/O readiness event.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Event {
    /// event type.
    pub kind: EventKind,
    /// True If this event is in response to a [`Busy`](super::Error::Busy) error
    pub lock_released: bool,
    /// True if the event contains error readiness.
    pub is_error: bool,
    /// source connection id.
    pub conn_id: u32,
    /// optional stream id.
    pub stream_id: u64,
}
