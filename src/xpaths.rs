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
    re_test, string_value_is, subtree, translate, union,
};

/// Evaluates one XPath expression against a context node: the matches in document order. An
/// expression that starts with `.//` never matches the context node; one that starts with `//`
/// selects from the document root, so the context node can be among its matches.
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

mod discard;

pub use discard::{
    COMMENTS_DISCARD_XPATH, DISCARD_IMAGE_ELEMENTS, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH,
    RAW_TREE_PRUNE_XPATH, TEASER_DISCARD_XPATH,
};
#[cfg(test)]
use discard::{OVERALL_CLASS, OVERALL_CONSENT, OVERALL_ID};

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

pattern!(OVERALL_HIDDEN_ID, "reader-comments|akismet");
pattern!(
    OVERALL_HIDDEN_CLASS,
    "^hide-|comments-title|nocomments|-reply-|message|akismet|suggest-links|-hide-|hide-print| hidden| hide|noprint|notloaded"
);

// `\s` is Python's whitespace set. `bottom` must start or end a token; `link` is a whole token.
pattern!(
    PRECISION_BOTTOM,
    format!("(^|[{PY_SPACE_CLASS}])bottom|bottom([{PY_SPACE_CLASS}]|$)")
);
pattern!(
    PRECISION_LINK,
    format!("(^|[{PY_SPACE_CLASS}])link([{PY_SPACE_CLASS}]|$)")
);

pattern!(
    COMMENTS_DISCARD_CLASS,
    "comments-title|nocomments|-reply-|message|signin"
);
pattern!(COMMENTS_DISCARD_REPLY, "^reply-|akismet");

// 5. METADATA

// An expression starting with `//` selects from the document root, which is the context element of
// every metadata call, so `subtree` (the context node itself first) is its walk.

pattern!(AUTHOR_NAME_CLASS, "author-?name|AuthorName|authorName");
pattern!(AUTHOR_LOOSE_ID, "author|zuozhe|bianji|xiaobian");
pattern!(
    AUTHOR_LOOSE_CLASS,
    "author|channel-name|zuozhe|bianji|xiaobian|submitted-by|posted-by|journalist-name"
);
pattern!(AUTHOR_ANY_ID, "[Aa]uthor");
pattern!(AUTHOR_ANY_CLASS, "[Aa]uthor|screenname|writer|[Bb]yline");
pattern!(AUTHOR_DISCARD_ID, "^comments|comment-?list|ProductReviews");
pattern!(
    AUTHOR_DISCARD_CLASS,
    "^[Cc]omments|commentlist|comments-list|sidebar|is-hidden|quote|embedly-instagram|article-(?:share|support)|print|category|meta-date|meta-reviewer"
);
pattern!(
    CATEGORIES_DIV_CLASS,
    "^(?:post-?info|post-?meta|meta|entry-meta|entry-info|entry-utility)"
);
pattern!(TAGS_DIV_CLASS, "^(?:tag|postmeta|meta)");
pattern!(
    TITLE_HEADING_CLASS,
    "(?:post-|entry-|article-|post__)title|headline"
);

/// Author elements, most specific selector first.
// upstream: xpaths.py::AUTHOR_XPATHS
pub const AUTHOR_XPATHS: &[Selector] = &[author_xpaths_0, author_xpaths_1, author_xpaths_2];

// upstream: xpaths.py::AUTHOR_XPATHS
fn author_xpaths_0(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["author"])
            || (is_html(
                tree,
                id,
                &["a", "address", "div", "link", "p", "span", "strong"],
            ) && (attr(tree, id, "rel") == "author"
                || attr(tree, id, "id") == "author"
                || attr(tree, id, "class") == "author"
                || attr(tree, id, "itemprop") == "author name"
                // `rel='me'` compares a child element named `rel`, not the attribute.
                || tree.children(id).any(|child| {
                    is_html(tree, child, &["rel"]) && string_value_is(tree, child, "me")
                })
                || ["AuthorCard", "AuthorURL"].contains(&attr(tree, id, "data-testid"))
                || re_test(attr(tree, id, "class"), &AUTHOR_NAME_CLASS)))
    })
}

// upstream: xpaths.py::AUTHOR_XPATHS
fn author_xpaths_1(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["a", "div", "h3", "h4", "p", "span"]) && {
            let (id_attr, class) = (attr(tree, id, "id"), attr(tree, id, "class"));
            ["byline", "username", "byl", "BBL"].contains(&class)
                || attr(tree, id, "itemprop").contains("author")
                || re_test(id_attr, &AUTHOR_LOOSE_ID)
                || re_test(class, &AUTHOR_LOOSE_CLASS)
        }
    })
}

// upstream: xpaths.py::AUTHOR_XPATHS
fn author_xpaths_2(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        attr(tree, id, "data-component").contains("Byline")
            || attr(tree, id, "itemprop").contains("author")
            || re_test(attr(tree, id, "id"), &AUTHOR_ANY_ID)
            || re_test(attr(tree, id, "class"), &AUTHOR_ANY_CLASS)
    })
}

/// Elements dropped before reading an author out of a subtree.
// upstream: xpaths.py::AUTHOR_DISCARD_XPATHS
pub const AUTHOR_DISCARD_XPATHS: &[Selector] = &[author_discard_xpaths_0, author_discard_xpaths_1];

