use std::{cmp::Reverse, collections::HashSet, time::Instant};

use priority_queue::PriorityQueue;

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
    StreamOpen,
    /// Readiness for `stream_send` operation.
    StreamSend,
    /// Readiness for `stream_recv` operation.
    StreamRecv,
}

/// Associates readiness events with quic connection.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Token(pub u32);

/// Readiness I/O event.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Event {
    /// Type of this event.
    pub kind: EventKind,
    /// Event source is a server-side connection.
    pub is_server: bool,
    /// Event raised by an I/O error.
    pub is_error: bool,
    /// Connection token.
    pub token: Token,
    /// Optional stream identifier for the event source.
    /// The default value `0` indicates that the event source is not a QUIC stream.
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
        if let Some(delay_to) = delay_to {
            self.delayed.push(event, Reverse(delay_to));
        } else {
            self.events.insert(event);
        }
    }

    /// Poll readiness events.
    ///
    /// If no ready messages are polled, return an optional wait time instant.
    pub fn poll(&mut self, readiness: &mut Vec<Event>) -> Option<Instant> {
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
            if !(deadline > now) {
                let (event, _) = self.delayed.pop().unwrap();

                readiness.push(event);
                continue;
            }

            if readiness.is_empty() {
                return Some(deadline);
            } else {
                return None;
            }
        }

        return None;
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    #[test]
    fn test_deduplication() {
        let mut readiness = Readiness::default();

        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token: Token(0),
                stream_id: 0,
            },
            None,
        );

        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token: Token(0),
                stream_id: 0,
            },
            None,
        );

        let mut events = vec![];

        assert!(readiness.poll(&mut events).is_none());

        assert_eq!(
            events,
            vec![Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token: Token(0),
                stream_id: 0,
            }]
        );
    }

    #[test]
    fn test_timeout() {
        let delay_to = Instant::now();

        let mut readiness = Readiness::default();

        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token: Token(0),
                stream_id: 0,
            },
            Some(delay_to),
        );

        readiness.insert(
            Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token: Token(0),
                stream_id: 0,
            },
            Some(delay_to),
        );

        let mut events = vec![];

        assert!(readiness.poll(&mut events).is_none());

        assert_eq!(
            events,
            vec![Event {
                kind: EventKind::Send,
                is_server: false,
                is_error: false,
                token: Token(0),
                stream_id: 0,
            }]
        );
    }
}
