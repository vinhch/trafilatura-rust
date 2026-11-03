//! The raw-tree prune and discard selectors of upstream `xpaths.py` (sections 3 and 4).

use std::sync::LazyLock;

use regex::Regex;

use super::select::{
    attr, descendants, first_attr, first_descendant, has_attr, is_html, re_test, translate,
};
use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;

use super::{
    COMMENTS_DISCARD_CLASS, COMMENTS_DISCARD_REPLY, MVP_POST_ADD, MVP_POST_ADD_OR_SCROLL,
    OVERALL_HIDDEN_CLASS, OVERALL_HIDDEN_ID, PRECISION_BOTTOM, PRECISION_LINK, Selector,
    token_pattern,
};

/// Raw-tree prune, kept out of `OVERALL_DISCARD_XPATH` because its over-pruning guard would revert
/// it.
// upstream: xpaths.py::RAW_TREE_PRUNE_XPATH
pub const RAW_TREE_PRUNE_XPATH: &[Selector] = &[raw_tree_prune_xpath_0, raw_tree_prune_xpath_1];

// Infinite-scroll containers holding whole follow-up articles.
// upstream: xpaths.py::RAW_TREE_PRUNE_XPATH
fn raw_tree_prune_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let candidates = descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "section", "aside"])
            && (re_test(attr(tree, id, "id"), &MVP_POST_ADD_OR_SCROLL)
                || re_test(attr(tree, id, "class"), &MVP_POST_ADD_OR_SCROLL))
    })?;
    // The `[not(.//h1) or ...]` predicate walks the subtree, so it runs as a second pass.
    let mut kept = Vec::with_capacity(candidates.len());
    for id in candidates {
        let is_mvp = re_test(attr(tree, id, "id"), &MVP_POST_ADD)
            || re_test(attr(tree, id, "class"), &MVP_POST_ADD);
        if is_mvp
            || first_descendant(tree, id, call, |tree, h| is_html(tree, h, &["h1"]))?.is_none()
        {
            kept.push(id);
        }
    }
    Ok(kept)
}

// upstream: xpaths.py::RAW_TREE_PRUNE_XPATH
fn raw_tree_prune_xpath_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "section", "aside"])
            && attr(tree, id, "class").contains("elementor-share-buttons")
    })
}

// 4. DISCARD

// OVERALL_DISCARD_XPATH boilerplate token vocabulary, grouped by concept; `_LEGACY_SITE_*` are the
// single-site tokens.
const SHARE_SOCIAL_ID_CLASS_TOKENS: &[&str] = &["^shar", "social", "viral"];
const NEWSLETTER_ID_CLASS_TOKENS: &[&str] = &["newsletter", "syndication"];
const CONSENT_ID_CLASS_TOKENS: &[&str] = &["cookie"];
const TAGS_ID_CLASS_TOKENS: &[&str] = &["tags"];
const UI_CHROME_ID_CLASS_TOKENS: &[&str] = &["sidebar", "banner", "bread-?crumb", "button"];
const AUTHOR_ID_CLASS_TOKENS: &[&str] = &["author"];

const LEGACY_SITE_ID_TOKENS: &[&str] = &["^(?:jp-|dpsp-content)", "bmdh"];
const FOOTER_ID_TOKENS: &[&str] = &["footer", "Footer"];
const SHARE_ID_TOKENS: &[&str] = &["share", "Share"];
const NAV_ID_TOKENS: &[&str] = &["nav", "Nav", "menu"];
const RELATED_ID_TOKENS: &[&str] = &["related"];
const UI_CHROME_ID_TOKENS: &[&str] = &["message-container"];
const PAYWALL_ID_TOKENS: &[&str] = &["premium"];
const AD_ID_TOKENS: &[&str] = &["^ad-"];

