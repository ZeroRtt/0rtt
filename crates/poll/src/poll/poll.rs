use std::{
    cell::UnsafeCell,
    collections::HashMap,
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

use quiche::{ConnectionId, RecvInfo, SendInfo};

use crate::{
    poll::{
        Error, Event, EventKind, Result,
        conn::{ConnState, ConnStateGuard, Lockind},
        readiness::Readiness,
    },
    utils::{min_of_some, release_time},
};

static DEFAULT_RELEASE_TIMER_THRESHOLD: Duration = Duration::from_micros(250);

#[derive(Default)]
struct PollState {
    /// connection id generator.
    conn_id_next: u32,
    /// Internally managed connection state set.
    conn_stats: HashMap<u32, ConnState>,
    /// Internally managed connection source id set.
    scids: HashMap<ConnectionId<'static>, u32>,
    /// readiness events.
    readiness: UnsafeCell<Readiness>,
}

impl PollState {
    /// Get readiness ptr. this is an unsafe function,
    /// must make sure only one thread can access this
    /// function at the same time.
    unsafe fn readiness(&self) -> &'static mut Readiness {
        unsafe { self.readiness.get().as_mut().unwrap() }
    }
}

/// Socket for quic connection created by [`register`](Poll::register) function.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct QuicConn(pub u32);

/// Socket for quic stream.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct QuicStream(pub u32, pub u64);

struct PollStateGuard<'a> {
    conn_id: u32,
    conn_state_guard: ConnStateGuard,
    poll: &'a Poll,
}

impl<'a> Drop for PollStateGuard<'a> {
    fn drop(&mut self) {
        self.poll.lock_state(|state| {
            // Safety: protected by top spin-lock.
            let readiness = unsafe { state.readiness() };

            let conn = state
                .conn_stats
                .get_mut(&self.conn_id)
                .expect("Resource is not found.");

            conn.unlock(self.conn_state_guard.lock_count, |event| {
                readiness.insert_event(event)
            });

            if !readiness.is_empty() {
                self.poll.condvar.notify_one();
            }
        });
    }
}

impl<'a> Deref for PollStateGuard<'a> {
    type Target = quiche::Connection;

    fn deref(&self) -> &Self::Target {
        self.conn_state_guard.conn
    }
}

impl<'a> DerefMut for PollStateGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn_state_guard.conn
    }
}

/// Poll for readiness events on all registered quiche connections.
#[derive(Default)]
pub struct Poll {
    state: parking_lot::Mutex<PollState>,
    condvar: parking_lot::Condvar,
}

impl Poll {
    /// Access the spin-lock protected `state`.
    #[inline]
    fn lock_state<F, O>(&self, f: F) -> O
    where
        F: FnOnce(&mut PollState) -> O,
    {
        let mut guard = self.state.lock();

        let out = f(&mut guard);

        out
    }

    /// Wrap and register a `quiche::Connection`.
    pub fn register(&self, conn: quiche::Connection) -> Result<QuicConn> {
        self.lock_state(|state| {
            let id = state.conn_id_next;
            (state.conn_id_next, _) = state.conn_id_next.overflowing_add(1);

            assert!(
                state
                    .scids
                    .insert(conn.source_id().into_owned(), id)
                    .is_none(),
                "add same scid"
            );

            assert!(
                state
                    .conn_stats
                    .insert(
                        id,
                        ConnState::new(id, DEFAULT_RELEASE_TIMER_THRESHOLD, conn),
                    )
                    .is_none(),
                "Connection id conflict"
            );

            Ok(QuicConn(id))
        })
    }

    /// Unwrap and release managed `quiche::Connection.`
    pub fn deregister(&self, conn: QuicConn) -> Result<quiche::Connection> {
        self.lock_state(|state| {
            unsafe {
                state.readiness().remove_delayed_send_event(conn.0);
            };

            let conn = state
                .conn_stats
                .remove(&conn.0)
                .ok_or_else(|| Error::NotFound)?;

            let conn: quiche::Connection = conn.into();

            assert!(
                state.scids.remove(&conn.source_id().into_owned()).is_some(),
                "deregister: scids."
            );

            Ok(conn)
        })
    }

