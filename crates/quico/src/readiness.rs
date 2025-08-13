use std::{cmp::Reverse, collections::HashSet, time::Instant};

use priority_queue::PriorityQueue;

use crate::{Event, EventKind};

/// tiny timing-wheel for internal use
#[derive(Default, Clone)]
pub(crate) struct Deadline(PriorityQueue<u32, Reverse<Instant>>);

impl Deadline {
    /// Insert a new deadline.
    #[inline]
    pub fn insert(&mut self, conn_id: u32, deadline: Instant) {
        self.0.push(conn_id, Reverse(deadline));
    }

    /// Deletes the deadline for a connection.
    #[inline]
    pub fn remove(&mut self, conn_id: &u32) {
        self.0.remove(conn_id);
    }

    /// Returns the nearest deadline.
    #[inline]
    pub fn deadline(&self) -> Option<Instant> {
        self.0.peek().map(|(_, deadline)| deadline.0)
    }

    /// Returns timeout iterator.
    #[inline]
    pub fn timeout(&mut self, now: Instant) -> Timeout<'_> {
        Timeout {
            deadline: self,
            now,
        }
    }
}

pub struct Timeout<'a> {
    deadline: &'a mut Deadline,
    now: Instant,
}

impl<'a> Iterator for Timeout<'a> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(deadline) = self.deadline.deadline() {
            if !(deadline > self.now) {
                let (conn_id, _) = self.deadline.0.pop().unwrap();
                return Some(conn_id);
            }
        }

        return None;
    }
}

/// Readiness events.
#[derive(Default)]
pub(crate) struct Readiness {
    /// delayed `send` events.
    delayed: Deadline,
    /// readiness events.
    immediate: HashSet<Event>,
}

impl Readiness {
    #[inline]
    pub fn insert_event(&mut self, event: Event) {
        if event.kind == EventKind::Send {
            if let Some(release_time) = event.release_time {
                self.delayed.insert(event.conn_id, release_time);
                return;
            }
        }

        self.immediate.insert(event);
    }

    #[inline]
    pub fn next_delayed_send_event(&self) -> Option<Instant> {
        self.delayed.deadline()
    }

    #[inline]
    pub fn insert_delayed_send_event(&mut self, conn_id: u32, deadline: Instant) {
        self.delayed.insert(conn_id, deadline);
    }

    #[inline]
    pub fn remove_delayed_send_event(&mut self, conn_id: u32) {
        self.delayed.remove(&conn_id);
    }

    #[inline]
    pub fn immediate(&mut self) -> impl Iterator<Item = Event> {
        self.immediate.drain()
    }

    #[inline]
    pub fn timeout(&mut self) -> impl Iterator<Item = u32> {
        self.delayed.timeout(Instant::now())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.immediate.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rand::random;

    use super::*;

    #[test]
    fn test_deadline() {
        let mut deadline = Deadline::default();

        let now = Instant::now();

        let range_max = 1000;

        for i in 0..range_max {
            deadline.insert(i, now + Duration::from_secs(i as u64));
        }

        for _ in 0..range_max {
            let next = random::<u64>() % range_max as u64;
            assert_eq!(
                deadline
                    .clone()
                    .timeout(now + Duration::from_secs(next))
                    .count(),
                next as usize + 1
            );
        }
    }

    #[test]
    fn test_deadline_update() {
        let mut deadline = Deadline::default();
        let now = Instant::now();
        deadline.insert(0, now + Duration::from_secs(1));
        assert_eq!(deadline.deadline(), Some(now + Duration::from_secs(1)));
        deadline.insert(0, now + Duration::from_secs(10));
        assert_eq!(deadline.deadline(), Some(now + Duration::from_secs(10)));
        assert_eq!(
            deadline.timeout(now + Duration::from_secs(20)).next(),
            Some(0)
        );
    }
}
