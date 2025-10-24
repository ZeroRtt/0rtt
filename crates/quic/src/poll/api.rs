use std::{
    borrow::Cow,
    net::{SocketAddr, ToSocketAddrs},
    time::Instant,
};

use quiche::{Config, RecvInfo, SendInfo};

#[cfg(feature = "server")]
use crate::Acceptor;
use crate::{Event, StreamKind, Token};

/// A poll api for `QUIC` group.
pub trait QuicPoll: Send + Sync {
    type Error;
    /// Waits for readiness events without blocking current thread
    /// and returns possible retry time duration.
    fn poll(&self, events: &mut Vec<Event>) -> Result<Option<Instant>, Self::Error>;

    /// Wrap and handle a new `quiche::Connection`.
    ///
    /// On success, returns a reference handle [`Token`] to the [`Connection`](quiche::Connection).
    fn register(&self, wrapped: quiche::Connection) -> Result<Token, Self::Error>;

    /// Unwrap a `quiche::Connection` referenced by the handle [`Token`].
    fn deregister(&self, token: Token) -> Result<quiche::Connection, Self::Error>;

    /// Returns the number of `QUIC` connections in the group.
    fn len(&self) -> usize;

    /// Close a wrapped `QUIC` connection.
    ///
    /// This function does not immediately remove the `QUIC` connection
    /// from memory, but instead waits until the `QUIC` connection has
    /// completed its closure process before actually removing it from
    /// memory.
    ///
    /// See [`close`](quiche::Connection::close) for more information.
    fn close(
        &self,
        token: Token,
        app: bool,
        err: u64,
        reason: Cow<'static, [u8]>,
    ) -> Result<(), Self::Error>;

    /// Open a local stream via a `QUIC` connection.
    fn stream_open(
        &self,
        token: Token,
        kind: StreamKind,
        max_streams_as_error: bool,
    ) -> Result<u64, Self::Error>;

    ///Shuts down reading and writing from/to the specified stream.
    ///
    /// See [`stream_send`](quiche::Connection::stream_shutdown) for more information.
    fn stream_shutdown(&self, token: Token, stream_id: u64, err: u64) -> Result<(), Self::Error>;

    /// Writes data to a stream.
    ///
    /// See [`stream_send`](quiche::Connection::stream_send) for more information.
    fn stream_send(
        &self,
        token: Token,
        stream_id: u64,
        buf: &[u8],
        fin: bool,
    ) -> Result<usize, Self::Error>;

    /// Reads contiguous data from a stream into the provided slice.
    ///
    /// See [`stream_recv`](quiche::Connection::stream_recv) for more information.
    fn stream_recv(
        &self,
        token: Token,
        stream_id: u64,
        buf: &mut [u8],
    ) -> Result<(usize, bool), Self::Error>;
}

/// `API` for client-side `QUIC`.
#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub trait QuicClient: QuicPoll {
    /// Creates a new client-side connection.
    fn connect(
        &self,
        server_name: Option<&str>,
        local: SocketAddr,
        peer: SocketAddr,
        config: &mut Config,
    ) -> Result<Token, Self::Error>;
}

/// Underlying transport layer API for `QUIC` group.
pub trait QuicTransport {
    type Error;

    /// Processes QUIC packets received from the peer.
    fn recv(&self, buf: &mut [u8], info: RecvInfo) -> Result<usize, Self::Error>;

    /// Writes a single QUIC packet to be sent to the peer.
    fn send(&self, token: Token, buf: &mut [u8]) -> Result<(usize, SendInfo), Self::Error>;
}

/// Underlying transport layer API for server-side `QUIC` group.
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub trait QuicServerTransport: QuicTransport {
    /// Server-side `recv` function, supports for `QUIC` handshake process.
    fn recv_with_acceptor(
        &self,
        acceptor: &mut Acceptor,
        buf: &mut [u8],
        recv_size: usize,
        recv_info: RecvInfo,
        unparker: Option<&crossbeam_utils::sync::Unparker>,
    ) -> Result<(usize, SendInfo), Self::Error>;
}

/// A `group` with underlying transport layer.
pub trait QuicBind: QuicPoll + Sized {
    /// Create a new `Group` and bind it to `laddrs`.
    fn bind<S>(laddrs: S, acceptor: Option<Acceptor>) -> Result<Self, Self::Error>
    where
        S: ToSocketAddrs;

    /// Returns local bound addresses.
    fn local_addrs(&self) -> impl Iterator<Item = &SocketAddr>;
}
