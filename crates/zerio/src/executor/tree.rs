use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::concurrency::{ScopeKey, TasKey};

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
