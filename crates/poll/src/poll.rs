//! Poll on masive quiche connecitons.

use std::{
    cell::UnsafeCell,
    collections::HashMap,
    io::{Error, ErrorKind, Result},
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

use quiche::{ConnectionId, RecvInfo, SendInfo, Shutdown};

/// Non-blocking socket for quic connection.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct QuicConn(u64);

/// Non-blocking socket for quic stream.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct QuicStream(u64, u64);

/// Readiness event returns by `Poll`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum Event {
    Send(QuicConn),
    Recv(QuicConn),
    Close(QuicConn),
    Closed(QuicConn),
    Established(QuicConn),
    StreamShutdown(QuicStream),
    StreamSend(QuicStream),
    StreamRecv(QuicStream),
    StreamOpen(QuicConn),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum LockEvent {
    None,
    Send,
    Recv,
    Close,
    StreamOpen,
    StreamShutdown(u64),
    StreamSend(u64),
    StreamRecv(u64),
}

type RcConn = Rc<UnsafeCell<quiche::Connection>>;

struct ConnState {
    #[allow(unused)]
    id: u64,
    quiche_conn: RcConn,
    locked: LockEvent,
    lock_count: u64,
    outbound_bidi_stream_id_next: u64,
    lock_requests: Vec<LockEvent>,
}

/// Release the `quiche::Connection`
impl From<ConnState> for quiche::Connection {
    fn from(value: ConnState) -> Self {
        Rc::try_unwrap(value.quiche_conn).unwrap().into_inner()
    }
}

impl ConnState {
    fn new(id: u64, conn: quiche::Connection) -> Self {
        // `0` and `1` are special-purpose stream ids
        let outbound_bidi_stream_id_next = if conn.is_server() { 5 } else { 4 };

        Self {
            id,
            quiche_conn: Rc::new(UnsafeCell::new(conn)),
            locked: LockEvent::None,
            outbound_bidi_stream_id_next,
            lock_count: 0,
            lock_requests: Default::default(),
        }
    }

    #[inline(always)]
    fn as_mut(&mut self) -> &mut quiche::Connection {
        unsafe { self.quiche_conn.get().as_mut().unwrap() }
    }

    #[inline(always)]
    fn as_ref(&mut self) -> &quiche::Connection {
        unsafe { self.quiche_conn.get().as_ref().unwrap() }
    }

    /// Try lock this connection state.
    #[inline(always)]
    fn try_lock(&mut self, event: LockEvent) -> Result<(u64, RcConn)> {
        assert_ne!(event, LockEvent::None);

        if self.locked != LockEvent::None {
            self.lock_requests.push(event);
            return Err(Error::new(
                ErrorKind::WouldBlock,
                format!("try lock: {:?}", event),
            ));
        }

        self.locked = event;

        return Ok((self.lock_count, self.quiche_conn.clone()));
    }

    #[inline(always)]
    fn unlock<F>(&mut self, lock_count: u64, mut f: F)
    where
        F: FnMut(Event),
    {
        assert_ne!(self.locked, LockEvent::None, "Unlock twice.");
        assert_eq!(lock_count, self.lock_count, "Lock count is mismatched.");

        let id = self.id;

        let send_event = match self.locked {
            LockEvent::None => {
                unreachable!("")
            }
            LockEvent::Send => false,
            _ => {
                f(Event::Send(QuicConn(id)));
                true
            }
        };

        self.locked = LockEvent::None;
        self.lock_count += 1;

        for lock_event in self.lock_requests.drain(..).collect::<Vec<_>>() {
            match lock_event {
                LockEvent::Send => {
                    if !send_event {
                        f(Event::Send(QuicConn(id)))
                    }
                }
                LockEvent::Recv => f(Event::Recv(QuicConn(id))),
                LockEvent::Close => f(Event::Close(QuicConn(id))),
                LockEvent::StreamOpen => {
                    if self.as_ref().peer_streams_left_bidi() > 0 {
                        f(Event::StreamOpen(QuicConn(id)))
                    } else {
                        self.lock_requests.push(LockEvent::StreamOpen);
                    }
                }
                LockEvent::StreamShutdown(stream_id) => {
                    f(Event::StreamShutdown(QuicStream(id, stream_id)))
                }
                LockEvent::StreamSend(stream_id) => f(Event::StreamSend(QuicStream(id, stream_id))),
                LockEvent::StreamRecv(stream_id) => f(Event::StreamRecv(QuicStream(id, stream_id))),
                LockEvent::None => {}
            }
        }

        let conn = self.as_mut();

        while let Some(stream_id) = conn.stream_readable_next() {
            f(Event::StreamRecv(QuicStream(id, stream_id)))
        }

        while let Some(stream_id) = conn.stream_writable_next() {
            f(Event::StreamRecv(QuicStream(id, stream_id)))
        }
    }
}

struct ConnGuard<'a> {
    id: u64,
    lock_count: u64,
    quiche_conn: RcConn,
    poll: &'a Poll,
}

