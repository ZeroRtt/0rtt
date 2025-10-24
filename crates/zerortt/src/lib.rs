//! Poll api for masive [`quiche::Connection`].
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use zerortt_api::*;
pub use zerortt_poll::*;

#[cfg(feature = "futures")]
#[cfg_attr(docsrs, doc(cfg(feature = "futures")))]
pub use zerortt_futures as futures;

#[cfg(feature = "mio")]
#[cfg_attr(docsrs, doc(cfg(feature = "mio")))]
pub use zerortt_mio as mio;
