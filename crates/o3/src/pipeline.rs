use std::{borrow::Borrow, collections::HashMap, hash::Hash};

pub trait Pipeline {
    type From: Eq + Hash + Clone;
    type To: Eq + Hash + Clone;

    fn from_key(&self) -> &Self::From;
    fn to_key(&self) -> &Self::To;
}

/// Pipeline set.
pub struct Pipelines<P: Pipeline> {
    id_next: u32,
    from: HashMap<P::From, u32>,
    to: HashMap<P::To, u32>,
    pipelines: HashMap<u32, P>,
}

impl<P: Pipeline> Default for Pipelines<P> {
    fn default() -> Self {
        Self {
            id_next: 0,
            from: Default::default(),
            to: Default::default(),
            pipelines: Default::default(),
        }
    }
}

#[allow(unused)]
impl<P> Pipelines<P>
where
    P: Pipeline,
{
    /// Insert new pipeline.
    pub fn insert(&mut self, p: P) -> Option<P> {
        if let Some(id) = self.from.get(p.from_key()) {
            assert!(self.to.contains_key(&p.to_key()));
            return self.pipelines.insert(*id, p);
        }

        loop {
            let id = self.id_next;

            (self.id_next, _) = self.id_next.overflowing_add(1);

            if self.pipelines.contains_key(&id) {
                continue;
            }

            self.from.insert(p.from_key().clone(), id);
            self.to.insert(p.to_key().clone(), id);
            return self.pipelines.insert(id, p);
        }
    }

    /// Get pipeline by from `key`
    pub fn get_by_from_id<Q>(&mut self, k: &Q) -> Option<&mut P>
    where
        Q: ?Sized,
        P::From: Borrow<Q>,
        Q: Hash + Eq,
        // Bounds from impl:
    {
        if let Some(id) = self.from.get(k) {
            return self.pipelines.get_mut(id);
        } else {
            None
        }
    }

    /// Remove pipeline.
    pub fn remove<Q>(&mut self, f: &P::From, t: &P::To) -> Option<P> {
        if let Some(id) = self.from.remove(f) {
            assert_eq!(self.to.remove(t), Some(id));

            return self.pipelines.remove(&id);
        } else {
            None
        }
    }

    /// Get pipeline by from `key`
    pub fn get_by_to_id<Q>(&mut self, k: &Q) -> Option<&mut P>
    where
        Q: ?Sized,
        P::To: Borrow<Q>,
        Q: Hash + Eq,
    {
        if let Some(id) = self.to.get(k) {
            return self.pipelines.get_mut(id);
        } else {
            None
        }
    }
}
