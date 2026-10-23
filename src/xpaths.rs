//! The XPath expressions of upstream `xpaths.py`, as Rust selector functions over the tree.
//!
//! Upstream compiles each expression once and evaluates it against a context element; a selector
//! here is the hand-written equivalent of one such expression.

pub(crate) mod select;

use std::sync::LazyLock;

use regex::Regex;

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use select::{
    attr, descendants, first_among_siblings, first_attr, first_descendant, is_html, re_test,
    translate, union,
};

/// Evaluates one XPath expression against a context node: the matches in document order, the
/// context node never among them.
pub type Selector = fn(&Tree, NodeId, &Call<'_>) -> Result<Vec<NodeId>, ExtractError>;

/// Joins XPath alternatives with `|`; `None` for an empty list, where upstream raises `ValueError`.
// upstream: xpaths.py::_alt
pub fn alt(tokens: &[&str]) -> Option<String> {
    (!tokens.is_empty()).then(|| tokens.join("|"))
}

// A pattern is a literal that compiles (`patterns_compile` forces each one); `None` would make
// `re_test` false instead of panicking.
fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// Declares a lazily compiled `re:test` pattern.
macro_rules! pattern {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Option<Regex>> = LazyLock::new(|| compile(&$pattern));
    };
}

/// Builds a pattern from an upstream token tuple, through `alt`.
fn token_pattern(tokens: &[&str]) -> Option<Regex> {
    compile(&alt(tokens)?)
}

// 1. CONTENT

const ARTICLE_CONTENT_ID_TOKENS: &[&str] = &[
    "(?:entry|article|art)-content",
    "article__content",
    "article(?:-|__)?body",
    "articleBody",
    "body-text",
];
const ARTICLE_CONTENT_CLASS_TOKENS: &[&str] = &[
    "post[-_]text",
    "post-body",
    "post-?entry",
    "post[-_]?content",
    "postContent",
    "post_inner_wrapper",
    "article-?text",
    "articleText",
    "(?:entry|page|text|article|art)-content",
    "article__content",
    "article(?:-|__)?body",
    "articleBody",
    "ArticleContent",
    "body-text",
    "article__container",
];
const STORY_ID_TOKENS: &[&str] = &["^primary", "story-body"];
const STORY_CLASS_TOKENS: &[&str] = &[
    "^article ",
    "post-bodycopy",
    "story-?content",
    "(?:theme|blog|section|single)-content",
    "single-post",
    "main-column",
    "wpb_text_column",
    "story-body",
    "field-body",
];
const MAIN_CONTENT_ID_TOKENS: &[&str] = &["content-main", "content-body", "contentBody"];
const MAIN_CONTENT_CLASS_TOKENS: &[&str] = &["content[-_]main", "content(?:-|__)body"];

static ARTICLE_CONTENT_ID: LazyLock<Option<Regex>> =
    LazyLock::new(|| token_pattern(ARTICLE_CONTENT_ID_TOKENS));
static ARTICLE_CONTENT_CLASS: LazyLock<Option<Regex>> =
    LazyLock::new(|| token_pattern(ARTICLE_CONTENT_CLASS_TOKENS));
static STORY_ID: LazyLock<Option<Regex>> = LazyLock::new(|| token_pattern(STORY_ID_TOKENS));
static STORY_CLASS: LazyLock<Option<Regex>> = LazyLock::new(|| token_pattern(STORY_CLASS_TOKENS));
static MAIN_CONTENT_ID: LazyLock<Option<Regex>> =
    LazyLock::new(|| token_pattern(MAIN_CONTENT_ID_TOKENS));
static MAIN_CONTENT_CLASS: LazyLock<Option<Regex>> =
    LazyLock::new(|| token_pattern(MAIN_CONTENT_CLASS_TOKENS));
// `re:test(@class, 'fulltext', 'i')`: no `i` in the pattern, so `ignore_case_i` has nothing to do.
pattern!(FULLTEXT, "(?i)fulltext");

/// The content-area selectors, tried in order until one finds the main content.
// upstream: xpaths.py::BODY_XPATH
pub const BODY_XPATH: &[Selector] = &[
    body_xpath_0,
    body_xpath_1,
    body_xpath_2,
    body_xpath_3,
    body_xpath_4,
];

// upstream: xpaths.py::BODY_XPATH
fn body_xpath_0(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    first_among_siblings(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["article", "div", "main", "section"]) && {
            let (id_attr, class) = (attr(tree, id, "id"), attr(tree, id, "class"));
            class == "post"
                || class == "entry"
                || attr(tree, id, "itemprop") == "articleBody"
                || id_attr == "articleContent"
                || re_test(id_attr, &ARTICLE_CONTENT_ID)
                || re_test(class, &ARTICLE_CONTENT_CLASS)
        }
    })
}

// upstream: xpaths.py::BODY_XPATH
fn body_xpath_1(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    let first = first_descendant(tree, ctx, call, |tree, id| is_html(tree, id, &["article"]))?;
    Ok(first.into_iter().collect())
}

// upstream: xpaths.py::BODY_XPATH
fn body_xpath_2(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    let first = first_descendant(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["article", "div", "main", "section"]) && {
            let (id_attr, class) = (attr(tree, id, "id"), attr(tree, id, "class"));
            attr(tree, id, "role") == "article"
                || id_attr == "article"
                || id_attr == "story"
                || ["postarea", "art-postcontent", "text", "cell", "story"].contains(&class)
                || re_test(id_attr, &STORY_ID)
                || re_test(class, &FULLTEXT)
                || re_test(class, &STORY_CLASS)
        }
    })?;
    Ok(first.into_iter().collect())
}

