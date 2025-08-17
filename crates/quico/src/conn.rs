use std::{
    cell::UnsafeCell,
    collections::HashSet,
    fmt::Debug,
    time::{Duration, Instant},
};

use quiche::{Connection, Shutdown};

use crate::{Error, Event, EventKind, Readiness, Result, Token, utils::release_time};

/// `ConnState` resource acquire kind.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum LocKind {
    None,
    Send,
    Recv,
    Close,
    StreamOpen,
    StreamSend(u64, usize),
    StreamRecv(u64),
    StreamShutdown {
        shutdown_read: bool,
        shutdown_write: bool,
        stream_id: u64,
        err: u64,
    },
}

impl LocKind {
    pub fn need_retry(&self) -> bool {
        match self {
            LocKind::None | LocKind::Send => false,
            _ => true,
        }
    }
}

/// Internal connection state.
pub struct ConnState {
    /// Connection id.
    id: Token,
    /// Wrapped `quiche::Connection`
    conn: UnsafeCell<quiche::Connection>,
    /// Current lock type.
    locked: LocKind,
    /// Requests that need to be retried for locking.
    retries: HashSet<LocKind>,
    /// The count of lock times used as lock tracking handle.
    lock_count: u64,
    /// Record the next locally opened bi-directional stream id
    local_bidi_stream_id_next: u64,
}

/// unwrap `quiche::Connection` from `ConnState`
impl From<ConnState> for quiche::Connection {
    fn from(value: ConnState) -> Self {
        value.conn.into_inner()
    }
}

/// A lock guard for `ConnState`
pub struct ConnGuard {
    pub lock_count: u64,
    pub conn: &'static mut Connection,
}

impl Debug for ConnGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnStateGuard")
            .field("lock_count", &self.lock_count)
            .finish()
    }
}

impl ConnState {
    /// Wrap a new `quiche::Connection`
    pub fn new(id: Token, conn: quiche::Connection) -> Self {
        Self {
            id,
            local_bidi_stream_id_next: if conn.is_server() { 5 } else { 4 },
            conn: UnsafeCell::new(conn),
            locked: LocKind::None,
            retries: Default::default(),
            lock_count: 0,
        }
    }

    #[inline]
    fn retry_later(&mut self, kind: LocKind) {
        self.retries.insert(kind);
    }

    /// Try lock this `state`.
    ///
    /// Safety:
    /// - This function is protected by an upper-level spin-lock.
    /// - Only one specific thread own the returns `&'static mut Connection` at the same time.
    pub fn try_lock(&mut self, kind: LocKind) -> Result<ConnGuard> {
        assert_ne!(kind, LocKind::None, "LocKind is None.");
        // The resource is busy.
        if self.locked != LocKind::None {
            // retry this operation later.
            if kind.need_retry() {
                self.retry_later(kind);
            }

            return Err(Error::Busy);
        }

        // Successfully locked this state.
        self.locked = kind;

        // Upper level code needs to be careful to save the trace handle, which is needed to call `unlock`.
        Ok(ConnGuard {
            lock_count: self.lock_count,
            conn: unsafe { self.conn.get().as_mut().unwrap() },
        })
    }

    /// Unlock this state with `lock_count` returned by `try_lock`.
    ///
    /// Returns when the next timeout event will occur.
    ///
    /// Safety:
    /// - This function is protected by an upper-level spin-lock.
    pub fn unlock(
        &mut self,
        lock_count: u64,
        release_timer_threshold: Duration,
        readiness: &mut Readiness,
    ) {
        assert_ne!(self.locked, LocKind::None, "Unlock a released stat.");
        assert_eq!(self.lock_count, lock_count, "`lock_count` is mismatched.");

        self.locked = LocKind::None;
        // step `lock_count`
        self.lock_count += 1;

        // Safety:
        // - only one thread can access this code at the same time.
        let conn = unsafe { self.as_mut() };

        let mut retry_stream_open = false;

        // check `peer_streams_left_bidi`
        if self.retries.remove(&LocKind::StreamOpen) {
            if conn.peer_streams_left_bidi() > 0 {
                readiness.insert(
                    Event {
                        kind: EventKind::StreamOpen,
                        is_server: conn.is_server(),
                        is_error: false,
                        token: self.id,
                        // unset.
                        stream_id: 0,
                    },
                    None,
                );
            } else {
                retry_stream_open = true;
            }
        }

        for kind in self.retries.drain() {
            match kind {
                LocKind::Send => {
                    // We use `get_next_release_time` to determine if the connection has data to send.
                }
                LocKind::Recv => {
                    readiness.insert(
                        Event {
                            kind: EventKind::Recv,
                            is_server: conn.is_server(),
                            is_error: false,
                            token: self.id,
                            // unset.
                            stream_id: 0,
                        },
                        None,
                    );
                }
                LocKind::StreamSend(stream_id, len) => {
                    // such that it is not going to be
                    // reported as writable again by [`stream_writable_next()`] until its send
                    // capacity reaches `len`.
                    match conn.stream_writable(stream_id, len) {
                        Ok(writable) => {
                            if writable {
                                readiness.insert(
                                    Event {
                                        kind: EventKind::StreamSend,
                                        is_server: conn.is_server(),

                                        is_error: false,
                                        token: self.id,
                                        stream_id,
                                    },
                                    None,
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "failed to call `stream_writable`, scid={:?}, id={}, err={}",
                                conn.trace_id(),
                                stream_id,
                                err
                            );

                            readiness.insert(
                                Event {
                                    kind: EventKind::StreamSend,
                                    is_server: conn.is_server(),
                                    is_error: true,
                                    token: self.id,
                                    stream_id,
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
                                token: self.id,
                                stream_id,
                            },
                            None,
                        );
                    }
                }
                LocKind::StreamShutdown {
                    stream_id,
                    shutdown_read,
                    shutdown_write,
                    err,
                } => {
                    if shutdown_read {
                        if let Err(err) = conn.stream_shutdown(stream_id, Shutdown::Read, err) {
                            log::error!(
                                "shutdown read, scid={:?}, stream_id={}, err={}",
                                conn.source_id(),
                                stream_id,
                                err
                            );
                        }
                    }

                    if shutdown_write {
                        if let Err(err) = conn.stream_shutdown(stream_id, Shutdown::Write, err) {
                            log::error!(
                                "shutdown write, scid={:?}, stream_id={}, err={}",
                                conn.source_id(),
                                stream_id,
                                err
                            );
                        }
                    }
                }
                LocKind::Close => {
                    if let Err(err) = conn.close(false, 0x0, b"") {
                        if err != quiche::Error::Done {
                            log::error!(
                                "close connection, scid={:?}, err={}",
                                conn.source_id(),
                                err
                            );
                        }
                    }
                }
                _ => {
                    unreachable!("unexpect {:?}", kind)
                }
            }
        }

        if retry_stream_open {
            self.retries.insert(LocKind::StreamOpen);
        }

        while let Some(stream_id) = conn.stream_writable_next() {
            readiness.insert(
                Event {
                    kind: EventKind::StreamSend,
                    is_server: conn.is_server(),
                    is_error: false,
                    token: self.id,
                    stream_id,
                },
                None,
            );
        }

        while let Some(stream_id) = conn.stream_readable_next() {
            readiness.insert(
                Event {
                    kind: EventKind::StreamRecv,
                    is_server: conn.is_server(),
                    is_error: false,
                    token: self.id,
                    stream_id,
                },
                None,
            );
        }

        let now = Instant::now();

        // check if the connection has data to send.
        let delay_to = release_time(conn, now, release_timer_threshold);

        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: conn.is_server(),
                is_error: false,
                token: self.id,
                stream_id: 0,
            },
            delay_to,
        );
    }

