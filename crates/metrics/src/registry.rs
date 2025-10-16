/// Registry implemenation should implement this trait for `instrument counter`.
pub trait InstrumentCounter: Send + Sync {
    /// Increment counter with `step`.
    fn increment(&self, step: u64);
    /// Update counter to `value`.
    fn absolute(&self, value: u64);
}

/// `Counter` measuring instrument.
pub enum Counter {
    Noop,
    Record(Box<dyn InstrumentCounter>),
}

impl Counter {
    /// See [`increment`](InstrumentCounter::increment)
    #[inline]
    pub fn increment(&self, step: u64) {
        match self {
            Counter::Noop => {}
            Counter::Record(raw_counter) => raw_counter.increment(step),
        }
    }

    /// See [`absolute`](InstrumentCounter::absolute)
    #[inline]
    pub fn absolute(&self, value: u64) {
        match self {
            Counter::Noop => {}
            Counter::Record(raw_counter) => raw_counter.absolute(value),
        }
    }
}

/// Registry implemenation should implement this trait for `instrument gauge`.
pub trait InstrumentGauge: Send + Sync {
    /// Increments the gauge.
    fn increment(&self, value: f64);

    /// Decrements the gauge.
    fn decrement(&self, value: f64);

    /// Set the gauge.
    fn set(&self, value: f64);
}

/// `Gauge` measuring instrument.
pub enum Gauge {
    Noop,
    Record(Box<dyn InstrumentGauge>),
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

/// Registry implemenation should implement this trait for `instrument histogam`.
pub trait InstrumentHistogram: Send + Sync {
    /// Records a value into the histogram.
    fn record(&self, value: f64);
}

/// `Histogam` measuring instrument.
pub enum Histogram {
    Noop,
    Record(Box<dyn InstrumentHistogram>),
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

/// Registry of measuring instruments must implement this trait.
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
