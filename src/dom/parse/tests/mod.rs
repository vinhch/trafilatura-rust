mod emitter;
mod fromstring;
mod limits;
mod rules;
mod tables;

use super::*;
use crate::error::{Rejection, TreeLimit};

fn limited(max: usize) -> ParseLimits {
    ParseLimits {
        max_nodes: Some(max),
        ..ParseLimits::default()
    }
}

fn attrs(count: usize) -> String {
    (0..count).map(|i| format!(" a{i}")).collect()
}

/// `<x a0 a1 … a{count-1}>`, every name distinct.
fn tag(count: usize) -> String {
    format!("<x{}>", attrs(count))
}

/// `<{name} a0 .. a998 u{unique}>`: 1,000 attributes, so Noah's Ark keeps entries of one name apart
/// when `unique` differs.
fn heavy_tag(name: &str, unique: usize) -> String {
    format!("<{name}{} u{unique}>", attrs(MAX_TAG_ATTRIBUTES - 1))
}

const TOO_LARGE: ExtractError =
    ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes));
