//! Settings ported from upstream `settings.py` and `settings.cfg`: the extraction options, their
//! defaults and the element lists the pipeline shares.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::cancel::Call;
use crate::dom::{LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;
use crate::pystr::ignore_case_i;
use crate::xpaths::Selector;
use crate::xpaths::select::{attr, descendants, first_attr, is_html, re_test};

/// Largest input, compressed or not, that decoding accepts, in bytes.
// upstream: settings.cfg::MAX_FILE_SIZE
pub const MAX_FILE_SIZE: usize = 20_000_000;

/// Smallest input worth processing, in bytes.
// upstream: settings.cfg::MIN_FILE_SIZE
pub const MIN_FILE_SIZE: usize = 10;

/// Element count above which parsing fails with `Rejection::TreeTooLarge`, clones html5ever builds
/// included. Added by this crate, not an upstream constant (deviation D-5).
pub const DEFAULT_MAX_NODES: usize = 1_000_000;

/// The extraction keys of `settings.cfg`; its `Default` is the cfg file. Reading a cfg file is
/// upstream API (`use_config`) that this crate does not offer, so a caller overrides fields instead.
// upstream: settings.cfg
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Shortest extracted text, in characters, that skips the recall fallback.
    pub min_extracted_size: usize,
    /// Shortest main text, in characters, that is kept.
    pub min_output_size: usize,
    /// Shortest comments text, in characters, that is kept.
    pub min_output_comm_size: usize,
    /// Shortest text, in characters, that the duplicate test looks at.
    pub min_duplcheck_size: usize,
    /// Number of repetitions after which a text counts as a duplicate.
    pub max_repetitions: usize,
    /// Largest input in bytes.
    pub max_file_size: usize,
    /// Smallest input in bytes.
    pub min_file_size: usize,
    /// Element count above which a page is discarded; `None` = no limit.
    pub max_tree_size: Option<usize>,
    /// Whether the date search is extensive.
    pub extensive_date_search: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_extracted_size: 250,
            min_output_size: 1,
            min_output_comm_size: 1,
            min_duplcheck_size: 100,
            max_repetitions: 2,
            max_file_size: MAX_FILE_SIZE,
            min_file_size: MIN_FILE_SIZE,
            max_tree_size: None,
            extensive_date_search: true,
        }
    }
}

/// Output format; upstream's `output_format` strings, `Python` being the `bare_extraction` one.
// upstream: settings.py::SUPPORTED_FORMATS
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Plain text.
    Txt,
    /// Markdown.
    Markdown,
    /// HTML.
    Html,
    /// XML.
    Xml,
    /// XML-TEI.
    XmlTei,
    /// JSON.
    Json,
    /// CSV.
    Csv,
    /// The `Document` itself, for `bare_extraction`.
    Python,
}

/// Trade-off between the text kept and the noise let in.
// upstream: settings.py::Extractor.focus
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Neither favoured.
    Balanced,
    /// Less text, less noise.
    Precision,
    /// More text, more noise.
    Recall,
}

/// Parameters of the date extraction. `max_date` is a per-call value that the caller supplies, never
/// read from the clock when the options are built (`docs/architecture.md`, Runtime contract).
// upstream: settings.py::set_date_params
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateParams {
    /// Prefer the original publication date over the last modification.
    pub original_date: bool,
    /// Extensive date search.
    pub extensive_search: bool,
    /// Latest date accepted, `YYYY-MM-DD`; `None` until the call sets it.
    pub max_date: Option<String>,
}

