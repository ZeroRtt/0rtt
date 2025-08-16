/// `ConnState` resource acquire kind.
pub enum LocKind {
    Send,
    Recv,
    Close,
    StreamOpen,
    StreamSend,
    StreamRecv,
    StreamShutdown,
}

/// Internal connection state.
pub struct ConnState {
    /// Wrapped `quiche::Connection`
    conn: quiche::Connection,
}
