use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    ops::DerefMut,
    time::{Duration, Instant},
};

use crossbeam_utils::sync::Unparker;
use parking_lot::{Mutex, RwLock};
use quiche::{ConnectionId, RecvInfo, SendInfo};

use crate::{
    Acceptor, QuicClient, QuicPoll, QuicServerTransport, QuicTransport,
    poll::{
        Error, Event, Readiness, Result, StreamKind, Token,
        conn::{LocKind, LockContext, QuicConn},
        utils::release_time,
    },
    utils::random_conn_id,
};

#[cfg(feature = "server")]
use crate::poll::Handshake;

static DEFAULT_RELEASE_TIMER_THRESHOLD: Duration = Duration::from_micros(250);

macro_rules! lock {
    ($self:ident, $token: ident, $kind: expr) => {{
        let state = $self.state.lock();

        let conn = state
            .conns
            .get(&$token)
            .ok_or_else(|| Error::NotFound)?
            .borrow_mut()
            .try_lock($kind, |ctx| $self.unlock(ctx))?;

        drop(state);

        conn
    }};
}

#[derive(Default)]
struct State {
    token_next: u32,
    conns: HashMap<Token, RefCell<QuicConn>>,
    readiness: RefCell<Readiness>,
    unparkers: HashMap<Token, Unparker>,
}

/// A group of `quiche:Connection`s.
pub struct Group {
    state: Mutex<State>,
    scids: RwLock<HashMap<ConnectionId<'static>, Token>>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            state: Default::default(),
            scids: Default::default(),
        }
    }
}

impl Group {
    /// Create a group with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    fn unlock(&self, ctx: LockContext) {
        let mut state = self.state.lock();

        if let Some(unparker) = state.unparkers.remove(&ctx.token) {
            unparker.unpark();
        }

        if state
            .conns
            .get(&ctx.token)
            .expect("Unlock.")
            .borrow_mut()
            .unlock(
                ctx.lock_count,
                ctx.send_done,
                state.readiness.borrow_mut().deref_mut(),
            )
        {
            log::trace!(
                "automatic deregister closed connection, token={:?}",
                ctx.token
            );

            drop(state);
            _ = self.deregister(ctx.token);
        }
    }

    /// Process a packet recv.
    /// Processes QUIC packets received from the peer.
    pub(crate) fn recv_(
        &self,
        scid: &ConnectionId<'_>,
        buf: &mut [u8],
        info: RecvInfo,
        unparker: Option<&Unparker>,
    ) -> Result<(Token, usize)> {
        let token = self
            .scids
            .read()
            .get(&scid)
            .ok_or_else(|| Error::NotFound)?
            .clone();

        let mut state = self.state.lock();

        let Ok(mut conn) = state
            .conns
            .get(&token)
            .ok_or_else(|| Error::NotFound)?
            .borrow_mut()
            .try_lock(LocKind::Recv, |ctx| self.unlock(ctx))
        else {
            // insert unparker.
            if let Some(unparker) = unparker {
                state.unparkers.insert(token, unparker.clone());
            }

            return Err(Error::Busy);
        };

        drop(state);

        match conn.recv(buf, info) {
            Ok(recv_size) => {
                log::trace!(
                    "Connection recv, scid={:?}, len={}",
                    conn.source_id(),
                    recv_size
                );

                Ok((token, recv_size))
            }
            Err(err) => {
                log::error!("Connection recv, scid={:?}, err={}", conn.source_id(), err);
                Err(Error::Quiche(err))
            }
        }
    }
}

impl QuicPoll for Group {
    type Error = crate::Error;
    /// Wrap and register a new `quiche::Connection`.
    #[inline]
    fn register(&self, wrapped: quiche::Connection) -> Result<Token> {
        let mut state = self.state.lock();

        loop {
            let token = Token(state.token_next);

            (state.token_next, _) = state.token_next.overflowing_add(1);

            if state.conns.contains_key(&token) {
                continue;
            }

            assert!(
                self.scids
                    .write()
                    .insert(wrapped.source_id().into_owned(), token)
                    .is_none()
            );

            log::trace!(
                "register quic connection, token={:?}, trace_id={}",
                token,
                wrapped.trace_id()
            );

            let conn = RefCell::new(QuicConn::new(token, wrapped));

            let guard = conn.borrow_mut().try_lock(LocKind::ReadLock, |context| {
                conn.borrow_mut().unlock(
                    context.lock_count,
                    false,
                    state.readiness.borrow_mut().deref_mut(),
                );
            })?;

            drop(guard);

            state.conns.insert(token, conn);

            return Ok(token);
        }
    }

