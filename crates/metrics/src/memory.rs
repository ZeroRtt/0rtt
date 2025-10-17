//! An in-memory `registry` implementation.

use crate::Registry;

/// A builtin in-memory [`Registry`](crate::Registry) implementation works
/// in tandem with the pull-mode data collector.
pub struct MemoryRegistry {}

#[allow(unused)]
impl Registry for MemoryRegistry {
    fn counter(&self, token: crate::Token<'_>) -> crate::Counter {
        crate::Counter::Noop
    }

    fn gauge(&self, token: crate::Token<'_>) -> crate::Gauge {
        crate::Gauge::Noop
    }

    fn histogam(&self, token: crate::Token<'_>) -> crate::Histogram {
        crate::Histogram::Noop
    }
}