    // Try open a new bidi-outbound-stream.
    pub fn stream_open(
        &mut self,
        release_timer_threshold: Duration,
        readiness: &mut Readiness,
    ) -> Result<u64> {
        // check if this thread own the state.
        let guard = self.try_lock(LocKind::StreamOpen)?;

        // Safety: only one thread can access this code at the same time.
        let conn = unsafe { self.as_mut() };

        if conn.peer_streams_left_bidi() > 0 {
            let stream_id = self.local_bidi_stream_id_next;
            self.local_bidi_stream_id_next += 4;

            // this a trick, func `stream_priority` will created the target if did not exist.
            conn.stream_priority(stream_id, 255, true)?;

            self.unlock(guard.lock_count, release_timer_threshold, readiness);

            return Ok(stream_id);
        }

        assert_eq!(
            self.try_lock(LocKind::StreamOpen)
                .expect_err("insert `LocKind::StreamOpen` into retry_lock_requests"),
            Error::Busy
        );

        self.unlock(guard.lock_count, release_timer_threshold, readiness);

        Err(Error::Retry)
    }

    /// Careful use this function.
    #[inline]
    unsafe fn as_mut(&self) -> &'static mut Connection {
        unsafe { self.conn.get().as_mut().unwrap() }
    }
}

#[cfg(test)]
mod tests {

    use quiche::Config;

    use super::*;

    #[test]
    fn test_lock() {
        let scid = quiche::ConnectionId::from_ref(b"");
        let mut state = ConnState::new(
            Token(0),
            quiche::connect(
                None,
                &scid,
                "127.0.0.1:1".parse().unwrap(),
                "127.0.0.1:2".parse().unwrap(),
                &mut Config::new(quiche::PROTOCOL_VERSION).unwrap(),
            )
            .unwrap(),
        );

        let guard = state.try_lock(LocKind::Send).unwrap();
        assert_eq!(guard.lock_count, 0);

        assert_eq!(
            state.try_lock(LocKind::Recv).expect_err("Busy"),
            Error::Busy
        );

        assert_eq!(
            state.try_lock(LocKind::StreamOpen).expect_err("Busy"),
            Error::Busy
        );

        assert_eq!(
            state.try_lock(LocKind::StreamRecv(5)).expect_err("Busy"),
            Error::Busy
        );

        let mut readiness = Readiness::default();

        state.unlock(guard.lock_count, Duration::from_micros(250), &mut readiness);

        let guard = state.try_lock(LocKind::Recv).expect("LocKind::Recv");
        assert_eq!(guard.lock_count, 1, "step `lock_count`");
    }

    #[test]
    fn test_stream_open() {
        let scid = quiche::ConnectionId::from_ref(b"");
        let mut state = ConnState::new(
            Token(0),
            quiche::connect(
                None,
                &scid,
                "127.0.0.1:1".parse().unwrap(),
                "127.0.0.1:2".parse().unwrap(),
                &mut Config::new(quiche::PROTOCOL_VERSION).unwrap(),
            )
            .unwrap(),
        );

        let mut readiness = Readiness::default();

        assert_eq!(
            state
                .stream_open(Duration::from_micros(250), &mut readiness)
                .expect_err("pending"),
            Error::Retry
        );

        assert_eq!(
            state
                .stream_open(Duration::from_micros(250), &mut readiness)
                .expect_err("pending"),
            Error::Retry
        );
    }
}
