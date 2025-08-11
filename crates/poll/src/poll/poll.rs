use std::{
    cell::UnsafeCell,
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use quiche::ConnectionId;

use crate::poll::{Error, Event, Result, conn::ConnState, deadline::Deadline};

static DEFAULT_RELEASE_TIMER_THRESHOLD: Duration = Duration::from_micros(250);

#[derive(Default)]
struct PollState {
    /// connection id generator.
    conn_id_next: u32,
    /// Internally managed connection state set.
    conn_stats: HashMap<u32, ConnState>,
    /// Internally managed connection source id set.
    scids: HashMap<ConnectionId<'static>, u32>,
    /// Deadline for all internally managed conneciton timers.
    deadline: UnsafeCell<Deadline>,
    /// readiness events.
    events: UnsafeCell<HashSet<Event>>,
}

/// Socket for quic connection created by [`register`](Poll::register) function.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct QuicConn(pub u32);

/// Socket for quic stream.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct QuicStream(pub u32, pub u64);

/// Poll for readiness events on all registered quiche connections.
#[derive(Default)]
pub struct Poll {
    /// top spin-lock.
    lock: AtomicBool,
    /// internal state protected by spin-lock.
    state: UnsafeCell<PollState>,
}

impl Poll {
    /// Access the spin-lock protected `state`.
    fn lock_state<F, O>(&self, f: F) -> O
    where
        F: FnOnce(&mut PollState) -> O,
    {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // No need to worry about dead cycles taking up cpu time,
            // all internal locking operations will be done in a few
            // microseconds.
        }

        let out = f(unsafe { self.state.get().as_mut().unwrap() });

        self.lock.store(false, Ordering::Release);

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
                state.deadline.get().as_mut().unwrap().remove(&conn.0);
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
}
