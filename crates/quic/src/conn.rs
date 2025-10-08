//! Wrapper of [`quiche::Connection`] used by `Poller`.

use std::{
    borrow::Cow,
    cell::UnsafeCell,
    collections::HashSet,
    fmt::Debug,
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

use quiche::Shutdown;

use crate::{
    Error, Event, EventKind, Readiness, Result, StreamKind, Token,
    utils::{delay_send, is_bidi, is_local},
};

/// Lock type of one `QuicConn`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum LocKind {
    None,
    Send,
    Recv,
    Close {
        app: bool,
        err: u64,
        reason: Cow<'static, [u8]>,
    },
    StreamOpen(StreamKind),
    StreamSend {
        id: u64,
        len: usize,
    },
    StreamRecv(u64),
    StreamClose {
        id: u64,
        err: u64,
    },
    ReadLock,
}

impl Default for LocKind {
    fn default() -> Self {
        Self::None
    }
}

impl LocKind {
    fn need_retry(&self) -> bool {
        match self {
            LocKind::Send | LocKind::None => false,
            _ => true,
        }
    }
}

/// Lock context for `QuicConnGuard`
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct LockContext {
    pub token: Token,
    pub lock_count: u64,
    pub send_done: bool,
}

/// An RAII implementation of a “scoped connection” of a `QuicConn`.
pub struct QuicConnGuard<F>
where
    F: FnOnce(LockContext),
{
    lock: LockContext,
    drop: Option<F>,
    wrapped: &'static mut quiche::Connection,
}

impl<F> Debug for QuicConnGuard<F>
where
    F: FnOnce(LockContext),
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicConnGuard").finish_non_exhaustive()
    }
}

impl<F> Drop for QuicConnGuard<F>
where
    F: FnOnce(LockContext),
{
    fn drop(&mut self) {
        self.drop.take().expect("Drop")(self.lock);
    }
}

impl<F> Deref for QuicConnGuard<F>
where
    F: FnOnce(LockContext),
{
    type Target = quiche::Connection;

    fn deref(&self) -> &Self::Target {
        self.wrapped
    }
}

impl<F> DerefMut for QuicConnGuard<F>
where
    F: FnOnce(LockContext),
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.wrapped
    }
}

impl<F> QuicConnGuard<F>
where
    F: FnOnce(LockContext),
{
    pub fn send_done(&mut self) {
        self.lock.send_done = true;
    }
}

/// Quic connection object assoicated with a `Poller`.
pub struct QuicConn {
    /// poller id.
    token: Token,
    /// The generator for next local opening bidirectional stream.
    local_stream_bidi_next: u64,
    /// Maximum bidirectional stream id seen.
    remote_stream_bidi_seen: u64,
    /// The generator for next local opening unidirectional stream.
    local_stream_uni_next: u64,
    /// Maximum unidirectional stream id seen.
    remote_stream_uni_seen: u64,
    /// lock kind.
    locker: LocKind,
    /// retry ops.
    reties: HashSet<LocKind>,
    /// Counter for locking operations, used by `unlock` fn.
    lock_count: u64,
    /// read lock count.
    read_lock_count: usize,
    /// established status.
    established: bool,
    /// ...
    release_timer_threshold: Duration,
    /// wrapped `quiche::Connection`.
    wrapped: UnsafeCell<quiche::Connection>,
}

/// unwrap `quiche::Connection` from `ConnState`
impl From<QuicConn> for quiche::Connection {
    fn from(value: QuicConn) -> Self {
        value.wrapped.into_inner()
    }
}

impl QuicConn {
    pub fn new(
        token: Token,
        wrapped: quiche::Connection,
        release_timer_threshold: Duration,
    ) -> Self {
        Self {
            token,
            local_stream_bidi_next: if wrapped.is_server() { 0x01 } else { 0x0 },
            remote_stream_bidi_seen: Default::default(),
            local_stream_uni_next: if wrapped.is_server() { 0x03 } else { 0x2 },
            remote_stream_uni_seen: Default::default(),
            locker: Default::default(),
            reties: Default::default(),
            lock_count: Default::default(),
            read_lock_count: 0,
            release_timer_threshold,
            established: wrapped.is_established(),
            wrapped: UnsafeCell::new(wrapped),
        }
    }