impl<'a> Deref for ConnGuard<'a> {
    type Target = quiche::Connection;

    fn deref(&self) -> &Self::Target {
        unsafe { self.quiche_conn.get().as_ref().unwrap() }
    }
}

impl<'a> DerefMut for ConnGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.quiche_conn.get().as_mut().unwrap() }
    }
}

impl<'a> Drop for ConnGuard<'a> {
    fn drop(&mut self) {
        self.poll.lock_guard(|state| {
            let conn_stat = state
                .conns
                .get_mut(&self.id)
                .expect("Drop: conn is not found.");

            conn_stat.unlock(self.lock_count, |event| {
                self.poll.readiness_event(event);
            });
        });
    }
}

#[derive(Default)]
struct PollState {
    /// Id generator for new register quiche connection.
    conn_id_next: u64,
    /// managed connection set.
    conns: HashMap<u64, ConnState>,
    /// source id set.
    source_id_set: HashMap<ConnectionId<'static>, u64>,
}

/// Polls for readiness events on all registered `quiche` connections.
#[derive(Default)]
pub struct Poll {
    /// Sync guard for this poll.
    spin: AtomicBool,
    /// inner state protected by spin-locker
    state: UnsafeCell<PollState>,
    /// readiness events.
    readiness_events: UnsafeCell<Vec<Event>>,
}

impl Poll {
    fn readiness_event(&self, _event: Event) {
        assert_eq!(self.spin.load(Ordering::Relaxed), true);
        //TODO: filter and save event.
    }

    fn lock_guard<F, O>(&self, f: F) -> O
    where
        F: FnOnce(&mut PollState) -> O,
    {
        while self
            .spin
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // No need to worry about dead cycles taking up cpu time,
            // all internal locking operations will be done in a few
            // microseconds.
        }

        let o = f(unsafe { self.state.get().as_mut().unwrap() });

        self.spin.store(false, Ordering::Release);

