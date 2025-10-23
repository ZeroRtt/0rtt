//! Asynchronous Runtime Binding for `QUIC`.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::{Debug, Display},
    future::poll_fn,
    io::{Error, ErrorKind, Result},
    net::{SocketAddr, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Poll, Waker},
};

use futures_util::FutureExt;
use parking_lot::Mutex;

use crate::{
    mio::would_block::WouldBlock,
    poll::{StreamKind, Token, client::ClientGroup, server::Acceptor},
};

/// Event types used by `Group` inner.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum Event {
    Connected,
    StreamAccept,
    StreamOpenBidi,
    StreamOpenUni,
    StreamSend(u64),
    StreamRecv(u64),
}

/// Group inner mutable state.
#[derive(Default)]
struct State {
    stopped: bool,
    /// Waker for accept fn.
    accept: Option<Waker>,
    /// Wakers for connection ops.
    wakers: HashMap<Token, HashMap<Event, Waker>>,
    /// incoming connections first seen.
    incoming_conns: VecDeque<Token>,
    /// Established outbound connection.
    established_conns: HashSet<Token>,
    /// incoming streams first seen.
    incoming_streams: HashMap<Token, VecDeque<u64>>,
}

pub struct GroupWorker {
    group: crate::mio::Group,
    state: Mutex<State>,
}

/// Asynchronous Runtime Binding for `QUIC` group.
#[derive(Clone)]
pub struct Group(Arc<GroupWorker>);

impl Debug for Group {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Group").finish_non_exhaustive()
    }
}

impl Group {
    /// Create a new `Group` and bind to `laddrs`.
    pub fn bind<S>(laddrs: S, acceptor: Option<Acceptor>) -> Result<Self>
    where
        S: ToSocketAddrs,
    {
        Ok(Self(Arc::new(GroupWorker {
            group: crate::mio::Group::bind(laddrs, acceptor)?,
            state: Default::default(),
        })))
    }

    /// Returns `socket` addresses bound to this `Group`.
    pub fn local_addrs(&self) -> impl Iterator<Item = &SocketAddr> {
        self.0.group.local_addrs()
    }

    /// Returns underlying poll api.
    #[inline]
    pub fn poll(&self) -> &crate::mio::Group {
        &self.0.group
    }

    /// Stop this group, and cancel any running background thread.
    pub fn stop(&self) {
        let mut state = self.0.state.lock();

        if let Some(waker) = state.accept.take() {
            waker.wake();
        }

        for (_, wakers) in state.wakers.drain() {
            for (_, waker) in wakers {
                waker.wake();
            }
        }
    }

    /// Consume self and execute `poll events` dispatch loop
    pub fn run(self) -> Result<()> {
        loop {
            if self.run_once()? {
                log::trace!("Stopped group.");
                return Ok(());
            }
        }
    }

    fn run_once(&self) -> Result<bool> {
        let mut events = vec![];

        self.0.group.poll(&mut events, None)?;

        let mut state = self.0.state.lock();

        for event in events {
            log::trace!("readiness, event={:?}", event);
            match event.kind {
                crate::poll::EventKind::Connected => self.on_connected(&mut state, event.token)?,
                crate::poll::EventKind::Accept => self.on_accept(&mut state, event.token)?,
                crate::poll::EventKind::Closed => self.on_closed(&mut state, event.token)?,
                crate::poll::EventKind::StreamOpenBidi => {
                    self.on_stream_open(&mut state, event.token, StreamKind::Bidi)?
                }
                crate::poll::EventKind::StreamOpenUni => {
                    self.on_stream_open(&mut state, event.token, StreamKind::Uni)?
                }
                crate::poll::EventKind::StreamAccept => {
                    self.on_stream_accept(&mut state, event.token, event.stream_id)?
                }
                crate::poll::EventKind::StreamSend => {
                    self.on_stream_send(&mut state, event.token, event.stream_id)?
                }
                crate::poll::EventKind::StreamRecv => {
                    self.on_stream_recv(&mut state, event.token, event.stream_id)?
                }
                _ => unreachable!("illegal event: {:?}", event),
            }
        }

        if self.0.group.len() == 0 && state.stopped {
            log::trace!("background stopped.");
            return Ok(true);
        }

        Ok(false)
    }

    #[inline]
    fn wake(&self, state: &mut State, token: Token, event: Event) {
        if let Some(conn) = state.wakers.get_mut(&token) {
            if let Some(waker) = conn.remove(&event) {
                log::trace!("wakeup, event={:?}", event);
                waker.wake();
            }
        }
    }

