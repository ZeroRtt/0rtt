use std::{
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use quiche::{ConnectionId, RecvInfo, SendInfo};

use crate::{
    Error, Event, Result, Token, conn::LocKind, registration::Registration, utils::min_of_some,
};

static DEFAULT_RELEASE_TIMER_THRESHOLD: Duration = Duration::from_micros(250);

struct ConnGuard<'a> {
    token: Token,
    conn_state_guard: crate::conn::ConnGuard,
    group: &'a Group,
    send_done: bool,
}

impl<'a> Drop for ConnGuard<'a> {
    fn drop(&mut self) {
        let mut registration = self.group.registration.lock();

        unsafe {
            assert!(
                registration
                    .unlock_conn(
                        self.send_done,
                        DEFAULT_RELEASE_TIMER_THRESHOLD,
                        self.token,
                        self.conn_state_guard.lock_count,
                    )
                    .is_ok(),
                "unlock connection {:?}",
                self.token
            );
        }

        self.group.condvar.notify_one();
    }
}

impl<'a> Deref for ConnGuard<'a> {
    type Target = quiche::Connection;

    fn deref(&self) -> &Self::Target {
        self.conn_state_guard.conn
    }
}

impl<'a> DerefMut for ConnGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn_state_guard.conn
    }
}

/// A multiplexer for quic connections.
#[derive(Default)]
pub struct Group {
    registration: Mutex<Registration>,
    condvar: Condvar,
}

impl Group {
    /// Create a new `Group` instance.
    pub fn new() -> Self {
        Self::default()
    }
    /// Wrap and register a `quiche::Connection`.
    pub fn register(&self, conn: quiche::Connection) -> Result<Token> {
        let mut state = self.registration.lock();

        state.register(conn, DEFAULT_RELEASE_TIMER_THRESHOLD)
    }

    /// Unwrap and release managed `quiche::Connection.`
    pub fn deregister(&self, token: Token) -> Result<quiche::Connection> {
        let mut state = self.registration.lock();
        state.deregister(token)
    }

    fn lock_conn(&self, token: Token, kind: LocKind) -> Result<ConnGuard<'_>> {
        let mut state = self.registration.lock();

        Ok(ConnGuard {
            token,
            conn_state_guard: state.try_lock_conn(token, kind)?,
            group: self,
            send_done: false,
        })
    }

    fn lock_conn_with(&self, scid: &ConnectionId<'_>, kind: LocKind) -> Result<ConnGuard<'_>> {
        let mut state = self.registration.lock();

        let (token, conn_state_guard) = state.try_lock_conn_by(scid, kind)?;

        Ok(ConnGuard {
            token,
            conn_state_guard,
            group: self,
            send_done: false,
        })
    }

    /// Writes a single QUIC packet to be sent to the peer.
    pub fn send(&self, token: Token, buf: &mut [u8]) -> Result<(usize, SendInfo)> {
        let mut conn = self.lock_conn(token, LocKind::Send)?;

        // if let Some(release_time) =
        //     release_time(&conn, Instant::now(), DEFAULT_RELEASE_TIMER_THRESHOLD)
        // {
        //     log::trace!(
        //         "connection send, scid={:?}, next_release_time={:?}",
        //         conn.trace_id(),
        //         release_time,
        //     );
        //     return Err(Error::Retry);
        // }

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
                conn.send_done = true;
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

        self.recv_with_(&header.dcid, buf, info, false)
            .map(|(_, recv_size)| recv_size)
    }

    /// Processes QUIC packets received from the peer.
    pub(crate) fn recv_with_(
        &self,
        scid: &ConnectionId<'_>,
        buf: &mut [u8],
        info: RecvInfo,
        is_server: bool,
    ) -> Result<(Token, usize)> {
        let mut conn = self.lock_conn_with(scid, LocKind::Recv)?;

        if is_server && !conn.is_server() {
            panic!("dispatch packet to non-server connection.");
        }

        match conn.recv(buf, info) {
            Ok(recv_size) => {
                log::trace!(
                    "Connection recv, scid={:?}, len={}",
                    conn.source_id(),
                    recv_size
                );

                Ok((conn.token, recv_size))
            }
            Err(err) => {
                log::error!("Connection recv, scid={:?}, err={}", conn.source_id(), err);
                Err(Error::Quiche(err))
            }
        }
    }

    /// Writes data to a stream.
    pub fn stream_send(
        &self,
        token: Token,
        stream_id: u64,
        buf: &[u8],
        fin: bool,
    ) -> Result<usize> {
        let mut conn = self.lock_conn(token, LocKind::StreamSend(stream_id, buf.len()))?;

        // assert!(
        //     conn.is_established() || conn.is_in_early_data(),
        //     "Call stream send before connection established."
        // );

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
    pub fn stream_recv(
        &self,
        token: Token,
        stream_id: u64,
        buf: &mut [u8],
    ) -> Result<(usize, bool)> {
        let mut conn = self.lock_conn(token, LocKind::StreamRecv(stream_id))?;

        match conn.stream_recv(stream_id, buf) {
            Ok((recv_size, fin)) => {
                log::trace!(
                    "stream recv, scid={:?}, stream_id={}, len={}, fin={}",
                    conn.source_id(),
                    stream_id,
                    recv_size,
                    fin
                );
                return Ok((recv_size, fin));
            }
            Err(quiche::Error::Done) => {
                log::trace!(
                    "stream recv, scid={:?}, stream_id={}, Done",
                    conn.source_id(),
                    stream_id,
                );
                return Err(Error::Retry);
            }
            Err(err) => {
                log::error!(
                    "stream recv, scid={:?}, stream_id={}, err={}",
                    conn.source_id(),
                    stream_id,
                    err
                );

                return Err(Error::Quiche(err));
            }
        }
    }

    /// Try open a new outbound stream.
    pub fn stream_open(&self, token: Token) -> Result<u64> {
        self.registration
            .lock()
            .stream_open(token, DEFAULT_RELEASE_TIMER_THRESHOLD)
    }

    /// Close a stream.
    pub fn stream_close(&self, token: Token, stream_id: u64) -> Result<()> {
        self.registration
            .lock()
            .stream_close(token, stream_id, DEFAULT_RELEASE_TIMER_THRESHOLD)
    }

    /// Wait for readiness events
    pub fn poll(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<()> {
        let timeout = timeout.map(|timeout| Instant::now() + timeout);

        let mut state = self.registration.lock();

        let readiness = unsafe { state.readiness() };

        loop {
            let next_release = readiness.poll(events);

            if !events.is_empty()
                || timeout
                    .filter(|timeout| !(Instant::now() < *timeout))
                    .is_some()
            {
                return Ok(());
            }

            let timeout = min_of_some(timeout, next_release);

            if let Some(timeout) = timeout {
                self.condvar.wait_until(&mut state, timeout);
            } else {
                self.condvar.wait(&mut state);
            }
        }
    }

    /// Waits for readiness events without blocking current thread and returns possible retry time duration.
    pub fn non_blocking_poll(&self, events: &mut Vec<Event>) -> Option<Instant> {
        let mut state = self.registration.lock();

        let readiness = unsafe { state.readiness() };

        readiness.poll(events)
    }
}
