//! Core `poll` feature implementation

mod errors;
pub use errors::*;

pub mod conn;
pub mod deadline;

mod events;
pub use events::*;

#[allow(unused)]
mod poll;
pub use poll::*;
