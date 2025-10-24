//! Asynchronous API based [`zerortt::mio`]

/// `QUIC` group for asynchronous runtimes.
pub type Group = crate::futures::Group<crate::mio::Group>;

/// `QUIC` connection for asynchronous runtimes.
pub type QuicConn = crate::futures::QuicConn<crate::mio::Group>;

/// `QUIC` stream for asynchronous runtimes.
pub type QuicStream = crate::futures::QuicStream<crate::mio::Group>;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod server {
    /// A **server-side** socket that accept inbound `QUIC` connections/streams.
    pub type QuicListener = crate::futures::QuicListener<crate::mio::Group>;
}

#[cfg(feature = "server")]
pub use server::*;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
mod client {
    /// Connector for `QUIC` client.
    pub type QuicConnector = crate::futures::QuicConnector<crate::mio::Group>;
}

#[cfg(feature = "client")]
pub use client::*;
