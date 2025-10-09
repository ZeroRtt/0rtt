use std::{
    cmp::Reverse,
    collections::HashSet,
    time::{Duration, Instant},
};

use priority_queue::PriorityQueue;

use crate::poll::utils::is_bidi;

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

/// Readiness events.
#[derive(Default)]
pub struct Readiness {
    /// delayed event priority queue.
    pub(crate) delayed: PriorityQueue<Event, Reverse<Instant>>,
    /// readiness events.
    pub(crate) events: HashSet<Event>,
}

impl Readiness {
    /// Insert a new readiness event with optional delay times.
    pub fn insert(&mut self, event: Event, delay_to: Option<Instant>) {
        log::trace!("readiness, event={:?}, delay={:?}", event, delay_to);
        if let Some(delay_to) = delay_to {
            self.events.remove(&event);
            self.delayed.push(event, Reverse(delay_to));
        } else {
            self.delayed.remove(&event);
            self.events.insert(event);
        }
    }

    pub fn remove(&mut self, event: Event) {
        self.events.remove(&event);
        self.delayed.remove(&event);
    }

    /// Poll readiness events.
    ///
    /// If no ready messages are polled, return an optional wait time instant.
    pub fn poll(
        &mut self,
        readiness: &mut Vec<Event>,
        release_timer_threshold: Duration,
    ) -> Option<Instant> {
        assert!(
            readiness.is_empty(),
            "The input readiness Vec<_> is not empty."
        );

        readiness.extend(self.events.drain());

        if self.delayed.is_empty() {
            return None;
        }

        let now = Instant::now();

        while let Some(deadline) = self.delayed.peek().map(|(_, instant)| instant.0) {
            if !(deadline.checked_duration_since(now).unwrap_or_default() > release_timer_threshold)
            {
                let (event, _) = self.delayed.pop().unwrap();

                readiness.push(event);
                continue;
            }

            return Some(deadline);
        }

        return None;
    }
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
