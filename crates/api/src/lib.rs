//！ Primitive types and traits for `zerortt`
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::{
    borrow::Cow,
    net::{SocketAddr, ToSocketAddrs},
    time::Instant,
};

use quiche::{Config, ConnectionId, RecvInfo, SendInfo};

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

    /// Resource is not found.
    #[error("Resource is not found.")]
    NotFound,

    #[error("Failed to validate client address.")]
    ValidateAddress,

    #[error("Maximumn currently opened streams.")]
    MaxStreams,
}

impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::Quiche(error) => std::io::Error::other(error),
            Error::Retry | Error::Busy => {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, value)
            }

            Error::NotFound => std::io::Error::new(std::io::ErrorKind::NotFound, value),
            _ => std::io::Error::other(value),
        }
    }
}

/// Short for `std::result::Result<T, crate::poll::Error>`
pub type Result<T> = std::result::Result<T, Error>;

/// A trait to filter `WouldBlock` like errors and convert them into `Poll::Pending`
pub trait WouldBlock<T> {
    type Error;

    fn would_block(self) -> std::task::Poll<std::result::Result<T, Self::Error>>;
}

impl<T> WouldBlock<T> for Result<T> {
    type Error = Error;

    fn would_block(self) -> std::task::Poll<Result<T>> {
        match self {
            Err(Error::Busy) | Err(Error::Retry) => std::task::Poll::Pending,
            _ => std::task::Poll::Ready(self),
        }
    }
}

impl<T> WouldBlock<T> for std::io::Result<T> {
    type Error = std::io::Error;

    fn would_block(self) -> std::task::Poll<std::result::Result<T, Self::Error>> {
        match self {
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => std::task::Poll::Pending,
            _ => std::task::Poll::Ready(self),
        }
    }
}

/// Generate a random `ConnectionId`
#[inline]
pub fn random_conn_id() -> ConnectionId<'static> {
    let mut buf = vec![0; 20];
    boring::rand::rand_bytes(&mut buf).unwrap();

    ConnectionId::from_vec(buf)
}

/// Returns true if the stream was created locally.
#[inline]
pub fn is_local(stream_id: u64, is_server: bool) -> bool {
    (stream_id & 0x1) == (is_server as u64)
}

/// Returns true if the stream is bidirectional.
#[inline]
pub fn is_bidi(stream_id: u64) -> bool {
    (stream_id & 0x2) == 0
}

/// Type for readiness events.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum EventKind {
    /// Readiness for `send` operation.
    Send,
    /// Readiness for `recv` operation.
    Recv,
    /// Client-side connection handshake is completed.
    Connected,
    /// Server-side connection handshake is completed.
    Accept,
    /// Connection is closed.
    Closed,
    /// Readiness for `stream_open` operation.
    StreamOpenBidi,
    /// Readiness for `stream_open` operation.
    StreamOpenUni,
    /// Readiness for new inbound stream.
    StreamAccept,
    /// Readiness for `stream_send` operation.
    StreamSend,
    /// Readiness for `stream_recv` operation.
    StreamRecv,
    /// Read lock.
    ReadLock,
}

/// Associates readiness events with quic connection.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Token(pub u32);

/// Readiness I/O event.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Event {
    /// Connection token.
    pub token: Token,
    /// Type of this event.
    pub kind: EventKind,
    /// Event source is a server-side connection.
    pub is_server: bool,
    /// Event raised by an I/O error.
    pub is_error: bool,
    /// The meaning of this field depends on the [`kind`](Self::kind) field.
    pub stream_id: u64,
}

/// Stream type.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum StreamKind {
    Uni,
    Bidi,
}

impl From<u64> for StreamKind {
    fn from(value: u64) -> Self {
        if is_bidi(value) {
            StreamKind::Bidi
        } else {
            StreamKind::Uni
        }
    }
}

/// A poll api for `QUIC` group.
pub trait QuicPoll: Send + Sync {
    type Error;
    /// Waits for readiness events without blocking current thread
    /// and returns possible retry time duration.
    fn poll(&self, events: &mut Vec<Event>) -> std::result::Result<Option<Instant>, Self::Error>;

    /// Wrap and handle a new `quiche::Connection`.
    ///
    /// On success, returns a reference handle [`Token`] to the [`Connection`](quiche::Connection).
    fn register(&self, wrapped: quiche::Connection) -> std::result::Result<Token, Self::Error>;

    /// Unwrap a `quiche::Connection` referenced by the handle [`Token`].
    fn deregister(&self, token: Token) -> std::result::Result<quiche::Connection, Self::Error>;

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
    ) -> std::result::Result<(), Self::Error>;

    /// Open a local stream via a `QUIC` connection.
    fn stream_open(
        &self,
        token: Token,
        kind: StreamKind,
        non_blocking: bool,
    ) -> std::result::Result<Option<u64>, Self::Error>;

    ///Shuts down reading and writing from/to the specified stream.
    ///
    /// See [`stream_send`](quiche::Connection::stream_shutdown) for more information.
    fn stream_shutdown(
        &self,
        token: Token,
        stream_id: u64,
        err: u64,
    ) -> std::result::Result<(), Self::Error>;

    /// Writes data to a stream.
    ///
    /// See [`stream_send`](quiche::Connection::stream_send) for more information.
    fn stream_send(
        &self,
        token: Token,
        stream_id: u64,
        buf: &[u8],
        fin: bool,
    ) -> std::result::Result<usize, Self::Error>;

    /// Reads contiguous data from a stream into the provided slice.
    ///
    /// See [`stream_recv`](quiche::Connection::stream_recv) for more information.
    fn stream_recv(
        &self,
        token: Token,
        stream_id: u64,
        buf: &mut [u8],
    ) -> std::result::Result<(usize, bool), Self::Error>;
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
    ) -> std::result::Result<Token, Self::Error>;
}

/// Underlying transport layer API for `QUIC` group.
pub trait QuicTransport {
    type Error;

    /// Processes QUIC packets received from the peer.
    fn recv(&self, buf: &mut [u8], info: RecvInfo) -> std::result::Result<usize, Self::Error>;

    /// Writes a single QUIC packet to be sent to the peer.
    fn send(
        &self,
        token: Token,
        buf: &mut [u8],
    ) -> std::result::Result<(usize, SendInfo), Self::Error>;
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
    ) -> std::result::Result<(usize, SendInfo), Self::Error>;
}

/// A `group` with underlying transport layer.
pub trait QuicBind: QuicPoll + Sized {
    /// Create a new `Group` and bind it to `laddrs`.
    #[cfg(feature = "server")]
    #[cfg_attr(docsrs, doc(cfg(feature = "server")))]
    fn bind<S>(laddrs: S, acceptor: Option<Acceptor>) -> std::result::Result<Self, Self::Error>
    where
        S: ToSocketAddrs;

    /// Create a new `Group` and bind it to `laddrs`.
    #[cfg(not(feature = "server"))]
    #[cfg_attr(docsrs, doc(cfg(not(feature = "server"))))]
    fn bind<S>(laddrs: S) -> std::result::Result<Self, Self::Error>
    where
        S: ToSocketAddrs;

    /// Returns local bound addresses.
    fn local_addrs(&self) -> impl Iterator<Item = &SocketAddr>;
}

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod acceptor;
#[cfg(feature = "server")]
pub use acceptor::*;

pub use quiche;
