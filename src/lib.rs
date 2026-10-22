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
// No caller until `utils` and the extraction modules land; the crate's own tests are its only users.
#[allow(dead_code, unused_imports)]
mod pystr;
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
mod settings;
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
mod utils;

pub use cancel::{Call, CancelToken};
pub use error::{ExtractError, Rejection};

/// Crate internals for the crate's own integration tests; unstable, not public API.
#[cfg(feature = "__internals")]
#[doc(hidden)]
pub mod __internals {
    pub use crate::dom::{
        DumpMode, LiveIter, Node, NodeId, Ns, ParseLimits, Tree, dump, parse_document,
        parse_fragment,
    };
    pub use crate::settings::{DEFAULT_MAX_NODES, MAX_FILE_SIZE, MIN_FILE_SIZE};
    pub use crate::utils::{HtmlInput, decode_file, detect_encoding, load_html};
}