        o
    }

    /// Consume the provided quiche connection and generate a non-blocking poll-aware socket.
    pub fn register(&self, conn: quiche::Connection) -> Result<QuicConn> {
        let source_id = conn.source_id().into_owned();

        // Minimise the mutex region.
        let id = self.lock_guard(|state| {
            let id = state.conn_id_next;
            let conn_state = ConnState::new(id, conn);
            state.conns.insert(id, conn_state);
            state.source_id_set.insert(source_id, id);
            id
        });

        Ok(QuicConn(id))
    }

    /// Release the non-blocking socket previously created by `register`
    /// and returns the `quiche::Connection` it points to.
    ///
    /// It is the application's responsibility to call this function as soon as possible
    /// after a connection closes or an error occurs to reclaim resources.
    pub fn deregister(&self, socket: QuicConn) -> Result<quiche::Connection> {
        self.lock_guard(|state| {
            let conn = state.conns.remove(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let conn = quiche::Connection::from(conn);

            let source_id = conn.source_id().into_owned();

            assert!(
                state.source_id_set.remove(&source_id).is_some(),
                "source_id_set: is null({:?})",
                source_id
            );

            Ok(conn)
        })
    }

    /// Poll readiness events.
    pub fn poll(&self, events: &mut Vec<Event>) -> Result<()> {
        self.lock_guard(|_| {
            std::mem::swap(
                unsafe { self.readiness_events.get().as_mut().unwrap() },
                events,
            );
        });

        Ok(())
    }

    /// Writes a single QUIC packet to be sent to the peer.
    pub fn send(&self, socket: QuicConn, buf: &mut [u8]) -> Result<(usize, SendInfo)> {
        let mut conn = self.lock_guard(|state| -> Result<_> {
            let conn_stat = state.conns.get_mut(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let (lock_count, conn) = conn_stat.try_lock(LockEvent::Send)?;

            Ok(ConnGuard {
                id: socket.0,
                lock_count,
                quiche_conn: conn,
                poll: self,
            })
        })?;

        match conn.send(buf) {
            Ok((send_size, send_info)) => {
                log::trace!(
                    "conn send, scid={:?}, len={}, send_info={:?}",
                    conn.source_id(),
                    send_size,
                    send_info
                );
                Ok((send_size, send_info))
            }
            Err(quiche::Error::Done) => {
                // check if this connection is closed.
                if conn.is_closed() {
                    log::info!("conn is closed, scid={:?}", conn.source_id());

                    return Err(Error::new(
                        ErrorKind::BrokenPipe,
                        format!("{:?} is closed", socket),
                    ));
                }

                log::trace!("conn has no packets to send, scid={:?}", conn.source_id());

                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    format!("{:?} would block: send", socket),
                ));
            }
            Err(err) => {
                log::error!("conn send, scid={:?}, err={}", conn.source_id(), err);
                Err(Error::other(err))
            }
        }
    }

    /// Processes QUIC packets received from the peer.
    pub fn recv(
        &self,
        dcid: &ConnectionId<'_>,
        buf: &mut [u8],
        recv_info: RecvInfo,
    ) -> Result<usize> {
        let mut conn = self.lock_guard(|state| -> Result<_> {
            let id = state.source_id_set.get(&dcid).cloned().ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    format!("scid={:?} is not found.", dcid),
                )
            })?;

            let conn_stat = state.conns.get_mut(&id).expect("find by source id.");

            let (lock_count, conn) = conn_stat.try_lock(LockEvent::Recv)?;

            Ok(ConnGuard {
                id,
                lock_count,
                quiche_conn: conn,
                poll: self,
            })
        })?;

        match conn.recv(buf, recv_info) {
            Ok(recv_size) => {
                log::trace!("conn recv, scid={:?}, len={}", conn.source_id(), recv_size);
                Ok(recv_size)
            }
            Err(err) => {
                log::error!("conn recv, scid={:?}, err={}", conn.source_id(), err);
                Err(Error::other(err))
            }
        }
    }

    /// Closes the connection with the given error and reason.
    pub fn close(&self, socket: QuicConn, app: bool, err: u64, reason: &[u8]) -> Result<()> {
        let mut conn = self.lock_guard(|state| -> Result<_> {
            let conn_stat = state.conns.get_mut(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let (lock_count, conn) = conn_stat.try_lock(LockEvent::Close)?;

            Ok(ConnGuard {
                id: socket.0,
                lock_count,
                quiche_conn: conn,
                poll: self,
            })
        })?;

        match conn.close(app, err, reason) {
            Ok(_) => {
                log::trace!(
                    "close conn, scid={:?}, app={}, err={}, reason={:?}",
                    conn.source_id(),
                    app,
                    err,
                    reason
                );
                Ok(())
            }
            Err(quiche::Error::Done) => {
                log::warn!("close conn twice, scid={:?}", conn.source_id(),);
                Ok(())
            }
            Err(err) => {
                log::error!(
                    "failed to close conn, scid={:?}, err={}",
                    conn.source_id(),
                    err
                );
                Err(Error::other(err))
            }
        }
    }

    /// Writes data to a stream.
    ///
    /// On success the number of bytes written is returned
    pub fn stream_send(&self, socket: QuicStream, buf: &[u8], fin: bool) -> Result<usize> {
        let mut conn = self.lock_guard(|state| -> Result<_> {
            let conn_stat = state.conns.get_mut(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let (lock_count, conn) = conn_stat.try_lock(LockEvent::StreamSend(socket.1))?;

            Ok(ConnGuard {
                id: socket.0,
                lock_count,
                quiche_conn: conn,
                poll: self,
            })
        })?;

        match conn.stream_send(socket.1, buf, fin) {
            Ok(send_size) => {
                log::trace!(
                    "stream send, scid={:?}, id={}, len={}, fin={}",
                    conn.source_id(),
                    socket.1,
                    send_size,
                    fin
                );
                Ok(send_size)
            }
            Err(quiche::Error::Done) => {
                log::trace!(
                    "stream has no capacity, scid={:?}, id={}, len={}, fin={}",
                    conn.source_id(),
                    socket.1,
                    buf.len(),
                    fin
                );

                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    format!("{:?} no capacity.", socket),
                ));
            }
            Err(err) => {
                log::error!(
                    "stream send, scid={:?}, id={}, fin={}, err={}",
                    conn.source_id(),
                    socket.1,
                    fin,
                    err
                );
                Err(Error::other(err))
            }
        }
    }

    /// Reads contiguous data from a stream into the provided slice.
    pub fn stream_recv(&self, socket: QuicStream, buf: &mut [u8]) -> Result<(usize, bool)> {
        let mut conn = self.lock_guard(|state| -> Result<_> {
            let conn_stat = state.conns.get_mut(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let (lock_count, conn) = conn_stat.try_lock(LockEvent::StreamRecv(socket.1))?;

            Ok(ConnGuard {
                id: socket.0,
                lock_count,
                quiche_conn: conn,
                poll: self,
            })
        })?;

        match conn.stream_recv(socket.1, buf) {
            Ok((recv_size, fin)) => {
                log::trace!(
                    "stream recv, scid={:?}, id={}, len={}, fin={}",
                    conn.source_id(),
                    socket.1,
                    recv_size,
                    fin
                );

                Ok((recv_size, fin))
            }
            Err(err) => {
                log::error!(
                    "stream recv, scid={:?}, id={}, err={}",
                    conn.source_id(),
                    socket.1,
                    err
                );

                Err(Error::other(err))
            }
        }
    }

    /// Open a new outbound bidirectional stream, if possiable.
    pub fn stream_open(&self, socket: QuicConn) -> Result<QuicStream> {
        self.lock_guard(|state| -> Result<_> {
            let conn_stat = state.conns.get_mut(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let (lock_count, _) = conn_stat.try_lock(LockEvent::StreamOpen)?;

            if conn_stat.as_ref().peer_streams_left_bidi() > 0 {
                let stream_id = conn_stat.outbound_bidi_stream_id_next;
                conn_stat.outbound_bidi_stream_id_next += 4;

                // this a trick, func `stream_priority` will created the target if did not exist.

                conn_stat
                    .as_mut()
                    .stream_priority(stream_id, 255, true)
                    .map_err(|err| Error::other(err))?;

                let conn = conn_stat.as_ref();

                log::trace!(
                    "open a new stream, scid={:?}, id={}, left_bidi={}",
                    conn.source_id(),
                    stream_id,
                    conn.peer_streams_left_bidi()
                );

                conn_stat.unlock(lock_count, |event| {
                    self.readiness_event(event);
                });

                Ok(QuicStream(socket.0, stream_id))
            } else {
                log::warn!(
                    "stream open is failed, scid={:?}, left_bidi=0",
                    conn_stat.as_ref().source_id()
                );

                conn_stat.lock_requests.push(LockEvent::StreamOpen);

                conn_stat.unlock(lock_count, |event| {
                    self.readiness_event(event);
                });

                Err(Error::new(
                    ErrorKind::WouldBlock,
                    "peer streams left is `0`",
                ))
            }
        })
    }

    /// Shuts down reading or writing from/to the specified stream.
    pub fn stream_shutdown(&self, socket: QuicStream, direction: Shutdown, err: u64) -> Result<()> {
        let mut conn = self.lock_guard(|state| -> Result<_> {
            let conn_stat = state.conns.get_mut(&socket.0).ok_or_else(|| {
                Error::new(ErrorKind::NotFound, format!("{:?} is not found.", socket))
            })?;

            let (lock_count, conn) = conn_stat.try_lock(LockEvent::StreamShutdown(socket.1))?;

            Ok(ConnGuard {
                id: socket.0,
                lock_count,
                quiche_conn: conn,
                poll: self,
            })
        })?;

        match conn.stream_shutdown(socket.1, direction, err) {
            Ok(_) => {
                log::trace!(
                    "shutdown stream, scid={:?}, id={}",
                    conn.source_id(),
                    socket.1
                );
                Ok(())
            }
            Err(err) => {
                log::error!(
                    "shutdown stream, scid={:?}, id={}, err={}",
                    conn.source_id(),
                    socket.1,
                    err
                );

                Err(Error::other(err))
            }
        }
    }
}