    #[inline]
    fn on_closed(&self, state: &mut State, token: Token) -> Result<()> {
        log::trace!("on closed ....");
        // wakeup all pending tasks.
        if let Some(conn) = state.wakers.remove(&token) {
            for (event, waker) in conn {
                log::trace!("wakeup, event={:?}", event);
                waker.wake();
            }
        }

        // remove `Stream` incomoing pipeline.
        state.incoming_streams.remove(&token);

        Ok(())
    }

    #[inline]
    fn on_connected(&self, state: &mut State, token: Token) -> Result<()> {
        state.established_conns.insert(token);
        self.wake(state, token, Event::Connected);
        Ok(())
    }

    #[inline]
    fn on_accept(&self, state: &mut State, token: Token) -> Result<()> {
        state.incoming_conns.push_back(token);

        if let Some(waker) = state.accept.take() {
            waker.wake();
        }

        Ok(())
    }

    #[inline]
    fn on_stream_open(&self, state: &mut State, token: Token, kind: StreamKind) -> Result<()> {
        self.wake(
            state,
            token,
            if kind == StreamKind::Bidi {
                Event::StreamOpenBidi
            } else {
                Event::StreamOpenUni
            },
        );

        Ok(())
    }

    #[inline]
    fn on_stream_accept(&self, state: &mut State, token: Token, stream_id: u64) -> Result<()> {
        state
            .incoming_streams
            .entry(token)
            .or_insert_with(|| VecDeque::new())
            .push_back(stream_id);

        self.wake(state, token, Event::StreamAccept);

        Ok(())
    }

    #[inline]
    fn on_stream_send(&self, state: &mut State, token: Token, stream_id: u64) -> Result<()> {
        self.wake(state, token, Event::StreamSend(stream_id));
        Ok(())
    }

    #[inline]
    fn on_stream_recv(&self, state: &mut State, token: Token, stream_id: u64) -> Result<()> {
        self.wake(state, token, Event::StreamRecv(stream_id));
        Ok(())
    }
}

/// future-awared `QUIC` connection socket.
#[derive(Debug)]
pub struct QuicConn(Group, Token, bool);

impl Drop for QuicConn {
    fn drop(&mut self) {
        assert!(
            self.0
                .0
                .group
                .close(self.1, false, 0x0, b"".into())
                .would_block()
                .is_ready()
        );

        if self.2 {
            self.0.stop();
        }
    }
}

impl QuicConn {
    /// Returns associated underlying `Token` value of this `QuicConn`.
    #[inline]
    pub fn token(&self) -> Token {
        self.1
    }

    /// Check if this connection is established.
    pub async fn is_established(&self) -> Result<()> {
        poll_fn(|cx| {
            let mut state = self.0.0.state.lock();

            if state.established_conns.remove(&self.token()) {
                return Poll::Ready(Ok(()));
            } else {
                state
                    .wakers
                    .entry(self.token())
                    .or_insert_with(|| Default::default())
                    .insert(Event::Connected, cx.waker().clone());

                Poll::Pending
            }
        })
        .await
    }

    /// Accepts a new incoming stream from this `connection`.
    #[inline]
    pub async fn accept(&self) -> Result<QuicStream> {
        poll_fn(|ctx| {
            let mut state = self.0.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            if let Some(stream_id) = state
                .incoming_streams
                .entry(self.1)
                .or_insert_with(|| Default::default())
                .pop_front()
            {
                return Poll::Ready(Ok(QuicStream {
                    group: self.0.clone(),
                    token: self.1,
                    stream_id,
                    shutdown: Default::default(),
                    remote: true,
                }));
            }

            state
                .wakers
                .entry(self.1)
                .or_insert_with(|| Default::default())
                .insert(Event::StreamAccept, ctx.waker().clone());

            Poll::Pending
        })
        .await
    }

