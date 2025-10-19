pub use metricrs_derive::*;

use crate::Token;

/// Kind of the `#[metricrs::instrument]` driving.
pub enum DeriveKind {
    /// A `counter` measuring instrument.
    Counter,
    /// A `timer` measuring instrument.
    Timer,
}

/// Instrument options set directly by the user in `#[metricrs::instrument]`.
#[derive(Default)]
pub struct DeriveOption<'a> {
    /// Driving instrument `type`.
    pub kind: Option<DeriveKind>,
    /// Set the `name` of generating instrument .
    pub name: Option<&'a str>,
    /// Attach labels to this instrument.
    pub labels: Option<&'a [(&'a str, &'a str)]>,
}

impl<'a> From<DeriveOption<'a>> for Token<'a> {
    fn from(value: DeriveOption<'a>) -> Self {
        Self::new(
            value.name.unwrap_or(concat!(module_path!(), column!())),
            value.labels.unwrap_or_default(),
        )
    }
}