    /// Try lock this connection.
    pub fn try_lock<F>(&mut self, kind: LocKind, drop: F) -> Result<QuicConnGuard<F>>
    where
        F: FnOnce(LockContext),
    {
        match kind {
            LocKind::None => {
                if kind == LocKind::ReadLock {
                    assert_eq!(self.read_lock_count, 0);
                    self.read_lock_count += 1;
                }
            }
            LocKind::ReadLock if kind == LocKind::ReadLock => {
                assert!(self.read_lock_count > 0);
                self.read_lock_count += 1;
            }
            _ => {
                if kind.need_retry() {
                    self.reties.insert(kind);
                }
                return Err(Error::Busy);
            }
        }

        Ok(QuicConnGuard {
            lock: LockContext {
                token: self.token,
                lock_count: self.lock_count,
                send_done: false,
            },
            drop: Some(drop),
            wrapped: unsafe { self.wrapped.get().as_mut().unwrap() },
        })
    }

    /// Unlock this connection.
    pub fn unlock(&mut self, lock_count: u64, send_done: bool, readiness: &mut Readiness) {
        assert!(self.locker != LocKind::None);
        assert_eq!(self.lock_count, lock_count);

        if self.locker == LocKind::ReadLock {
            self.read_lock_count -= 1;
            /// all read lock are released.
            if self.read_lock_count == 0 {
                self.lock_count += 1;
                self.locker = LocKind::None;
            }
        } else {
            self.lock_count += 1;
            self.locker = LocKind::None;
        }

        let conn = unsafe { self.wrapped.get().as_mut().unwrap() };

        self.handle_retry_events(conn, readiness);
        self.handle_stream_state_changed(conn, readiness);
        self.handle_send(conn, send_done, readiness);
        self.handle_state_changed(conn, readiness);
    }

    /// Close this connection.
    pub fn close<F>(
        &mut self,
        app: bool,
        err: u64,
        reason: Cow<'static, [u8]>,
        drop: F,
    ) -> Result<()>
    where
        F: FnOnce(LockContext),
    {
        let guard = self.try_lock(
            LocKind::Close {
                app,
                err,
                reason: reason.clone(),
            },
            drop,
        )?;

        /// safety: state is protected by previous level locker.
        let conn = unsafe { self.wrapped.get().as_mut().unwrap() };

        if let Err(err) = conn.close(app, err, &reason) {
            if err != quiche::Error::Done {
                return Err(err.into());
            }
        }

        Ok(())
    }

    /// Shutdown specific stream's read/write.
    pub fn stream_open<F>(&mut self, kind: StreamKind, drop: F) -> Result<u64>
    where
        F: FnOnce(LockContext),
    {
        let guard = self.try_lock(LocKind::StreamOpen(kind), drop)?;

        match self.stream_open_prv(
            /// safety: state is protected by previous level locker.
            unsafe {
                self.wrapped.get().as_mut().unwrap()
            },
            kind,
        ) {
            Ok(stream_id) => return Ok(stream_id),
            Err(Error::Retry) => {}
            Err(err) => return Err(err),
        }

        self.reties.insert(LocKind::StreamOpen(kind));

        Err(Error::Retry)
    }

    /// Shutdown specific stream's read/write.
    pub fn stream_shutdown<F>(&mut self, stream_id: u64, err: u64, drop: F) -> Result<()>
    where
        F: FnOnce(LockContext),
    {
        let kind = LocKind::StreamClose { id: stream_id, err };

        let guard = self.try_lock(kind, drop)?;

        self.stream_shutdown_prv(
            /// safety: state is protected by previous level locker.
            unsafe {
                self.wrapped.get().as_mut().unwrap()
            },
            stream_id,
            err,
        )
    }