/// All options of one extraction, as upstream's `Extractor` minus `fast` (D-1) and `config`, whose
/// values are copied into the size fields.
// upstream: settings.py::Extractor
#[derive(Clone, Debug)]
pub struct Extractor {
    /// Output format.
    pub format: Format,
    /// Precision or recall focus.
    pub focus: Focus,
    /// Whether comments are extracted.
    pub comments: bool,
    /// Whether text formatting is kept; resolved, Markdown implies it unless set.
    pub formatting: bool,
    /// Whether links are kept.
    pub links: bool,
    /// Whether images are kept.
    pub images: bool,
    /// Whether tables are kept.
    pub tables: bool,
    /// Whether duplicates are dropped (a caller-owned cache from Phase 5).
    pub dedup: bool,
    /// Target language, a two-letter code.
    pub lang: Option<String>,
    /// See `Config::min_extracted_size`.
    pub min_extracted_size: usize,
    /// See `Config::min_output_size`.
    pub min_output_size: usize,
    /// See `Config::min_output_comm_size`.
    pub min_output_comm_size: usize,
    /// See `Config::min_duplcheck_size`.
    pub min_duplcheck_size: usize,
    /// See `Config::max_repetitions`.
    pub max_repetitions: usize,
    /// See `Config::max_file_size`.
    pub max_file_size: usize,
    /// See `Config::min_file_size`.
    pub min_file_size: usize,
    /// See `Config::max_tree_size`.
    pub max_tree_size: Option<usize>,
    /// Where the page came from, when no URL is given.
    pub source: Option<String>,
    /// URL of the page.
    pub url: Option<String>,
    /// Whether metadata is extracted; resolved as upstream does.
    pub with_metadata: bool,
    /// Whether pages without metadata are rejected.
    pub only_with_metadata: bool,
    /// Whether XML-TEI output is validated.
    pub tei_validation: bool,
    /// Parameters of the date search.
    pub date_params: DateParams,
    /// Authors whose pages are dropped.
    pub author_blacklist: HashSet<String>,
    /// URLs that are dropped.
    pub url_blacklist: HashSet<String>,
}

impl Extractor {
    /// Builds the options, resolving what upstream resolves in `Extractor.__init__`: `recall` wins
    /// over `precision`; Markdown implies `formatting` unless it is given; a blacklist,
    /// `only_with_metadata` or the TEI format imply `with_metadata`. The remaining fields take
    /// upstream's defaults and are set directly.
    // upstream: settings.py::Extractor.__init__
    #[allow(clippy::too_many_arguments)] // mirrors upstream's keyword arguments
    pub fn new(
        config: Config,
        format: Format,
        precision: bool,
        recall: bool,
        formatting: Option<bool>,
        with_metadata: bool,
        only_with_metadata: bool,
        author_blacklist: HashSet<String>,
        url_blacklist: HashSet<String>,
    ) -> Self {
        let focus = if recall {
            Focus::Recall
        } else if precision {
            Focus::Precision
        } else {
            Focus::Balanced
        };
        Self {
            format,
            focus,
            comments: true,
            formatting: formatting.unwrap_or(format == Format::Markdown),
            links: false,
            images: false,
            tables: true,
            dedup: false,
            lang: None,
            min_extracted_size: config.min_extracted_size,
            min_output_size: config.min_output_size,
            min_output_comm_size: config.min_output_comm_size,
            min_duplcheck_size: config.min_duplcheck_size,
            max_repetitions: config.max_repetitions,
            max_file_size: config.max_file_size,
            min_file_size: config.min_file_size,
            max_tree_size: config.max_tree_size,
            source: None,
            url: None,
            with_metadata: with_metadata
                || only_with_metadata
                || !url_blacklist.is_empty()
                || !author_blacklist.is_empty()
                || format == Format::XmlTei,
            only_with_metadata,
            tei_validation: false,
            date_params: DateParams {
                original_date: true,
                extensive_search: config.extensive_date_search,
                max_date: None,
            },
            author_blacklist,
            url_blacklist,
        }
    }
}

/// What an extraction returns: the metadata fields, the body and comments body as trees, and the
/// texts. The body and the comments body are two `body` elements of one compact tree the document
/// owns, copied out of the call's arena. Upstream's `from_dict` and `as_dict` are not ported: the
/// fields are the dict.
// upstream: settings.py::Document
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Document {
    /// Page title.
    pub title: Option<String>,
    /// Author.
    pub author: Option<String>,
    /// URL of the page.
    pub url: Option<String>,
    /// Host name of the URL.
    pub hostname: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Name of the site.
    pub sitename: Option<String>,
    /// Publication date, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Categories.
    pub categories: Option<Vec<String>>,
    /// Tags.
    pub tags: Option<Vec<String>>,
    /// Content fingerprint.
    pub fingerprint: Option<String>,
    /// Record id given by the caller.
    pub id: Option<String>,
    /// License.
    pub license: Option<String>,
    /// Comments as text, `bare_extraction` format `Python` only.
    pub comments: Option<String>,
    /// The main text as the extraction left it, before the conversion to the output format.
    pub raw_text: Option<String>,
    /// The main text, `bare_extraction` format `Python` only.
    pub text: Option<String>,
    /// Language of the page.
    pub language: Option<String>,
    /// Main image.
    pub image: Option<String>,
    /// Page type.
    pub pagetype: Option<String>,
    /// Date of the file.
    pub filedate: Option<String>,
    pub(crate) tree: Tree,
    pub(crate) body: NodeId,
    pub(crate) comments_body: NodeId,
}

