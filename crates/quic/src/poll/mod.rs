mod conn;
pub(crate) mod utils;

mod events;
pub use events::*;

mod errors;
pub use errors::*;

mod group;
pub use group::*;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod server;
