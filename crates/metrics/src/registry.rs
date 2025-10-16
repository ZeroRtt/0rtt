/// Measuring instrument **should** implement this trait.
pub trait Instrument {
    /// Creates a no-op `instrument` which does nothing.
    fn noop() -> Self;
}

/// Raw counter instrument.
pub trait RawCounter: Send + Sync {
    /// Increment counter with `step`.
    fn increment(&self, step: u64);
    /// Update counter to `value`.
    fn absolute(&self, value: u64);
}

/// measuring instrument `counter`.
pub enum Counter {
    Noop,
    Record(Box<dyn RawCounter>),
}

impl Instrument for Counter {
    fn noop() -> Self {
        Self::Noop
    }
}

impl RawCounter for Counter {
    fn increment(&self, step: u64) {
        match self {
            Counter::Noop => {}
            Counter::Record(raw_counter) => raw_counter.increment(step),
        }
    }

    fn absolute(&self, value: u64) {
        match self {
            Counter::Noop => {}
            Counter::Record(raw_counter) => raw_counter.absolute(value),
        }
    }
}

/// Raw `histogram` instrument.
pub trait RawHistogram: Send + Sync {
    /// Records a value into the histogram.
    fn record(&self, step: u64);
}

/// Registry for measuring instruments.
pub trait Registry {
    /// Register/Get measuring instrument `counter`.
    fn counter(&self, key: &str, tags: &[(&str, &str)]) -> Counter;
}