/// The value of a [`Document`] field read by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field<'a> {
    /// A string field.
    Text(&'a str),
    /// `categories` or `tags`.
    List(&'a [String]),
}

impl Document {
    /// An empty document: no metadata and an empty `body` for the text and for the comments
    /// (upstream's defaults, two separate elements).
    pub(crate) fn new() -> Self {
        let mut tree = Tree::new(LocalName::from("body"), Ns::Html, Vec::new());
        let body = tree.root();
        let comments_body = tree.new_element("body");
        Self {
            title: None,
            author: None,
            url: None,
            hostname: None,
            description: None,
            sitename: None,
            date: None,
            categories: None,
            tags: None,
            fingerprint: None,
            id: None,
            license: None,
            comments: None,
            raw_text: None,
            text: None,
            language: None,
            image: None,
            pagetype: None,
            filedate: None,
            tree,
            body,
            comments_body,
        }
    }

    /// The tree that holds the body and the comments body.
    #[cfg(feature = "__internals")]
    pub fn body_tree(&self) -> &Tree {
        &self.tree
    }

    /// The `body` element of the main text in [`Document::body_tree`].
    #[cfg(feature = "__internals")]
    pub fn body(&self) -> NodeId {
        self.body
    }

    /// The `body` element of the comments in [`Document::body_tree`].
    #[cfg(feature = "__internals")]
    pub fn comments_body(&self) -> NodeId {
        self.comments_body
    }

    /// The metadata field `name` as upstream's `getattr(document, name, None)` followed by a truth
    /// test reads it: `None` for an unset, empty or unknown field.
    pub(crate) fn field(&self, name: &str) -> Option<Field<'_>> {
        fn text(value: &Option<String>) -> Option<&str> {
            value.as_deref().filter(|text| !text.is_empty())
        }
        fn list(value: &Option<Vec<String>>) -> Option<&[String]> {
            value.as_deref().filter(|list| !list.is_empty())
        }
        match name {
            "title" => text(&self.title).map(Field::Text),
            "author" => text(&self.author).map(Field::Text),
            "url" => text(&self.url).map(Field::Text),
            "hostname" => text(&self.hostname).map(Field::Text),
            "description" => text(&self.description).map(Field::Text),
            "sitename" => text(&self.sitename).map(Field::Text),
            "date" => text(&self.date).map(Field::Text),
            "categories" => list(&self.categories).map(Field::List),
            "tags" => list(&self.tags).map(Field::List),
            "fingerprint" => text(&self.fingerprint).map(Field::Text),
            "id" => text(&self.id).map(Field::Text),
            "license" => text(&self.license).map(Field::Text),
            "language" => text(&self.language).map(Field::Text),
            _ => None,
        }
    }
}

/// Elements that are dropped when they are empty.
// upstream: settings.py::CUT_EMPTY_ELEMS
pub const CUT_EMPTY_ELEMS: &[&str] = &[
    "article",
    "b",
    "blockquote",
    "dd",
    "div",
    "dt",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "i",
    "li",
    "main",
    "p",
    "pre",
    "q",
    "section",
    "span",
    "strong",
];

