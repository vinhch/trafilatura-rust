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
use crate::pystr::PY_SPACE_CLASS;
use select::{
    attr, descendants, first_among_siblings, first_attr, first_descendant, has_attr, is_html,
    re_test, translate, union,
};

/// Evaluates one XPath expression against a context node: the matches in document order, the
/// context node never among them.
pub type Selector = fn(&Tree, NodeId, &Call<'_>) -> Result<Vec<NodeId>, ExtractError>;

/// Joins XPath alternatives with `|`; `None` for an empty list, where upstream raises `ValueError`.
// upstream: xpaths.py::_alt
pub fn alt(tokens: &[&str]) -> Option<String> {
    (!tokens.is_empty()).then(|| tokens.join("|"))
}

// A pattern is a literal that compiles (`every_pattern_compiles` forces each one); `None` would make
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

static OVERALL_CONSENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| token_pattern(CONSENT_ID_CLASS_TOKENS));
static OVERALL_ID: LazyLock<Option<Regex>> =
    LazyLock::new(|| grouped_pattern(OVERALL_DISCARD_BOTH_TOKENS, OVERALL_DISCARD_ID_TOKENS));
static OVERALL_CLASS: LazyLock<Option<Regex>> =
    LazyLock::new(|| grouped_pattern(OVERALL_DISCARD_BOTH_TOKENS, OVERALL_DISCARD_CLASS_TOKENS));
pattern!(OVERALL_HIDDEN_ID, "reader-comments|akismet");
pattern!(
    OVERALL_HIDDEN_CLASS,
    "^hide-|comments-title|nocomments|-reply-|message|akismet|suggest-links|-hide-|hide-print| hidden| hide|noprint|notloaded"
);

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

// `\s` is Python's whitespace set. `bottom` must start or end a token; `link` is a whole token.
pattern!(
    PRECISION_BOTTOM,
    format!("(^|[{PY_SPACE_CLASS}])bottom|bottom([{PY_SPACE_CLASS}]|$)")
);
pattern!(
    PRECISION_LINK,
    format!("(^|[{PY_SPACE_CLASS}])link([{PY_SPACE_CLASS}]|$)")
);

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

pattern!(
    COMMENTS_DISCARD_CLASS,
    "comments-title|nocomments|-reply-|message|signin"
);
pattern!(COMMENTS_DISCARD_REPLY, "^reply-|akismet");

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

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
