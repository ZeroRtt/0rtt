//! Asynchronous API based [`zerortt::mio`]

/// `QUIC` group for asynchronous runtimes.
pub type Group = zerortt_futures::Group<crate::Group>;

/// `QUIC` connection for asynchronous runtimes.
pub type QuicConn = zerortt_futures::QuicConn<crate::Group>;

/// `QUIC` stream for asynchronous runtimes.
pub type QuicStream = zerortt_futures::QuicStream<crate::Group>;

/// A **server-side** socket that accept inbound `QUIC` connections/streams.
pub type QuicListener = zerortt_futures::QuicListener<crate::Group>;

/// Connector for `QUIC` client.
pub type QuicConnector = zerortt_futures::QuicConnector<crate::Group>;