// upstream: xpaths.py::AUTHOR_DISCARD_XPATHS
fn author_discard_xpaths_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["a", "div", "section", "span"]) && {
            let (id_attr, class) = (attr(tree, id, "id"), attr(tree, id, "class"));
            id_attr == "comments"
                || ["comments", "title", "date"].contains(&class)
                || re_test(id_attr, &AUTHOR_DISCARD_ID)
                || re_test(class, &AUTHOR_DISCARD_CLASS)
                || attr(tree, id, "data-component").contains("Figure")
        }
    })
}

// upstream: xpaths.py::AUTHOR_DISCARD_XPATHS
fn author_discard_xpaths_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["time", "figure"])
    })
}

/// `//a[@href]` below an element that satisfies `holder`: a link under several holders matches once.
fn is_link_under(tree: &Tree, id: NodeId, holder: impl Fn(&Tree, NodeId) -> bool) -> bool {
    is_html(tree, id, &["a"])
        && has_attr(tree, id, "href")
        && tree.iterancestors(id, &[]).any(|up| holder(tree, up))
}

/// Links that carry the categories of a page.
// upstream: xpaths.py::CATEGORIES_XPATHS
pub const CATEGORIES_XPATHS: &[Selector] = &[
    categories_xpaths_0,
    categories_xpaths_1,
    categories_xpaths_2,
    categories_xpaths_3,
    categories_xpaths_4,
    categories_xpaths_5,
];

// upstream: xpaths.py::CATEGORIES_XPATHS
fn categories_xpaths_0(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            is_html(tree, up, &["div"])
                && (re_test(attr(tree, up, "class"), &CATEGORIES_DIV_CLASS)
                    || attr(tree, up, "id").starts_with("postpath"))
        })
    })
}

// upstream: xpaths.py::CATEGORIES_XPATHS
fn categories_xpaths_1(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            let class = attr(tree, up, "class");
            is_html(tree, up, &["p"])
                && (class.starts_with("postmeta")
                    || class.starts_with("entry-categories")
                    || class == "postinfo"
                    || attr(tree, up, "id") == "filedunder")
        })
    })
}

// upstream: xpaths.py::CATEGORIES_XPATHS
fn categories_xpaths_2(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            let class = attr(tree, up, "class");
            is_html(tree, up, &["footer"])
                && (class.starts_with("entry-meta") || class.starts_with("entry-footer"))
        })
    })
}

// upstream: xpaths.py::CATEGORIES_XPATHS
fn categories_xpaths_3(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            let class = attr(tree, up, "class");
            is_html(tree, up, &["li", "span"])
                && (["post-category", "postcategory", "entry-category"].contains(&class)
                    || class.contains("cat-links"))
        })
    })
}

// upstream: xpaths.py::CATEGORIES_XPATHS
fn categories_xpaths_4(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            is_html(tree, up, &["header"]) && attr(tree, up, "class") == "entry-header"
        })
    })
}

// upstream: xpaths.py::CATEGORIES_XPATHS
fn categories_xpaths_5(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            is_html(tree, up, &["div"]) && ["row", "tags"].contains(&attr(tree, up, "class"))
        })
    })
}

/// Links that carry the tags of a page.
// upstream: xpaths.py::TAGS_XPATHS
pub const TAGS_XPATHS: &[Selector] = &[tags_xpaths_0, tags_xpaths_1, tags_xpaths_2, tags_xpaths_3];

// upstream: xpaths.py::TAGS_XPATHS
fn tags_xpaths_0(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            is_html(tree, up, &["div"]) && attr(tree, up, "class") == "tags"
        })
    })
}

// upstream: xpaths.py::TAGS_XPATHS
fn tags_xpaths_1(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            is_html(tree, up, &["p"]) && attr(tree, up, "class").starts_with("entry-tags")
        })
    })
}

// upstream: xpaths.py::TAGS_XPATHS
fn tags_xpaths_2(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            let class = attr(tree, up, "class");
            is_html(tree, up, &["div"])
                && (["row", "jp-relatedposts", "entry-utility"].contains(&class)
                    || re_test(class, &TAGS_DIV_CLASS))
        })
    })
}

// upstream: xpaths.py::TAGS_XPATHS
fn tags_xpaths_3(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_link_under(tree, id, |tree, up| {
            let class = attr(tree, up, "class");
            class == "entry-meta" || class.contains("topics") || class.contains("tags-links")
        })
    })
}

/// Title elements, most specific selector first.
// upstream: xpaths.py::TITLE_XPATHS
pub const TITLE_XPATHS: &[Selector] = &[title_xpaths_0, title_xpaths_1, title_xpaths_2];

// upstream: xpaths.py::TITLE_XPATHS
fn title_xpaths_0(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["h1", "h2"])
            && (re_test(attr(tree, id, "class"), &TITLE_HEADING_CLASS)
                || attr(tree, id, "id").contains("headline")
                || attr(tree, id, "itemprop").contains("headline"))
    })
}

// upstream: xpaths.py::TITLE_XPATHS
fn title_xpaths_1(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        ["entry-title", "post-title"].contains(&attr(tree, id, "class"))
    })
}

// upstream: xpaths.py::TITLE_XPATHS
fn title_xpaths_2(tree: &Tree, ctx: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    subtree(tree, ctx, call, |tree, id| {
        is_html(tree, id, &["h1", "h2", "h3"])
            && (attr(tree, id, "class").contains("title") || attr(tree, id, "id").contains("title"))
    })
}

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