    fn stream_shutdown_prv(
        &mut self,
        conn: &mut quiche::Connection,
        stream_id: u64,
        err: u64,
    ) -> Result<()> {
        if let Err(err) = conn.stream_shutdown(stream_id, Shutdown::Read, err) {
            if err != quiche::Error::Done {
                return Err(err.into());
            }
        }

        if let Err(err) = conn.stream_shutdown(stream_id, Shutdown::Write, err) {
            if err != quiche::Error::Done {
                return Err(err.into());
            }
        }

        Ok(())
    }

    fn stream_open_prv(&mut self, conn: &mut quiche::Connection, kind: StreamKind) -> Result<u64> {
        let stream_id = match kind {
            StreamKind::Uni => {
                if conn.peer_streams_left_uni() == 0 {
                    return Err(Error::Retry);
                }

                let stream_id = self.local_stream_uni_next;
                self.local_stream_uni_next += 4;
                stream_id
            }
            StreamKind::Bidi => {
                if conn.peer_streams_left_bidi() == 0 {
                    return Err(Error::Retry);
                }

                let stream_id = self.local_stream_bidi_next;
                self.local_stream_bidi_next += 4;
                stream_id
            }
        };

        // this a trick, func `stream_priority` will created the target if did not exist.
        conn.stream_priority(stream_id, 255, true)?;

        log::trace!(
            "stream open, scid={:?}, kind={:?}, stream_id={}",
            conn.trace_id(),
            kind,
            stream_id
        );

        return Ok(stream_id);
    }