// upstream: xpaths.py::BODY_XPATH
fn body_xpath_3(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    let first = first_descendant(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["article", "div", "main", "section"]) && {
            let (id_attr, class) = (attr(tree, id, "id"), attr(tree, id, "class"));
            id_attr == "content"
                || class == "content"
                || re_test(id_attr, &MAIN_CONTENT_ID)
                || re_test(class, &MAIN_CONTENT_CLASS)
                || translate(id_attr, "CM", "cm").contains("main-content")
                || translate(class, "CM", "cm").contains("main-content")
                || translate(class, "CP", "cp").contains("page-content")
        }
    })?;
    Ok(first.into_iter().collect())
}

// upstream: xpaths.py::BODY_XPATH
fn body_xpath_4(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    let section = first_descendant(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["article", "div", "section"])
            && (attr(tree, id, "class").starts_with("main")
                || attr(tree, id, "id").starts_with("main")
                || attr(tree, id, "role").starts_with("main"))
    })?;
    let main = first_descendant(tree, ctx, call, |tree, id| is_html(tree, id, &["main"]))?;
    let parts: Vec<NodeId> = section.into_iter().chain(main).collect();
    union(tree, ctx, call, &parts)
}

// 2. COMMENTS

pattern!(COMMENT_LIST, "comment-?list");
pattern!(COMMENT_PAGE, "comment-page|comments-content|post-comments");
pattern!(COMMENT_PREFIX, "^comment[s-]");
pattern!(COMMENTS_PREFIX, "^Comments|article-comments");
pattern!(COMMENT_SERVICE_ID, "^(?:comol|disqus_thread|dsq-comments)");

/// The selectors of the comment sections, tried in order.
// upstream: xpaths.py::COMMENTS_XPATH
pub const COMMENTS_XPATH: &[Selector] = &[
    comments_xpath_0,
    comments_xpath_1,
    comments_xpath_2,
    comments_xpath_3,
];

// upstream: xpaths.py::COMMENTS_XPATH
fn comments_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "list", "section"])
            && (re_test(first_attr(tree, id, &["id", "class"]), &COMMENT_LIST)
                || re_test(attr(tree, id, "class"), &COMMENT_PAGE))
    })
}

// upstream: xpaths.py::COMMENTS_XPATH
fn comments_xpath_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "section", "list"])
            && (re_test(first_attr(tree, id, &["id", "class"]), &COMMENT_PREFIX)
                || re_test(attr(tree, id, "class"), &COMMENTS_PREFIX))
    })
}

// upstream: xpaths.py::COMMENTS_XPATH
fn comments_xpath_2(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "section", "list"])
            && re_test(attr(tree, id, "id"), &COMMENT_SERVICE_ID)
    })
}

// upstream: xpaths.py::COMMENTS_XPATH
fn comments_xpath_3(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "section"])
            && (attr(tree, id, "id").starts_with("social")
                || attr(tree, id, "class").contains("comment"))
    })
}

// upstream: xpaths.py::_COMMENTS_ID
// `[Cc]omment(?!ary)|comol`; Rust `regex` has no lookahead, so `(?!ary)` is spelled out: "comment"
// not followed by "ary", at the end of the string included.
const COMMENTS_ID: &str = "[Cc]omment(?:$|[^a]|a$|a[^r]|ar$|ar[^y])|comol";
// upstream: xpaths.py::_COMMENTS_CLASS
// `^[Cc]omment(?!ary)|(?:article|post)-comments`, with the lookahead spelled out as above.
const COMMENTS_CLASS: &str = "^[Cc]omment(?:$|[^a]|a$|a[^r]|ar$|ar[^y])|(?:article|post)-comments";

pattern!(
    REMOVE_COMMENTS_ID,
    format!("^(?:{COMMENTS_ID}|disqus_thread|dsq-comments)")
);
pattern!(REMOVE_COMMENTS_LIST_ID, format!("^(?:{COMMENTS_ID})"));
pattern!(REMOVE_COMMENTS_CLASS, COMMENTS_CLASS);

/// Comment containers to remove from the tree.
// upstream: xpaths.py::REMOVE_COMMENTS_XPATH
pub const REMOVE_COMMENTS_XPATH: &[Selector] = &[remove_comments_xpath_0];

// upstream: xpaths.py::REMOVE_COMMENTS_XPATH
fn remove_comments_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "list", "section", "details"])
            && (re_test(attr(tree, id, "id"), &REMOVE_COMMENTS_ID)
                || re_test(attr(tree, id, "class"), &REMOVE_COMMENTS_CLASS))
    })
}

/// `REMOVE_COMMENTS_XPATH` plus the `ol` and `ul` comment lists, in one tree walk.
// upstream: xpaths.py::REMOVE_COMMENTS_AND_LISTS_XPATH
pub const REMOVE_COMMENTS_AND_LISTS_XPATH: &[Selector] = &[remove_comments_and_lists_xpath_0];

// upstream: xpaths.py::REMOVE_COMMENTS_AND_LISTS_XPATH
fn remove_comments_and_lists_xpath_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["div", "list", "section", "details", "ol", "ul"])
            && (re_test(attr(tree, id, "id"), &REMOVE_COMMENTS_ID)
                || re_test(attr(tree, id, "class"), &REMOVE_COMMENTS_CLASS))
            && (!is_html(tree, id, &["ol", "ul"])
                || re_test(attr(tree, id, "id"), &REMOVE_COMMENTS_LIST_ID)
                || re_test(attr(tree, id, "class"), &REMOVE_COMMENTS_CLASS))
    })
}

// 3. RAW TREE PRUNE

pattern!(
    MVP_POST_ADD_OR_SCROLL,
    "mvp-post-add-(?:box|wrap)|infinite-?scroll"
);
pattern!(MVP_POST_ADD, "mvp-post-add-(?:box|wrap)");

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

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
