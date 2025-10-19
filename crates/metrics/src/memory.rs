//! An in-memory `registry` implementation.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::RwLock;

use crate::{CounterWrite, GaugeWrite, HistogramWrite, Registry, Token};

#[allow(unused)]
struct Write(Arc<AtomicU64>);

#[allow(unused)]
impl CounterWrite for Write {
    fn increment(&self, step: u64) {
        self.0.fetch_add(step, Ordering::AcqRel);
    }

    fn absolute(&self, value: u64) {
        self.0.swap(value, Ordering::AcqRel);
    }
}

#[allow(unused)]
impl HistogramWrite for Write {
    fn record(&self, value: f64) {
        self.0.swap(value.to_bits(), Ordering::AcqRel);
    }
}

#[allow(unused)]
impl GaugeWrite for Write {
    fn increment(&self, value: f64) {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |curr| {
                let input = f64::from_bits(curr);
                let output = input + value;
                Some(output.to_bits())
            });
    }

    fn decrement(&self, value: f64) {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |curr| {
                let input = f64::from_bits(curr);
                let output = input - value;
                Some(output.to_bits())
            });
    }

    fn set(&self, value: f64) {
        self.0.swap(value.to_bits(), Ordering::AcqRel);
    }
}

#[allow(unused)]
struct MetaData {
    name: String,
    labels: Vec<(String, String)>,
}

impl<'a> From<Token<'a>> for MetaData {
    fn from(value: Token<'a>) -> Self {
        Self {
            name: value.name.to_owned(),
            labels: value
                .labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<Vec<_>>(),
        }
    }
}

#[derive(Default)]
struct MutableData {
    instruments: HashMap<u64, Arc<AtomicU64>>,
    metadata: HashMap<u64, MetaData>,
}

/// A builtin in-memory [`Registry`](crate::Registry) implementation works
/// in tandem with the pull-mode data collector.
#[derive(Default)]
pub struct MemoryRegistry {
    mutable: RwLock<MutableData>,
}
impl MemoryRegistry {
    fn get(&self, token: crate::Token<'_>) -> Arc<AtomicU64> {
        if let Some(counter) = self.mutable.read().instruments.get(&token.hash) {
            return counter.clone();
        }

        let value: Arc<AtomicU64> = Default::default();

        let mut mutable_data = self.mutable.write();

        mutable_data
            .metadata
            .insert(token.hash, MetaData::from(token));

        mutable_data.instruments.insert(token.hash, value.clone());

        value
    }
}

impl Registry for MemoryRegistry {
    fn counter(&self, token: crate::Token<'_>) -> crate::Counter {
        crate::Counter::Record(Box::new(Write(self.get(token))))
    }

    fn gauge(&self, token: crate::Token<'_>) -> crate::Gauge {
        crate::Gauge::Record(Box::new(Write(self.get(token))))
    }

    fn histogam(&self, token: crate::Token<'_>) -> crate::Histogram {
        crate::Histogram::Record(Box::new(Write(self.get(token))))
    }
}
