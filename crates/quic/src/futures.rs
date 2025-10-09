//! Asynchronous Runtime Binding for `QUIC`.

use std::{collections::HashMap, sync::Arc, task::Waker};

use parking_lot::Mutex;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum Event {}

#[allow(unused)]
pub struct Group {
    group: Arc<crate::mio::Group>,
    wakers: Arc<Mutex<HashMap<Event, Waker>>>,
}