/// Elements removed with their content; the order is kept as upstream keeps it, for determinism.
// upstream: settings.py::MANUALLY_CLEANED
pub const MANUALLY_CLEANED: &[&str] = &[
    // important
    "aside",
    "embed",
    "fencedframe",
    "footer",
    "form",
    "head",
    "iframe",
    "menu",
    "object",
    "script",
    // other content
    "applet",
    "audio",
    "canvas",
    "figure",
    "map",
    "picture",
    "svg",
    "video",
    // secondary
    "area",
    "blink",
    "button",
    "datalist",
    "dialog",
    "frame",
    "frameset",
    "fieldset",
    "link",
    "input",
    "label",
    "legend",
    "marquee",
    "math",
    "menuitem",
    "nav",
    "noindex",
    "noscript",
    "optgroup",
    "option",
    "output",
    "param",
    "progress",
    "rp",
    "rt",
    "rtc",
    "select",
    "source",
    "style",
    "track",
    "textarea",
    "time",
    "use",
];

/// Elements whose tags are removed and whose content stays.
// upstream: settings.py::MANUALLY_STRIPPED
pub const MANUALLY_STRIPPED: &[&str] = &[
    "abbr", "acronym", "address", "bdi", "bdo", "big", "cite", "data", "dfn", "font", "hgroup",
    "img", "ins", "mark", "meta", "nobr", "ruby", "small", "tbody", "template", "tfoot", "thead",
    // the HTML parser does not treat it as void: it would swallow the rest of the paragraph
    "wbr",
];

/// Tags of the output tree that carry text.
// upstream: settings.py::TAG_CATALOG
pub const TAG_CATALOG: &[&str] = &["code", "del", "head", "hi", "lb", "list", "p", "quote"];

/// Shortest repeated span, in characters, that counts as an extraction artifact.
// upstream: settings.py::MIN_DUPLICATE_LENGTH
pub const MIN_DUPLICATE_LENGTH: usize = 50;

/// Accumulated text size, in characters, beyond which the quadratic substring-dedup scans are skipped.
// upstream: settings.py::DEDUPE_SCAN_CAP
pub const DEDUPE_SCAN_CAP: usize = 200_000;

/// Inline tags that fold their children into their own text.
// upstream: settings.py::INLINE_CONSUMING
pub const INLINE_CONSUMING: &[&str] = &["hi", "ref", "del"];

/// `INLINE_CONSUMING` plus `code`: has text, rendered verbatim.
// upstream: settings.py::INLINE_FORMATTABLE
pub const INLINE_FORMATTABLE: &[&str] = &["hi", "ref", "del", "code"];

/// `INLINE_FORMATTABLE` plus `graphic`: no text, rendered as image markup.
// upstream: settings.py::INLINE_CARRIED
pub const INLINE_CARRIED: &[&str] = &["hi", "ref", "del", "code", "graphic"];

/// Class and id vocabulary of consent banners: high-signal compounds and consent-platform vendors,
/// not the bare word `cookie`, which also sits on content classes.
// upstream: settings.py::_COOKIE_CONSENT_RE
const COOKIE_CONSENT_RE: &str = concat!(
    "cookie[-_]?(?:banner|bar|consent|law|notice|policy|description)|notice[-_]{0,2}cookie",
    "|consent[-_]?(?:banner|manager|sdk)|borlabs|cookiebot|cmplz|onetrust|moove[-_]?gdpr"
);

// `re:test(..., 'i')`: Python folds U+0130 and U+0131 to `i`, so each `i` is widened (`ignore_case_i`).
// A pattern that did not compile is `None`, which `re_test` reads as false.
pub(crate) static COOKIE_CONSENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(&format!("(?i){}", ignore_case_i(COOKIE_CONSENT_RE))).ok());

/// The elements removed by the basic clean-up, whatever their content.
// upstream: settings.py::BASIC_CLEAN_XPATH
pub const BASIC_CLEAN_XPATH: Selector = basic_clean_xpath;

// upstream: settings.py::BASIC_CLEAN_XPATH
fn basic_clean_xpath(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    // `.//aside|.//div[...]|...|.//*[...]`: a union of walks over the same descendants is one walk
    // with an or-predicate, in document order.
    descendants(tree, ctx, call, |tree, id| {
        is_html(
            tree,
            id,
            &[
                "aside",
                "fencedframe",
                "footer",
                "script",
                "style",
                "svg",
                "template",
            ],
        ) || (is_html(tree, id, &["div"])
            && first_attr(tree, id, &["id", "class"]).contains("footer"))
            || re_test(attr(tree, id, "class"), &COOKIE_CONSENT)
            || re_test(attr(tree, id, "id"), &COOKIE_CONSENT)
    })
}
