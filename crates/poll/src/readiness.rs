use std::{
    cmp::Reverse,
    collections::HashSet,
    time::{Duration, Instant},
};

use priority_queue::PriorityQueue;
use zerortt_api::Event;

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
        log::trace!("insert readiness, event={:?}, delay={:?}", event, delay_to);
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
