use std::fmt::Display;

/// A handle reference to one [`Task`] instance.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct TasKey(pub usize, pub Option<&'static str>);

impl Display for TasKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1
            .map(|v| write!(f, "task({})", v))
            .unwrap_or_else(|| write!(f, "task({})", self.0))
    }
}