    /// Writes a single QUIC packet to be sent to the peer.
    pub fn send(&self, conn: QuicConn, buf: &mut [u8]) -> Result<(usize, SendInfo)> {
        let conn_id = conn.0;

        let mut conn = self.lock_state(|state| -> Result<_> {
            let conn = state
                .conn_stats
                .get_mut(&conn_id)
                .ok_or_else(|| Error::NotFound)?;

            let conn_state_guard = conn.try_lock(Lockind::Send)?;

            Ok(PollStateGuard {
                conn_id,
                conn_state_guard,
                poll: self,
            })
        })?;

        let release_time = release_time(&conn, Instant::now(), DEFAULT_RELEASE_TIMER_THRESHOLD);

        if let Some(deadline) = release_time {
            self.lock_state(|state| unsafe {
                state
                    .readiness()
                    .insert_delayed_send_event(conn_id, deadline);
            });

            return Err(Error::Retry);
        }

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
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!("connection send, scid={:?}, err={}", conn.trace_id(), err);
                return Err(Error::Quiche(err));
            }
        }
    }

    /// Processes QUIC packets received from the peer.
    pub fn recv(&self, buf: &mut [u8], info: RecvInfo) -> Result<usize> {
        let header =
            quiche::Header::from_slice(buf, quiche::MAX_CONN_ID_LEN).map_err(Error::Quiche)?;

        // TODO: check header `ty`

        let scid = header.dcid;

        let mut conn = self.lock_state(|state| -> Result<_> {
            let conn_id = state
                .scids
                .get(&scid)
                .cloned()
                .ok_or_else(|| Error::NotFound)?;

            let conn = state
                .conn_stats
                .get_mut(&conn_id)
                .ok_or_else(|| Error::NotFound)?;

            let conn_state_guard = conn.try_lock(Lockind::Recv)?;

            Ok(PollStateGuard {
                conn_id,
                conn_state_guard,
                poll: self,
            })
        })?;

        match conn.recv(buf, info) {
            Ok(recv_size) => {
                log::trace!(
                    "Connection recv, scid={:?}, len={}",
                    conn.source_id(),
                    recv_size
                );
                Ok(recv_size)
            }
            Err(err) => {
                log::error!("Connection recv, scid={:?}, err={}", conn.source_id(), err);
                Err(Error::Quiche(err))
            }
        }
    }

    /// Writes data to a stream.
    pub fn stream_send(&self, stream: QuicStream, buf: &[u8], fin: bool) -> Result<usize> {
        let mut conn = self.lock_state(|state| -> Result<_> {
            let conn = state
                .conn_stats
                .get_mut(&stream.0)
                .ok_or_else(|| Error::NotFound)?;

            let conn_state_guard = conn.try_lock(Lockind::Recv)?;

            Ok(PollStateGuard {
                conn_id: stream.0,
                conn_state_guard,
                poll: self,
            })
        })?;

        assert!(
            conn.is_established() || conn.is_in_early_data(),
            "Call stream send before connection established."
        );

        match conn.stream_send(stream.1, buf, fin) {
            Ok(send_size) => {
                log::trace!(
                    "stream send, scid={:?}, stream_id={}, len={}, fin={}",
                    conn.source_id(),
                    stream.1,
                    send_size,
                    fin
                );
                return Ok(send_size);
            }
            Err(quiche::Error::Done) => {
                log::trace!(
                    "stream send, scid={:?}, stream_id={}, fin={}, Done",
                    conn.source_id(),
                    stream.1,
                    fin
                );
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!(
                    "stream send, scid={:?}, stream_id={}, fin={}, err={}",
                    conn.source_id(),
                    stream.1,
                    fin,
                    err
                );

                return Err(Error::Quiche(err));
            }
        }
    }

    /// Reads contiguous data from a stream into the provided slice.
    pub fn stream_recv(&self, stream: QuicStream, buf: &mut [u8]) -> Result<(usize, bool)> {
        let mut conn = self.lock_state(|state| -> Result<_> {
            let conn = state
                .conn_stats
                .get_mut(&stream.0)
                .ok_or_else(|| Error::NotFound)?;

            let conn_state_guard = conn.try_lock(Lockind::Recv)?;

            Ok(PollStateGuard {
                conn_id: stream.0,
                conn_state_guard,
                poll: self,
            })
        })?;

        match conn.stream_recv(stream.1, buf) {
            Ok((recv_size, fin)) => {
                log::trace!(
                    "stream recv, scid={:?}, stream_id={}, len={}, fin={}",
                    conn.source_id(),
                    stream.1,
                    recv_size,
                    fin
                );
                return Ok((recv_size, fin));
            }
            Err(quiche::Error::Done) => {
                log::trace!(
                    "stream recv, scid={:?}, stream_id={}, Done",
                    conn.source_id(),
                    stream.1,
                );
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!(
                    "stream recv, scid={:?}, stream_id={}, err={}",
                    conn.source_id(),
                    stream.1,
                    err
                );

                return Err(Error::Quiche(err));
            }
        }
    }

    /// Try open a new outbound stream.
    pub fn stream_open(&self, conn: QuicConn) -> Result<QuicStream> {
        let conn_id = conn.0;

        self.lock_state(|state| -> Result<_> {
            // Safety: protected by top spin-lock.
            let readiness = unsafe { state.readiness() };

            let conn = state
                .conn_stats
                .get_mut(&conn_id)
                .ok_or_else(|| Error::NotFound)?;

            let stream_id = conn.stream_open(|event| {
                readiness.insert_event(event);
            })?;

            Ok(QuicStream(conn_id, stream_id))
        })
    }

    /// Wait for readiness events
    pub fn poll(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<()> {
        let timeout = timeout.map(|timeout| Instant::now() + timeout);

        let mut state = self.state.lock();

        let readiness = unsafe { state.readiness() };

        loop {
            events.extend(readiness.immediate());

            if !events.is_empty()
                || timeout
                    .filter(|timeout| !(Instant::now() < *timeout))
                    .is_some()
            {
                return Ok(());
            }

            let next_delayed_send_event = readiness.next_delayed_send_event();

            let timeout = min_of_some(timeout, next_delayed_send_event);

            if let Some(timeout) = timeout {
                self.condvar.wait_until(&mut state, timeout);
            } else {
                self.condvar.wait(&mut state);
            }

            for conn_id in readiness.timeout() {
                events.push(Event {
                    kind: EventKind::Send,
                    lock_released: false,
                    is_error: false,
                    conn_id,
                    stream_id: 0,
                    release_time: None,
                });
            }
        }
    }
}
