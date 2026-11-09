mod cleaning;
mod convert;
mod html_output;
mod text;
mod upstream;

use std::borrow::Cow;
use std::sync::Arc;

use super::*;
use crate::dom::{
    DumpMode, LocalName, Ns, ParseLimits, dump, dump_from, parse_document, parse_fragment,
};
use crate::error::Rejection;
use crate::html_processing::convert::{
    CONVERSION_TAGS, convert_deletions, convert_details, convert_headings, convert_line_breaks,
    convert_link, convert_lists, convert_quotes, convert_to_html,
};
use crate::settings::Extractor;
use crate::settings::{Config, Format};
use crate::xpaths::{
    COMMENTS_DISCARD_XPATH, DISCARD_IMAGE_ELEMENTS, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH,
    RAW_TREE_PRUNE_XPATH, REMOVE_COMMENTS_AND_LISTS_XPATH, REMOVE_COMMENTS_XPATH,
    TEASER_DISCARD_XPATH,
};

fn options() -> Extractor {
    Extractor::new(
        Config::default(),
        Format::Python,
        false,
        false,
        None,
        false,
        false,
        Default::default(),
        Default::default(),
    )
}

fn attrs(pairs: &[(&str, &str)]) -> Vec<(LocalName, Arc<str>)> {
    pairs
        .iter()
        .map(|(key, value)| (LocalName::from(*key), Arc::from(*value)))
        .collect()
}