    /// Unwrap a bound `quiche::Connection`
    #[inline]
    fn deregister(&self, token: Token) -> Result<quiche::Connection> {
        let mut state = self.state.lock();

        let conn: quiche::Connection = state
            .conns
            .remove(&token)
            .ok_or_else(|| Error::NotFound)?
            .into_inner()
            .into();

        drop(state);

        assert_eq!(
            self.scids.write().remove(&conn.source_id().into_owned()),
            Some(token)
        );

        Ok(conn)
    }

    /// Returns number of connections in the group.
    #[inline]
    fn len(&self) -> usize {
        self.state.lock().conns.len()
    }

    /// Close one connection.
    #[inline]
    fn close(&self, token: Token, app: bool, err: u64, reason: Cow<'static, [u8]>) -> Result<()> {
        let state = self.state.lock();
        let conn = state.conns.get(&token).ok_or_else(|| Error::NotFound)?;

        conn.borrow_mut()
            .close(app, err, reason, state.readiness.borrow_mut().deref_mut())
    }

    /// Open a outbound stream.
    #[inline]
    fn stream_open(
        &self,
        token: Token,
        kind: StreamKind,
        max_streams_as_error: bool,
    ) -> Result<u64> {
        let state = self.state.lock();
        let conn = state.conns.get(&token).ok_or_else(|| Error::NotFound)?;

        conn.borrow_mut().stream_open(
            kind,
            max_streams_as_error,
            state.readiness.borrow_mut().deref_mut(),
        )
    }

    /// Shutdown a stream.
    #[inline]
    fn stream_shutdown(&self, token: Token, stream_id: u64, err: u64) -> Result<()> {
        let state = self.state.lock();
        let conn = state.conns.get(&token).ok_or_else(|| Error::NotFound)?;

        conn.borrow_mut()
            .stream_close(stream_id, err, state.readiness.borrow_mut().deref_mut())
    }

    /// Writes data to a stream.
    #[inline]
    fn stream_send(&self, token: Token, stream_id: u64, buf: &[u8], fin: bool) -> Result<usize> {
        let mut conn = lock!(
            self,
            token,
            LocKind::StreamSend {
                id: stream_id,
                len: buf.len()
            }
        );

        match conn.stream_send(stream_id, buf, fin) {
            Ok(send_size) => {
                log::trace!(
                    "stream send, scid={:?}, stream_id={}, len={}, fin={}",
                    conn.source_id(),
                    stream_id,
                    send_size,
                    fin
                );
                return Ok(send_size);
            }
            Err(quiche::Error::Done) => {
                log::trace!(
                    "stream send, scid={:?}, stream_id={}, fin={}, Done",
                    conn.source_id(),
                    stream_id,
                    fin
                );
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!(
                    "stream send, scid={:?}, stream_id={}, fin={}, err={}",
                    conn.source_id(),
                    stream_id,
                    fin,
                    err
                );

                return Err(Error::Quiche(err));
            }
        }
    }

    /// Reads contiguous data from a stream into the provided slice.
    #[inline]
    fn stream_recv(&self, token: Token, stream_id: u64, buf: &mut [u8]) -> Result<(usize, bool)> {
        let mut conn = lock!(self, token, LocKind::StreamRecv(stream_id));

        match conn.stream_recv(stream_id, buf) {
            Ok((recv_size, fin)) => {
                log::trace!(
                    "stream recv, scid={:?}, stream_id={}, len={}, fin={}, is_server={}",
                    conn.source_id(),
                    stream_id,
                    recv_size,
                    fin,
                    conn.is_server(),
                );
                return Ok((recv_size, fin));
            }
            Err(quiche::Error::Done) => {
                if conn.stream_finished(stream_id) {
                    log::trace!(
                        "stream recv, scid={:?}, stream_id={}, len={}, fin={}, is_server={}",
                        conn.source_id(),
                        stream_id,
                        0,
                        true,
                        conn.is_server(),
                    );

                    return Ok((0, true));
                }

                log::trace!(
                    "stream recv, scid={:?}, stream_id={}, is_server={}, Done",
                    conn.source_id(),
                    stream_id,
                    conn.is_server(),
                );
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!(
                    "stream recv, scid={:?}, stream_id={}, is_server={}, err={}",
                    conn.source_id(),
                    stream_id,
                    conn.is_server(),
                    err
                );

                return Err(Error::Quiche(err));
            }
        }
    }

