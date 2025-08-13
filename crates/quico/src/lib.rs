//! Poll for readiness events on masive quiche connecitons.
#![cfg_attr(docsrs, feature(doc_cfg))]

mod utils;

mod errors;
pub use errors::*;

mod conn;
mod readiness;

mod events;
pub use events::*;

mod poll;
pub use poll::*;

mod validator;
pub use validator::*;
