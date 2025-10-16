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

impl Counter {
    /// See [`increment`](RawCounter::increment)
    #[inline]
    pub fn increment(&self, step: u64) {
        match self {
            Counter::Noop => {}
            Counter::Record(raw_counter) => raw_counter.increment(step),
        }
    }

    /// See [`absolute`](RawCounter::absolute)
    #[inline]
    pub fn absolute(&self, value: u64) {
        match self {
            Counter::Noop => {}
            Counter::Record(raw_counter) => raw_counter.absolute(value),
        }
    }
}

/// Raw `gauge` instrument.
pub trait RawGauge: Send + Sync {
    /// Increments the gauge.
    fn increment(&self, value: f64);

    /// Decrements the gauge.
    fn decrement(&self, value: f64);

    /// Set the gauge.
    fn set(&self, value: f64);
}

/// measuring instrument `gauge`.
pub enum Gauge {
    Noop,
    Record(Box<dyn RawGauge>),
}

impl Instrument for Gauge {
    fn noop() -> Self {
        Self::Noop
    }
}

impl Gauge {
    /// Increments the gauge.
    #[inline]
    pub fn increment(&self, value: f64) {
        match self {
            Gauge::Noop => {}
            Gauge::Record(raw_gauge) => raw_gauge.increment(value),
        }
    }

    /// Decrements the gauge.
    #[inline]
    pub fn decrement(&self, value: f64) {
        match self {
            Gauge::Noop => {}
            Gauge::Record(raw_gauge) => raw_gauge.decrement(value),
        }
    }

    /// Set the gauge.
    #[inline]
    pub fn set(&self, value: f64) {
        match self {
            Gauge::Noop => {}
            Gauge::Record(raw_gauge) => raw_gauge.set(value),
        }
    }
}

/// Raw `histogram` instrument.
pub trait RawHistogram: Send + Sync {
    /// Records a value into the histogram.
    fn record(&self, value: f64);
}

/// measuring instrument `histogam`.
pub enum Histogram {
    Noop,
    Record(Box<dyn RawHistogram>),
}

impl Instrument for Histogram {
    fn noop() -> Self {
        Self::Noop
    }
}

impl Histogram {
    /// See [`record`](Histogram::record)
    #[inline]
    pub fn record(&self, value: f64) {
        match self {
            Histogram::Noop => {}
            Histogram::Record(raw_histogram) => raw_histogram.record(value),
        }
    }
}

/// Registry for measuring instruments.
pub trait Registry: Send + Sync {
    /// Register/Get measuring instrument `counter`.
    #[must_use = "This will cause unnecessary performance loss."]
    fn counter(&self, key: &str, tags: &[(&str, &str)]) -> Counter;

    /// Register/Get measuring instrument `gauge`.
    #[must_use = "This will cause unnecessary performance loss."]
    fn gauge(&self, key: &str, tags: &[(&str, &str)]) -> Gauge;

    /// Register/Get measuring instrument `histogam`.
    #[must_use = "This will cause unnecessary performance loss."]
    fn histogam(&self, key: &str, tags: &[(&str, &str)]) -> Histogram;
}