    fn handle_retry_events(&mut self, conn: &mut quiche::Connection, readiness: &mut Readiness) {
        for event in self.reties.drain().collect::<Vec<_>>() {
            match event {
                LocKind::Send => {}
                LocKind::Recv => {
                    readiness.insert(
                        Event {
                            token: self.token,
                            kind: EventKind::Recv,
                            is_server: conn.is_server(),
                            is_error: false,
                            stream_id: 0,
                        },
                        None,
                    );
                }
                LocKind::Close { app, err, reason } => {
                    if let Err(err) = conn.close(app, err, &reason) {
                        if err != quiche::Error::Done {
                            log::error!("close connection, id={}, err={}", conn.trace_id(), err);

                            readiness.insert(
                                Event {
                                    kind: EventKind::Closed,
                                    is_server: conn.is_server(),
                                    is_error: true,
                                    token: self.token,
                                    stream_id: 0,
                                },
                                None,
                            );
                        }
                    }
                }
                LocKind::StreamOpen(stream_kind) => match stream_kind {
                    StreamKind::Uni => {
                        if conn.peer_streams_left_uni() > 0 {
                            readiness.insert(
                                Event {
                                    token: self.token,
                                    kind: EventKind::StreamOpenUni,
                                    is_server: conn.is_server(),
                                    is_error: false,
                                    stream_id: 0,
                                },
                                None,
                            );
                        }
                    }
                    StreamKind::Bidi => {
                        if conn.peer_streams_left_uni() > 0 {
                            readiness.insert(
                                Event {
                                    token: self.token,
                                    kind: EventKind::StreamOpenBidi,
                                    is_server: conn.is_server(),
                                    is_error: false,
                                    stream_id: 0,
                                },
                                None,
                            );
                        }
                    }
                },
                LocKind::StreamSend { id, len } => {
                    // such that it is not going to be
                    // reported as writable again by [`stream_writable_next()`] until its send
                    // capacity reaches `len`.
                    match conn.stream_writable(id, len) {
                        Ok(writable) => {
                            if writable {
                                readiness.insert(
                                    Event {
                                        kind: EventKind::StreamSend,
                                        is_server: conn.is_server(),
                                        is_error: false,
                                        token: self.token,
                                        stream_id: id,
                                    },
                                    None,
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "failed to call `stream_writable`, scid={:?}, id={}, err={}",
                                conn.trace_id(),
                                id,
                                err
                            );

                            readiness.insert(
                                Event {
                                    kind: EventKind::StreamSend,
                                    is_server: conn.is_server(),
                                    is_error: true,
                                    token: self.token,
                                    stream_id: id,
                                },
                                None,
                            );
                        }
                    }
                }
                LocKind::StreamRecv(stream_id) => {
                    if conn.stream_readable(stream_id) {
                        readiness.insert(
                            Event {
                                kind: EventKind::StreamRecv,
                                is_server: conn.is_server(),

                                is_error: false,
                                token: self.token,
                                stream_id,
                            },
                            None,
                        );
                    }
                }
                LocKind::StreamClose { id, err } => {
                    if let Err(err) = self.stream_shutdown_prv(conn, id, err) {
                        log::error!(
                            "shutdown, scid={:?}, stream_id={}, err={}",
                            conn.source_id(),
                            id,
                            err
                        );
                    }
                }
                LocKind::ReadLock => {
                    readiness.insert(
                        Event {
                            kind: EventKind::ReadLock,
                            is_server: conn.is_server(),

                            is_error: false,
                            token: self.token,
                            stream_id: 0,
                        },
                        None,
                    );
                }
                _ => {
                    unreachable!("handle_retry_events: {:?}", event);
                }
            }
        }
    }

    fn handle_state_changed(&mut self, conn: &mut quiche::Connection, readiness: &mut Readiness) {
        if !self.established && conn.is_established() {
            self.established = true;
            readiness.insert(
                Event {
                    token: self.token,
                    kind: EventKind::Connected,
                    is_server: conn.is_server(),
                    is_error: false,
                    stream_id: 0,
                },
                None,
            );
        }

        if conn.is_closed() {
            readiness.insert(
                Event {
                    token: self.token,
                    kind: EventKind::Closed,
                    is_server: conn.is_server(),
                    is_error: false,
                    stream_id: 0,
                },
                None,
            );
        }
    }

    fn handle_stream_state_changed(
        &mut self,
        conn: &mut quiche::Connection,
        readiness: &mut Readiness,
    ) {
        while let Some(stream_id) = conn.stream_writable_next() {
            readiness.insert(
                Event {
                    kind: EventKind::StreamSend,
                    is_server: conn.is_server(),
                    is_error: false,
                    token: self.token,
                    stream_id,
                },
                None,
            );
        }

        while let Some(stream_id) = conn.stream_readable_next() {
            if !is_local(stream_id, conn.is_server()) && self.remote_stream_bidi_seen < stream_id {
                if is_bidi(stream_id) {
                    if self.remote_stream_bidi_seen < stream_id {
                        self.remote_stream_bidi_seen = stream_id;

                        readiness.insert(
                            Event {
                                kind: EventKind::StreamAccept,
                                is_server: conn.is_server(),
                                is_error: false,
                                token: self.token,
                                stream_id,
                            },
                            None,
                        );
                    }
                } else {
                    if self.remote_stream_uni_seen < stream_id {
                        self.remote_stream_uni_seen = stream_id;

                        readiness.insert(
                            Event {
                                kind: EventKind::StreamAccept,
                                is_server: conn.is_server(),
                                is_error: false,
                                token: self.token,
                                stream_id,
                            },
                            None,
                        );
                    }
                }
            }
        }
    }

    fn handle_send(
        &mut self,
        conn: &mut quiche::Connection,
        send_done: bool,
        readiness: &mut Readiness,
    ) {
        let now = Instant::now();

        // check if the connection has data to send.
        let delay_to = delay_send(conn, now, self.release_timer_threshold, send_done);

        if send_done && delay_to.is_none() {
            readiness.remove(Event {
                kind: EventKind::Send,
                is_server: conn.is_server(),
                is_error: false,
                token: self.token,
                stream_id: 0,
            });

            return;
        }

        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: conn.is_server(),
                is_error: false,
                token: self.token,
                stream_id: 0,
            },
            delay_to,
        );
    }
}
