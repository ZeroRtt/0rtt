use std::sync::Arc;

use crossbeam::channel::Sender;
use parking_lot::Mutex;

use crate::executor::tree::ScopeTree;

enum Task {}

/// A general-purpose thread pool for scheduling tasks that poll futures to completion.
pub struct ThreadPool {
    sender: Sender<Task>,
    scope_tree: Arc<Mutex<ScopeTree>>,
}
