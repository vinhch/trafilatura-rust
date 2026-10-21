//! Web page main-text and metadata extraction, ported from the Python
//! [trafilatura](https://github.com/adbar/trafilatura) library.
//!
//! The upstream release this crate follows is recorded in `Cargo.toml` under
//! `[package.metadata.upstream]`.

mod cancel;
// The tree has no caller outside the `__internals` feature until the pipeline lands.
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
mod dom;
mod error;

pub use cancel::{Call, CancelToken};
pub use error::{ExtractError, Rejection};

/// Crate internals for the crate's own integration tests; unstable, not public API.
#[cfg(feature = "__internals")]
#[doc(hidden)]
pub mod __internals {
    pub use crate::dom::{
        DumpMode, Node, NodeId, Ns, ParseLimits, Tree, dump, parse_document, parse_fragment,
    };
}
