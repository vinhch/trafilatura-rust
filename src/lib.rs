//! Web page main-text and metadata extraction, ported from the Python
//! [trafilatura](https://github.com/adbar/trafilatura) library.
//!
//! The upstream release this crate follows is recorded in `Cargo.toml` under
//! `[package.metadata.upstream]`.
//!
//! [`Options`] holds the settings, with upstream's defaults, and builds an immutable [`Extractor`]
//! that any number of threads share. A call takes an [`Input`] (the page, with its URL and record
//! id) and a [`Call`] that can cancel it or set a deadline. A page upstream rejects gives
//! [`ExtractError::Rejected`].
//!
//! ```
//! use trafilatura_rust::{Call, Extractor, Format, Input, Options};
//!
//! # fn main() -> Result<(), trafilatura_rust::ExtractError> {
//! let html = "<html><body><article><h2>Title</h2>\
//!     <p>A paragraph of the page, with a <a href=\"/more\">link</a> and enough words to keep.</p>\
//!     </article></body></html>";
//! let extractor: Extractor = Options::new().format(Format::Markdown).links(true).build();
//! let input = Input::text(html).url("https://example.org/post");
//! let text = extractor.extract(&input, &Call::default())?;
//! assert!(text.starts_with("## Title"));
//! assert!(text.contains("[link](https://example.org/more)"));
//! # Ok(())
//! # }
//! ```

mod api;
#[allow(dead_code)]
mod baseline;
mod cancel;
// Items the pipeline reaches only from later phases or from the tests stay `pub` in a private module
// (`context`, `settings`, `utils`, `pystr` and `xpaths` among them) and are re-exported only as far as
// the public API and `__internals` need.
#[allow(dead_code)]
mod context;
#[allow(dead_code)]
mod courlan;
#[allow(dead_code)]
mod deduplication;
// The tree has no caller outside the `__internals` feature until the pipeline lands.
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
mod dom;
mod error;
#[allow(dead_code)]
mod extraction;
#[allow(dead_code)]
mod html_processing;
#[allow(dead_code)]
mod main_extractor;
#[allow(dead_code)]
mod metadata;
#[allow(dead_code)]
mod pyhtml;
#[allow(dead_code)]
mod pyjson;
#[allow(dead_code)]
mod pystr;
#[allow(dead_code)]
mod pyunicodedata;
#[allow(dead_code)]
mod pyurl;
#[allow(dead_code)]
mod settings;
#[allow(dead_code)]
mod utils;
#[allow(dead_code)]
mod xml;
#[allow(dead_code)]
mod xpaths;

pub use api::{Extractor, Format, Input, Options, html2txt};
pub use cancel::{Call, CancelToken};
pub use error::{ExtractError, Rejection};
pub use settings::{Config, Document, Focus};

/// Crate internals for the crate's own integration tests; unstable, not public API.
#[cfg(feature = "__internals")]
#[doc(hidden)]
pub mod __internals {
    pub use crate::baseline::{
        baseline, baseline_tree, basic_cleaning, block_text, build_body, html2txt, html2txt_tree,
    };
    pub use crate::context::Ctx;
    pub use crate::dom::{
        DumpMode, LiveIter, LocalName, Node, NodeId, Ns, ParseLimits, Tree, dump, dump_from,
        parse_document, parse_fragment,
    };
    pub use crate::extraction::{
        Source, bare_extraction, determine_returnstring, internal_extraction,
    };
    pub use crate::html_processing::{
        CODE_INDICATORS, HTML_TAG_MAPPING, REND_TAG_MAPPING, build_html_output, collect_link_info,
        convert_deletions, convert_details, convert_headings, convert_line_breaks, convert_link,
        convert_lists, convert_quotes, convert_tags, convert_to_html, delete_by_link_density,
        handle_textnode, is_paragraph_listing, link_density_test, link_density_test_tables,
        process_node, prune_html, prune_unwanted_nodes, recover_math, tree_cleaning,
        tree_cleaning_with,
    };
    pub use crate::main_extractor::{
        extract_comments, extract_content, handle_code_blocks, handle_formatting, handle_image,
        handle_lists, handle_other_elements, handle_paragraphs, handle_quotes, handle_table,
        handle_textelem, handle_titles, process_nested_elements, prune_unwanted_sections,
        recover_wild_text, span,
    };
    pub use crate::pyhtml::unescape;
    pub use crate::pyjson::{Json, JsonRef, Kind, dumps_str, loads};
    pub use crate::pystr::{is_alpha, repr};
    pub use crate::pyunicodedata::normalize_nfc;
    pub use crate::settings::{
        Config, DEDUPE_SCAN_CAP, DEFAULT_MAX_NODES, Document, Extractor, Focus, Format,
        MANUALLY_CLEANED, MANUALLY_STRIPPED, MAX_FILE_SIZE, MIN_DUPLICATE_LENGTH, MIN_FILE_SIZE,
        TAG_CATALOG,
    };
    pub use crate::utils::{
        HtmlInput, decode_file, detect_encoding, load_html, normalize_unicode, safe_base_url,
        safe_relative_url, sanitize, textfilter, trim,
    };
    pub use crate::xml::{
        delete_element, md_link, merge_with_parent, remove_empty_elements, replace_element_text,
        strip_double_tags, xml_to_text,
    };
    pub use crate::xpaths::{
        BODY_XPATH, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, Selector, alt,
    };
}
