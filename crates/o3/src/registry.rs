use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct QuicRegistry(HashMap<zrquic::Token, HashSet<u64>>);

impl QuicRegistry {
    pub fn register(&mut self, token: zrquic::Token, stream_id: u64) {
        self.0
            .entry(token)
            .or_insert_with(|| {
                let mut set = HashSet::new();
                set.insert(stream_id);
                set
            })
            .insert(stream_id);
    }

    pub fn deregister(&mut self, token: zrquic::Token, stream_id: u64) {
        self.0.get_mut(&token).and_then(|set| {
            set.remove(&stream_id);
            None::<()>
        });
    }

    pub fn close(&mut self, token: zrquic::Token) -> Option<Vec<u64>> {
        self.0.remove(&token).map(|set| set.into_iter().collect())
    }
}
