use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::concurrency::ScopeKey;

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

#[derive(Default)]
struct ScopeNode {
    tasks: HashSet<TasKey>,
    scopes: HashSet<ScopeKey>,
}

#[derive(Default)]
pub(super) struct ScopeTree {
    scope_key_next: usize,
    scopes: HashMap<ScopeKey, ScopeNode>,
}

impl ScopeTree {
    pub fn new() -> Self {
        let mut tree = Self::default();

        tree.scope_key_next = 1;
        tree.scopes
            .insert(ScopeKey(0, Some("MainScope")), Default::default());

        tree
    }
}
