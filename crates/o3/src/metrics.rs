use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::token::Token;

pub struct Metrics {
    mio_polls: usize,
    quico_polls: usize,
    mio_events: usize,
    quico_events: usize,
    traffics: HashMap<(Token, Token), u64>,
    last_report_instant: Instant,
    report_interval: Duration,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            mio_polls: Default::default(),
            quico_polls: Default::default(),
            mio_events: Default::default(),
            quico_events: Default::default(),
            traffics: Default::default(),
            last_report_instant: Instant::now(),
            report_interval: Duration::from_secs(5),
        }
    }
}

impl Metrics {
    #[allow(unused)]
    pub fn new(report_interval: Duration) -> Self {
        Self {
            report_interval,
            ..Default::default()
        }
    }

    pub fn mio_poll_add(&mut self, events: usize) {
        self.mio_polls += 1;
        self.mio_events += events;
    }

    pub fn qucio_poll_add(&mut self, events: usize) {
        self.quico_polls += 1;
        self.quico_events += events;
    }

    pub fn traffic_add(&mut self, from: Token, to: Token, len: usize) {
        *self.traffics.entry((from, to)).or_insert(len as u64) += len as u64;
    }

    pub fn report(&mut self) {
        let elapsed = self.last_report_instant.elapsed();
        if elapsed < self.report_interval {
            return;
        }

        log::info!(
            "metrics: mio_polls={}, mio_events={}, quic_polls={}, quic_events={}",
            self.mio_polls,
            self.mio_events,
            self.quico_polls,
            self.quico_events
        );

        for ((from, to), len) in self.traffics.drain() {
            log::info!(
                "transfer data, from={:?}, to={:?}, len={}, rate={:.3}kb/s",
                from,
                to,
                len,
                len as f64 / 1000f64 / elapsed.as_secs() as f64
            );
        }

        self.mio_polls = 0;
        self.mio_events = 0;
        self.quico_polls = 0;
        self.quico_events = 0;

        self.last_report_instant = Instant::now();
    }
}