const NAV_CLASS_TOKENS: &[&str] = &[
    "^(?:nav|post-nav|ZendeskForm)",
    "subnav",
    "avigation",
    "navbar",
    "navbox",
    "menu",
    "bar",
];
const AD_CLASS_TOKENS: &[&str] = &[
    " ad ",
    "-ad-",
    "(?:^| )ad-",
    "dfp[Aa]d",
    "outbrain",
    "taboola",
    "criteo",
    "paid-?content",
    "widget",
];
const FOOTER_CLASS_TOKENS: &[&str] = &["footer", "Footer"];
const AUTHOR_CLASS_TOKENS: &[&str] = &["byline", "Byline"];
const SHARE_CLASS_TOKENS: &[&str] = &["share-", "sociable", "embedded", "embed"];
const TAGS_CLASS_TOKENS: &[&str] = &["tag-list"];
const CONSENT_CLASS_TOKENS: &[&str] = &["consent", "modal-content", "permission"];
const RELATED_CLASS_TOKENS: &[&str] = &["elated", "next-", "-stories", "most-popular", "more-on"];
const UI_META_CLASS_TOKENS: &[&str] = &[
    "meta",
    "rating",
    "attachment",
    "timestamp",
    "user-info",
    "user-profile",
    "-icon",
    "article-infos",
    "message-container",
    "slide",
    "viewport",
    "overlay",
];
const MISC_CLASS_TOKENS: &[&str] = &["options", "expand", "obfuscated", "blurred"];
const LEGACY_SITE_CLASS_TOKENS: &[&str] = &["mol-factbox", "yin", "zlylin", "nfoline"];

// Tokens checked on both attributes, each per attribute: `re:test(@id|@class, ...)` reads only the
// source-first attribute.
const OVERALL_DISCARD_BOTH_TOKENS: &[&[&str]] = &[
    SHARE_SOCIAL_ID_CLASS_TOKENS,
    NEWSLETTER_ID_CLASS_TOKENS,
    TAGS_ID_CLASS_TOKENS,
    UI_CHROME_ID_CLASS_TOKENS,
    AUTHOR_ID_CLASS_TOKENS,
];
const OVERALL_DISCARD_ID_TOKENS: &[&[&str]] = &[
    LEGACY_SITE_ID_TOKENS,
    FOOTER_ID_TOKENS,
    SHARE_ID_TOKENS,
    NAV_ID_TOKENS,
    RELATED_ID_TOKENS,
    UI_CHROME_ID_TOKENS,
    PAYWALL_ID_TOKENS,
    AD_ID_TOKENS,
];
const OVERALL_DISCARD_CLASS_TOKENS: &[&[&str]] = &[
    NAV_CLASS_TOKENS,
    AD_CLASS_TOKENS,
    FOOTER_CLASS_TOKENS,
    AUTHOR_CLASS_TOKENS,
    SHARE_CLASS_TOKENS,
    TAGS_CLASS_TOKENS,
    CONSENT_CLASS_TOKENS,
    RELATED_CLASS_TOKENS,
    UI_META_CLASS_TOKENS,
    MISC_CLASS_TOKENS,
    LEGACY_SITE_CLASS_TOKENS,
];

/// Builds a pattern from token groups joined in order, as Python's tuple `+` does, through `alt`.
fn grouped_pattern(first: &[&[&str]], second: &[&[&str]]) -> Option<Regex> {
    token_pattern(&[first, second].concat().concat())
}

pub(super) static OVERALL_CONSENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| token_pattern(CONSENT_ID_CLASS_TOKENS));
pub(super) static OVERALL_ID: LazyLock<Option<Regex>> =
    LazyLock::new(|| grouped_pattern(OVERALL_DISCARD_BOTH_TOKENS, OVERALL_DISCARD_ID_TOKENS));
pub(super) static OVERALL_CLASS: LazyLock<Option<Regex>> =
    LazyLock::new(|| grouped_pattern(OVERALL_DISCARD_BOTH_TOKENS, OVERALL_DISCARD_CLASS_TOKENS));

/// Boilerplate containers to discard before extraction.
// upstream: xpaths.py::OVERALL_DISCARD_XPATH
pub const OVERALL_DISCARD_XPATH: &[Selector] = &[overall_discard_xpath_0, overall_discard_xpath_1];