    /// Waits for readiness events without blocking current thread and returns possible retry time duration.
    #[inline]
    fn poll(&self, events: &mut Vec<Event>) -> Result<Option<Instant>> {
        let state = self.state.lock();

        Ok(state
            .readiness
            .borrow_mut()
            .poll(events, DEFAULT_RELEASE_TIMER_THRESHOLD))
    }
}

impl QuicTransport for Group {
    type Error = crate::Error;
    /// Processes QUIC packets received from the peer.
    #[inline]
    fn recv(&self, buf: &mut [u8], info: RecvInfo) -> Result<usize> {
        let header =
            quiche::Header::from_slice(buf, quiche::MAX_CONN_ID_LEN).map_err(Error::Quiche)?;

        self.recv_(&header.dcid, buf, info, None)
            .map(|(_, recv_size)| recv_size)
    }

    /// Writes a single QUIC packet to be sent to the peer.
    #[inline]
    fn send(&self, token: Token, buf: &mut [u8]) -> Result<(usize, SendInfo)> {
        let mut conn = lock!(self, token, LocKind::Recv);

        if let Some(release_time) =
            release_time(&conn, Instant::now(), DEFAULT_RELEASE_TIMER_THRESHOLD)
        {
            log::trace!(
                "connection send, scid={:?}, next_release_time={:?}",
                conn.trace_id(),
                release_time,
            );
            return Err(Error::Retry);
        }

        // TODO: prevent frequent calls to on_timeout
        conn.on_timeout();

        match conn.send(buf) {
            Ok((send_size, send_info)) => {
                log::trace!(
                    "connection send, scid={:?}, send_size={}, send_info={:?}",
                    conn.trace_id(),
                    send_size,
                    send_info
                );
                return Ok((send_size, send_info));
            }
            Err(quiche::Error::Done) => {
                log::trace!("connection send, scid={:?}, done", conn.trace_id());
                conn.send_done();
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!("connection send, scid={:?}, err={}", conn.trace_id(), err);
                return Err(Error::Quiche(err));
            }
        }
    }
}

impl QuicServerTransport for Group {
    fn recv_with_acceptor(
        &self,
        acceptor: &mut Acceptor,
        buf: &mut [u8],
        recv_size: usize,
        recv_info: RecvInfo,
        unparker: Option<&Unparker>,
    ) -> Result<(usize, SendInfo)> {
        let header = quiche::Header::from_slice(&mut buf[..recv_size], quiche::MAX_CONN_ID_LEN)
            .map_err(Error::Quiche)?;

        match self.recv_(&header.dcid, &mut buf[..recv_size], recv_info, unparker) {
            Ok((token, _)) => match self.send(token, buf) {
                Err(Error::Busy) | Err(Error::Retry) => Ok((
                    0,
                    SendInfo {
                        at: Instant::now(),
                        from: recv_info.to,
                        to: recv_info.from,
                    },
                )),
                r => r,
            },
            Err(Error::NotFound) => match acceptor.handshake(&header, buf, recv_size, recv_info) {
                Ok(Handshake::Accept(conn)) => {
                    let token = self.register(conn)?;

                    // Newly registered connections should be idle.
                    match self.recv_(&header.dcid, &mut buf[..recv_size], recv_info, None) {
                        Ok(_) => {}
                        Err(Error::Busy) | Err(Error::Retry) => {
                            unreachable!("Newly registered connections should be idle");
                        }
                        Err(err) => return Err(err),
                    }

                    match self.send(token, buf) {
                        Err(Error::Busy) | Err(Error::Retry) => Ok((
                            0,
                            SendInfo {
                                at: Instant::now(),
                                from: recv_info.to,
                                to: recv_info.from,
                            },
                        )),
                        r => r,
                    }
                }
                Ok(Handshake::Handshake(send_size)) => Ok((
                    send_size,
                    SendInfo {
                        at: Instant::now(),
                        from: recv_info.to,
                        to: recv_info.from,
                    },
                )),
                Err(err) => Err(err),
            },
            Err(err) => Err(err),
        }
    }
}

impl QuicClient for Group {
    type Error = crate::Error;
    fn connect(
        &self,
        server_name: Option<&str>,
        local: std::net::SocketAddr,
        peer: std::net::SocketAddr,
        config: &mut quiche::Config,
    ) -> Result<Token> {
        let conn = quiche::connect(server_name, &random_conn_id(), local, peer, config)?;

        let token = self.register(conn)?;

        Ok(token)
    }
}
