mod attack;
mod budget;
mod formatting;
mod scanner;
mod tables;

use std::cell::RefCell;

use html5ever::TokenizerResult;
use html5ever::interface::tree_builder::{ElementFlags, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tokenizer::{
    BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::tree_builder::{TreeBuilder, create_element};

use super::sink::{Sink, parse_opts};
use crate::dom::scan::{Pause, TagScanner};

use super::*;

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

/// `html` parsed by html5ever in one piece, without the scanner.
fn whole_tree(html: &str, fragment: bool) -> Tree {
    let call = Call::default();
    let limits = ParseLimits {
        max_nodes: None,
        ..ParseLimits::default()
    };
    let mut parser = if fragment {
        html5ever::parse_fragment(
            new_sink(&limits, &call, true),
            parse_opts(),
            QualName::new(None, ns!(html), local_name!("div")),
            Vec::new(),
            false,
        )
    } else {
        html5ever::parse_document(new_sink(&limits, &call, false), parse_opts())
    };
    parser.process(StrTendril::from_slice(html));
    parser.finish().unwrap()
}

fn parse(html: &str, fragment: bool) -> Result<Tree, ExtractError> {
    let limits = ParseLimits::default();
    let call = Call::default();
    if fragment {
        parse_fragment(html, &limits, &call)
    } else {
        parse_document(html, &limits, &call)
    }
}

/// `<{name} a0 .. a998 u{unique}>`: 1,000 attributes, so Noah's Ark keeps entries of one name apart
/// when `unique` differs.
fn heavy_tag(name: &str, unique: usize) -> String {
    format!("<{name}{} u{unique}>", attrs(MAX_TAG_ATTRIBUTES - 1))
}

/// The abort and the compare work of `html` fed in one piece through the gate with `max_nodes`.
fn compare_outcome(html: &str, max_nodes: usize) -> (Option<ExtractError>, usize) {
    let call = Call::default();
    let driver = Driver::new(new_sink(&limited(max_nodes), &call, false), false);
    driver.process(html);
    let inner = driver.sink().inner.borrow();
    (inner.abort.clone(), inner.compare_work)
}

const TOO_LARGE: ExtractError =
    ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes));