// upstream: xpaths.py::OVERALL_DISCARD_XPATH
fn overall_discard_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "item", "list", "p", "section", "span"])
            && (has_attr(tree, id, "data-lp-replacement-content")
                || translate(attr(tree, id, "role"), "N", "n").contains("nav")
                || attr(tree, id, "data-component").contains("MostPopularStories")
                || re_test(first_attr(tree, id, &["id", "class"]), &OVERALL_CONSENT)
                || re_test(attr(tree, id, "id"), &OVERALL_ID)
                || re_test(attr(tree, id, "class"), &OVERALL_CLASS))
    })
}

// upstream: xpaths.py::OVERALL_DISCARD_XPATH
fn overall_discard_xpath_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        let style = attr(tree, id, "style");
        attr(tree, id, "class") == "comments-title"
            || first_attr(tree, id, &["id", "class"]).starts_with("reply-")
            // `re:test(@id|@style, 'hidden')`: the pattern is a plain substring.
            || first_attr(tree, id, &["id", "style"]).contains("hidden")
            || style.contains("display:none")
            || style.contains("display: none")
            || re_test(attr(tree, id, "id"), &OVERALL_HIDDEN_ID)
            || re_test(attr(tree, id, "class"), &OVERALL_HIDDEN_CLASS)
            || attr(tree, id, "aria-hidden") == "true"
    })
}

/// Teaser blocks, discarded for precision.
// upstream: xpaths.py::TEASER_DISCARD_XPATH
pub const TEASER_DISCARD_XPATH: &[Selector] = &[teaser_discard_xpath_0];

// upstream: xpaths.py::TEASER_DISCARD_XPATH
fn teaser_discard_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "item", "list", "p", "section", "span"])
            && (translate(attr(tree, id, "id"), "T", "t").contains("teaser")
                || translate(attr(tree, id, "class"), "T", "t").contains("teaser"))
    })
}

/// Header elements and layout chrome, discarded for precision.
// upstream: xpaths.py::PRECISION_DISCARD_XPATH
pub const PRECISION_DISCARD_XPATH: &[Selector] =
    &[precision_discard_xpath_0, precision_discard_xpath_1];

// upstream: xpaths.py::PRECISION_DISCARD_XPATH
fn precision_discard_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| is_html(tree, id, &["header"]))
}

// upstream: xpaths.py::PRECISION_DISCARD_XPATH
fn precision_discard_xpath_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "item", "list", "p", "section", "span"]) && {
            let id_or_class = first_attr(tree, id, &["id", "class"]);
            re_test(id_or_class, &PRECISION_BOTTOM)
                || re_test(id_or_class, &PRECISION_LINK)
                || attr(tree, id, "style").contains("border")
        }
    })
}

/// Image caption containers.
// upstream: xpaths.py::DISCARD_IMAGE_ELEMENTS
pub const DISCARD_IMAGE_ELEMENTS: &[Selector] = &[discard_image_elements_0];

// upstream: xpaths.py::DISCARD_IMAGE_ELEMENTS
fn discard_image_elements_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "item", "list", "p", "section", "span"])
            && (attr(tree, id, "id").contains("caption")
                || attr(tree, id, "class").contains("caption"))
    })
}

/// Elements to discard from the comments tree.
// upstream: xpaths.py::COMMENTS_DISCARD_XPATH
pub const COMMENTS_DISCARD_XPATH: &[Selector] = &[
    comments_discard_xpath_0,
    comments_discard_xpath_1,
    comments_discard_xpath_2,
];

// upstream: xpaths.py::COMMENTS_DISCARD_XPATH
fn comments_discard_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "section"]) && attr(tree, id, "id").starts_with("respond")
    })
}

// upstream: xpaths.py::COMMENTS_DISCARD_XPATH
fn comments_discard_xpath_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    // `.//cite|.//quote`: both name tests in one walk.
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["cite", "quote"])
    })
}

// upstream: xpaths.py::COMMENTS_DISCARD_XPATH
fn comments_discard_xpath_2(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        attr(tree, id, "class") == "comments-title"
            || attr(tree, id, "style").contains("display:none")
            || re_test(attr(tree, id, "class"), &COMMENTS_DISCARD_CLASS)
            || re_test(
                first_attr(tree, id, &["id", "class"]),
                &COMMENTS_DISCARD_REPLY,
            )
    })
}