    /// Open a new outbound stream.
    pub async fn open(&self, kind: StreamKind, max_streams_as_error: bool) -> Result<QuicStream> {
        let event = match kind {
            StreamKind::Uni => Event::StreamOpenUni,
            StreamKind::Bidi => Event::StreamOpenBidi,
        };

        poll_fn(|ctx| {
            let mut state = self.0.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            state
                .wakers
                .entry(self.1)
                .or_insert_with(|| Default::default())
                .insert(event, ctx.waker().clone());

            drop(state);

            match self
                .0
                .0
                .group
                .stream_open(self.1, kind, max_streams_as_error)
                .would_block()?
            {
                Poll::Ready(stream_id) => {
                    let mut state = self.0.0.state.lock();
                    state.wakers.get_mut(&self.1).unwrap().remove(&event);

                    Poll::Ready(Ok(QuicStream {
                        group: self.0.clone(),
                        token: self.1,
                        stream_id,
                        shutdown: Default::default(),
                        remote: false,
                    }))
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

/// future-awared `QUIC` stream socket.
pub struct QuicStream {
    group: Group,
    token: Token,
    stream_id: u64,
    shutdown: AtomicBool,
    remote: bool,
}

impl Display for QuicStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QuicStream({},{})", self.token.0, self.stream_id)
    }
}

impl Debug for QuicStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicStream")
            .field("token", &self.token)
            .field("stream_id", &self.stream_id)
            .finish()
    }
}

impl Drop for QuicStream {
    fn drop(&mut self) {
        _ = self.shutdown();
    }
}

impl QuicStream {
    /// Write data over this `stream`.
    pub async fn send(&self, buf: &[u8], fin: bool) -> Result<usize> {
        let event = Event::StreamSend(self.stream_id);

        poll_fn(|cx| {
            let mut state = self.group.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            if self.shutdown.load(Ordering::SeqCst) {
                return Poll::Ready(Err(Error::other("Quic stream is shutdown.")));
            }

            state
                .wakers
                .entry(self.token)
                .or_insert_with(|| Default::default())
                .insert(event, cx.waker().clone());

            drop(state);

            match self
                .group
                .0
                .group
                .stream_send(self.token, self.stream_id, buf, fin)
                .would_block()?
            {
                Poll::Ready(send_size) => {
                    let mut state = self.group.0.state.lock();
                    state.wakers.get_mut(&self.token).unwrap().remove(&event);

                    Poll::Ready(Ok(send_size))
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }

    /// Receive data from this stream.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<(usize, bool)> {
        let event = Event::StreamRecv(self.stream_id);

        poll_fn(|cx| {
            log::trace!(
                "stream recv token={:?}, stream_id={}, remote={}",
                self.token,
                self.stream_id,
                self.remote
            );

            let mut state = self.group.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            if self.shutdown.load(Ordering::SeqCst) {
                return Poll::Ready(Ok((0, true)));
            }

            state
                .wakers
                .entry(self.token)
                .or_insert_with(|| Default::default())
                .insert(event, cx.waker().clone());

            drop(state);

            match self
                .group
                .0
                .group
                .stream_recv(self.token, self.stream_id, buf)
                .would_block()?
            {
                Poll::Ready((read_size, fin)) => {
                    let mut state = self.group.0.state.lock();
                    state.wakers.get_mut(&self.token).unwrap().remove(&event);

                    Poll::Ready(Ok((read_size, fin)))
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }

    fn shutdown(&self) -> Result<()> {
        let mut state = self.group.0.state.lock();

        log::trace!(
            "stream shutdown, token={:?}, stream_id={}, remote={}",
            self.token,
            self.stream_id,
            self.remote
        );
        self.shutdown.store(true, Ordering::SeqCst);

        self.group
            .wake(&mut state, self.token, Event::StreamSend(self.stream_id));

        self.group
            .wake(&mut state, self.token, Event::StreamRecv(self.stream_id));

        if state.stopped {
            return Err(Error::new(
                ErrorKind::BrokenPipe,
                "Stopped, backgroud group.",
            ));
        }

        drop(state);

        assert_eq!(
            self.group
                .0
                .group
                .stream_close(self.token, self.stream_id, 0x0)
                .would_block()?,
            Poll::Ready(())
        );

        Ok(())
    }
}

#[cfg(not(feature = "tokio"))]
mod futures {

    use super::*;
    use futures_io::{AsyncRead, AsyncWrite};

    impl AsyncRead for QuicStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize>> {
            Box::pin(self.recv(buf))
                .poll_unpin(cx)
                .map(|r| r.map(|(read_size, _)| read_size))
        }
    }

    impl AsyncWrite for QuicStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize>> {
            Box::pin(self.send(buf, false)).poll_unpin(cx)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<()>> {
            let state = self.group.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            drop(state);

            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<()>> {
            self.shutdown()?;
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncRead for &QuicStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize>> {
            Box::pin(self.recv(buf))
                .poll_unpin(cx)
                .map(|r| r.map(|(read_size, _)| read_size))
        }
    }

    impl AsyncWrite for &QuicStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize>> {
            Box::pin(self.send(buf, false)).poll_unpin(cx)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<()>> {
            let state = self.group.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            drop(state);

            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<()>> {
            self.shutdown()?;

            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(feature = "tokio")]
mod tokio_impl {

    use super::*;

    impl tokio::io::AsyncRead for QuicStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let read_size = match Box::pin(self.recv(buf.initialize_unfilled())).poll_unpin(cx) {
                Poll::Ready(Ok((read_size, _))) => read_size,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            };

            buf.advance(read_size);

            Poll::Ready(Ok(()))
        }
    }

    impl tokio::io::AsyncWrite for QuicStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<std::result::Result<usize, std::io::Error>> {
            Box::pin(self.send(buf, false)).poll_unpin(cx)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> Poll<std::result::Result<(), std::io::Error>> {
            let state = self.group.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            drop(state);

            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> Poll<std::result::Result<(), std::io::Error>> {
            self.shutdown()?;

            Poll::Ready(Ok(()))
        }
    }

    impl tokio::io::AsyncRead for &QuicStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let read_size = match Box::pin(self.recv(buf.initialize_unfilled())).poll_unpin(cx) {
                Poll::Ready(Ok((read_size, _))) => read_size,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            };

            buf.advance(read_size);

            Poll::Ready(Ok(()))
        }
    }

    impl tokio::io::AsyncWrite for &QuicStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<std::result::Result<usize, std::io::Error>> {
            Box::pin(self.send(buf, false)).poll_unpin(cx)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> Poll<std::result::Result<(), std::io::Error>> {
            let state = self.group.0.state.lock();

            if state.stopped {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "Stopped, backgroud group.",
                )));
            }

            drop(state);

            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> Poll<std::result::Result<(), std::io::Error>> {
            self.shutdown()?;

            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
mod server {
    use super::*;

    /// A **server-side** socket that accept inbound `QUIC` connections/streams.
    pub struct QuicListener(Group, bool);

    impl From<Group> for QuicListener {
        fn from(value: Group) -> Self {
            Self(value, false)
        }
    }

    impl Drop for QuicListener {
        fn drop(&mut self) {
            // own the group.
            if self.1 {
                self.0.stop();
            }
        }
    }

    impl QuicListener {
        /// Create `QuicListener` with private `Qroup` and background thread.
        pub fn bind<S>(laddrs: S, acceptor: Acceptor) -> Result<Self>
        where
            S: ToSocketAddrs,
        {
            let group = Group::bind(laddrs, Some(acceptor))?;

            let background = group.clone();

            std::thread::spawn(|| {
                if let Err(err) = background.run() {
                    log::error!("background `QUIC` group: {}", err);
                }
            });

            Ok(Self(group, true))
        }

        /// Fetch the bound addrs of this listener.
        #[inline]
        pub fn local_addrs(&self) -> impl Iterator<Item = &SocketAddr> {
            self.0.local_addrs()
        }

        /// Accepts a new incoming connection from this listener.
        #[inline]
        pub async fn accept(&self) -> Result<QuicConn> {
            poll_fn(|ctx| {
                let mut state = self.0.0.state.lock();

                if state.stopped {
                    return Poll::Ready(Err(Error::new(
                        ErrorKind::BrokenPipe,
                        "Stopped, backgroud group.",
                    )));
                }

                if let Some(token) = state.incoming_conns.pop_front() {
                    return Poll::Ready(Ok(QuicConn(self.0.clone(), token, false)));
                }

                state.accept = Some(ctx.waker().clone());

                Poll::Pending
            })
            .await
        }
    }
}

#[cfg(feature = "server")]
pub use server::*;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
mod client {
    use super::*;

    /// Connector for `QUIC` client-side.
    pub struct QuicConnector(Group);

    impl From<Group> for QuicConnector {
        fn from(value: Group) -> Self {
            Self(value)
        }
    }

    impl QuicConnector {
        /// Create a new `outbound` connection.
        pub fn create(
            &self,
            server_name: Option<&str>,
            local: SocketAddr,
            peer: SocketAddr,
            config: &mut quiche::Config,
        ) -> Result<QuicConn> {
            let token = self.0.0.group.connect(server_name, local, peer, config)?;

            Ok(QuicConn(self.0.clone(), token, false))
        }

        /// Establish a client-side connection asynchronously.
        pub async fn connect(
            &self,
            server_name: Option<&str>,
            local: SocketAddr,
            peer: SocketAddr,
            config: &mut quiche::Config,
        ) -> Result<QuicConn> {
            let conn = self.create(server_name, local, peer, config)?;

            conn.is_established().await?;

            Ok(conn)
        }
    }

    impl QuicConn {
        /// Establish a client-side connection with private group and background thread.
        pub async fn connect(
            server_name: Option<&str>,
            local: SocketAddr,
            peer: SocketAddr,
            config: &mut quiche::Config,
        ) -> Result<Self> {
            let group = Group::bind(local, None)?;

            let background = group.clone();

            std::thread::spawn(|| {
                if let Err(err) = background.run() {
                    log::error!("background `QUIC` group: {}", err);
                }
            });

            let local = group.local_addrs().next().unwrap().clone();

            let mut conn = QuicConnector::from(group).create(server_name, local, peer, config)?;

            // own the group.
            conn.2 = true;

            conn.is_established().await?;

            Ok(conn)
        }
    }
}

#[cfg(feature = "client")]
pub use client::*;
