//! Web page main-text and metadata extraction, ported from the Python
//! [trafilatura](https://github.com/adbar/trafilatura) library.
//!
//! The upstream release this crate follows is recorded in `Cargo.toml` under
//! `[package.metadata.upstream]`.

mod cancel;
// No caller outside the tests until the extraction modules land (`context`, `settings`, `utils`,
// `pystr` and `xpaths` items are `pub` in a private module and re-exported only as far as the tests need).
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
mod html_processing;
#[allow(dead_code)]
mod main_extractor;
#[allow(dead_code)]
mod pyhtml;
#[allow(dead_code)]
mod pystr;
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

pub use cancel::{Call, CancelToken};
pub use error::{ExtractError, Rejection};

/// Crate internals for the crate's own integration tests; unstable, not public API.
#[cfg(feature = "__internals")]
#[doc(hidden)]
pub mod __internals {
    pub use crate::context::Ctx;
    pub use crate::dom::{
        DumpMode, LiveIter, LocalName, Node, NodeId, Ns, ParseLimits, Tree, dump, dump_from,
        parse_document, parse_fragment,
    };
    pub use crate::html_processing::{
        CODE_INDICATORS, REND_TAG_MAPPING, collect_link_info, convert_deletions, convert_details,
        convert_headings, convert_line_breaks, convert_link, convert_lists, convert_quotes,
        convert_tags, delete_by_link_density, handle_textnode, is_paragraph_listing,
        link_density_test, link_density_test_tables, process_node, prune_html,
        prune_unwanted_nodes, recover_math, tree_cleaning, tree_cleaning_with,
    };
    pub use crate::main_extractor::{
        extract_comments, extract_content, handle_code_blocks, handle_formatting, handle_image,
        handle_lists, handle_other_elements, handle_paragraphs, handle_quotes, handle_table,
        handle_textelem, handle_titles, process_nested_elements, prune_unwanted_sections,
        recover_wild_text, span,
    };
    pub use crate::pyhtml::unescape;
    pub use crate::settings::{
        Config, DEDUPE_SCAN_CAP, DEFAULT_MAX_NODES, Extractor, Focus, Format, MANUALLY_CLEANED,
        MANUALLY_STRIPPED, MAX_FILE_SIZE, MIN_DUPLICATE_LENGTH, MIN_FILE_SIZE, TAG_CATALOG,
    };
    pub use crate::utils::{
        HtmlInput, decode_file, detect_encoding, load_html, safe_base_url, safe_relative_url,
        sanitize, textfilter, trim,
    };
    pub use crate::xml::{delete_element, md_link};
    pub use crate::xpaths::{
        BODY_XPATH, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, Selector, alt,
    };
}
