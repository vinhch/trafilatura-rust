//! Metadata from JSON-LD, ported from upstream `json_metadata.py`.
//!
//! An upstream exception that `extract_metadata`'s `except Exception` catches is [`JsonFailure::Raised`]:
//! `.get` on a value that is not an object, `.lower()` on a non-string, `" ".join` over a non-string,
//! `in` on a number, a nesting or integer limit of `json.loads`, and the `ValueError` of
//! `html.unescape`. The `Document` keeps what was set before it, as upstream's one object does.
//!
//! Deviation D-19: where upstream stores a JSON value that is not a string in a string field of the
//! `Document`, the field stays as it was, and a list item of `categories` that is not a string is
//! dropped.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{Captures, NoExpand, Regex};

use crate::cancel::Call;
use crate::error::{ExtractError, Rejection};
use crate::html_processing::tick;
use crate::pyhtml::unescape;
use crate::pyjson::{Invalid, Json, JsonRef, Kind, try_loads};
use crate::pystr::{self, PY_SPACE_CLASS, PY_WORD_CLASS, ignore_case_i};
use crate::settings::Document;
use crate::utils::{HTML_STRIP_TAGS, as_list, trim};

/// Schema types of an article, lowercase.
// upstream: json_metadata.py::JSON_ARTICLE_SCHEMA
pub const JSON_ARTICLE_SCHEMA: &[&str] = &[
    "article",
    "backgroundnewsarticle",
    "blogposting",
    "medicalscholarlyarticle",
    "newsarticle",
    "opinionnewsarticle",
    "reportagenewsarticle",
    "scholarlyarticle",
    "socialmediaposting",
    "liveblogposting",
];

/// Schema types that give the page type, lowercase.
// upstream: json_metadata.py::JSON_OGTYPE_SCHEMA
pub const JSON_OGTYPE_SCHEMA: &[&str] = &[
    "aboutpage",
    "checkoutpage",
    "collectionpage",
    "contactpage",
    "faqpage",
    "itempage",
    "medicalwebpage",
    "profilepage",
    "qapage",
    "realestatelisting",
    "searchresultspage",
    "webpage",
    "website",
    "article",
    "advertisercontentarticle",
    "newsarticle",
    "analysisnewsarticle",
    "askpublicnewsarticle",
    "backgroundnewsarticle",
    "opinionnewsarticle",
    "reportagenewsarticle",
    "reviewnewsarticle",
    "report",
    "satiricalarticle",
    "scholarlyarticle",
    "medicalscholarlyarticle",
    "socialmediaposting",
    "blogposting",
    "liveblogposting",
    "discussionforumposting",
    "techarticle",
    "blog",
    "jobposting",
];

/// Schema types that give the site name, lowercase.
// upstream: json_metadata.py::JSON_PUBLISHER_SCHEMA
pub const JSON_PUBLISHER_SCHEMA: &[&str] = &[
    "newsmediaorganization",
    "organization",
    "webpage",
    "website",
];

/// The union of the three sets above and `person`.
// upstream: json_metadata.py::JSON_SCHEMA_TYPES
pub const JSON_SCHEMA_TYPES: &[&[&str]] = &[
    JSON_ARTICLE_SCHEMA,
    JSON_OGTYPE_SCHEMA,
    JSON_PUBLISHER_SCHEMA,
    &["person"],
];

/// The name parts of a `Person`, in the order upstream joins them.
// upstream: json_metadata.py::AUTHOR_ATTRS
const AUTHOR_ATTRS: [&str; 3] = ["givenName", "additionalName", "familyName"];

// The patterns are literals that compile (`patterns_compile` forces each one); `None` would make the
// step that uses it a no-op instead of a panic.
fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// A pattern of upstream with `\w` and `\s` written as `@W@` and `@S@`, which stand for Python's classes.
fn compile_py(pattern: &str) -> Option<Regex> {
    compile(
        &pattern
            .replace("@W@", PY_WORD_CLASS)
            .replace("@S@", PY_SPACE_CLASS),
    )
}

/// Crude author search of a JSON-LD text that does not parse; `.` takes newlines as `re.DOTALL` does.
// upstream: json_metadata.py::JSON_AUTHOR_1
pub static JSON_AUTHOR_1: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(
        r#"(?s)"author":[^}\[]+?"name?\\?": ?\\?"([^"\\]+)|"author"[^}\[]+?"names?".+?"([^"]+)"#,
    )
});

/// Second author search, for a `Person` node.
// upstream: json_metadata.py::JSON_AUTHOR_2
pub static JSON_AUTHOR_2: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r#"(?s)"[Pp]erson"[^}]+?"names?".+?"([^"]+)"#));

/// Image, organization and web page nodes, which hold names that are no author.
// upstream: json_metadata.py::JSON_AUTHOR_REMOVE
pub static JSON_AUTHOR_REMOVE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(
        r#",?(?:"[@W@]+":?[:|,\[])?\{?"@type":"(?:[Ii]mageObject|[Oo]rganization|[Ww]eb[Pp]age)",[^}\[]+\}[\]|}]?"#,
    )
});

/// Publisher name of a JSON-LD text that does not parse.
// upstream: json_metadata.py::JSON_PUBLISHER
pub static JSON_PUBLISHER: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r#"(?s)"publisher":[^}]+?"name?\\?": ?\\?"([^"\\]+)"#));

/// The value of the first `@type` key.
// upstream: json_metadata.py::JSON_TYPE
pub static JSON_TYPE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(r#"(?s)"@type"[@S@]*:[@S@]*"([^"]*)""#));

/// The first `articleSection` value.
// upstream: json_metadata.py::JSON_CATEGORY
pub static JSON_CATEGORY: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r#"(?s)"articleSection": ?"([^"\\]+)"#));

/// A `@context` that is schema.org; applied with `re.match`, so anchored at the start.
// upstream: json_metadata.py::JSON_SCHEMA_ORG
pub static JSON_SCHEMA_ORG: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?i)\Ahttps?://schema\.org"));

/// A `\uXXXX` escape left in a string.
// upstream: json_metadata.py::JSON_UNICODE_REPLACE
pub static JSON_UNICODE_REPLACE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"\\u([0-9a-fA-F]{4})"));

/// Title of an `Article` node in a text that does not parse.
// upstream: json_metadata.py::JSON_NAME
pub static JSON_NAME: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r#"(?s)"@type":"[Aa]rticle", ?"name": ?"([^"\\]+)"#));

/// Headline in a text that does not parse.
// upstream: json_metadata.py::JSON_HEADLINE
pub static JSON_HEADLINE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r#"(?s)"headline": ?"([^"\\]+)"#));

/// A leading role word of an author: `written by`, `von`. `[a-z]` takes U+0130 and U+0131 under
/// `re.IGNORECASE`, which Rust's `(?i)` does not, and each `i` of the words does too.
// upstream: json_metadata.py::AUTHOR_PREFIX
pub static AUTHOR_PREFIX: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let words = ignore_case_i("written by|words by|words|by|von|from");
    compile(&format!(r"(?i)\A(?:[a-zäöüßİı]+(?:ed|t))? ?(?:{words}) "))
});

/// From the first digit that another character follows to the end. `$` is `\z`: the author holds no
/// newline here, `trim` having turned each into a space.
// upstream: json_metadata.py::AUTHOR_REMOVE_NUMBERS
pub static AUTHOR_REMOVE_NUMBERS: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(&format!(r"[{}].+?\z", pystr::digit_class())));

/// A twitter handle.
// upstream: json_metadata.py::AUTHOR_TWITTER
pub static AUTHOR_TWITTER: LazyLock<Option<Regex>> = LazyLock::new(|| compile_py(r"@[@W@]+"));

/// Characters that stand for a space.
// upstream: json_metadata.py::AUTHOR_REPLACE_JOIN
pub static AUTHOR_REPLACE_JOIN: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"[._+]"));

/// A nickname in quotes or brackets.
// upstream: json_metadata.py::AUTHOR_REMOVE_NICKNAME
pub static AUTHOR_REMOVE_NICKNAME: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r#"["‘({\[’'][^"]+?[‘’"')\]}]"#));

/// Trailing non-word characters, and special characters anywhere.
// upstream: json_metadata.py::AUTHOR_REMOVE_SPECIAL
pub static AUTHOR_REMOVE_SPECIAL: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(r"[^@W@]+\z|[:()?*$#!%/<>{}~¿]"));

/// A preposition and what follows it. Upstream starts with `\b\s+`, a word boundary before a space, that
/// is a word character before the space; the pattern takes that character as group 1 and the
/// replacement puts it back.
// upstream: json_metadata.py::AUTHOR_REMOVE_PREPOSITION
pub static AUTHOR_REMOVE_PREPOSITION: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let prepositions = ignore_case_i("am|on|for|at|in|to|from|of|via|with|—|-|–");
    compile_py(&format!(
        r"(?i)(?-i:([@W@]))[@S@]+(?:{prepositions})[@S@]+.*"
    ))
});

/// An email address at the start; applied with `re.match`. Upstream's `\b` at the start needs a word
/// character there, and at the end a non-word character or the end, which is consumed here as only
/// the match matters.
// upstream: json_metadata.py::AUTHOR_EMAIL
pub static AUTHOR_EMAIL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(r"\A[A-Za-z0-9_][A-Za-z0-9._%+-]*@[A-Za-z0-9.-]+\.[A-Za-z]{2,}(?:\z|[^@W@])")
});

/// Separators between authors. `(?:^|\W)` and `(?:$|\W)` take `\A` and `\z`; a final newline that
/// upstream's `$` leaves out of the match is a piece that `trim` empties.
// upstream: json_metadata.py::AUTHOR_SPLIT
pub static AUTHOR_SPLIT: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(r"(?i)/|;|,|\||&|(?:\A|(?-i:[^@W@]))[ua]nd(?:\z|(?-i:[^@W@]))"));

/// Emoji ranges: dingbats, emoticons, miscellaneous symbols, transport and the supplemental sets.
// upstream: json_metadata.py::AUTHOR_EMOJI_REMOVE
pub static AUTHOR_EMOJI_REMOVE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(
        r"[\x{2700}-\x{27BE}\x{1F600}-\x{1F64F}\x{2600}-\x{26FF}\x{1F300}-\x{1F5FF}\x{1F900}-\x{1F9FF}\x{1FA70}-\x{1FAFF}\x{1F680}-\x{1F6FF}]+",
    )
});

/// Why a JSON-LD step stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raised;

/// How a step over JSON-LD ended without finishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonFailure {
    /// Upstream raised an exception that `extract_metadata` catches.
    Raised,
    /// The call was cancelled or hit a limit; the caller's error, never an upstream exception.
    Stop(ExtractError),
}

impl From<Raised> for JsonFailure {
    fn from(_: Raised) -> Self {
        Self::Raised
    }
}

impl From<ExtractError> for JsonFailure {
    /// `InvalidValue` is the `ValueError` of `html.unescape`.
    fn from(error: ExtractError) -> Self {
        match error {
            ExtractError::Rejected(Rejection::InvalidValue) => Self::Raised,
            other => Self::Stop(other),
        }
    }
}

/// `text` with every match of `regex` replaced by `replacement` as it is, `text` itself when the pattern
/// did not compile.
fn sub_all<'a>(regex: &Option<Regex>, text: &'a str, replacement: &str) -> Cow<'a, str> {
    match regex {
        Some(regex) => regex.replace_all(text, NoExpand(replacement)),
        None => Cow::Borrowed(text),
    }
}

/// The first group of the first match of `regex` in `text`.
fn first_group<'a>(regex: &Option<Regex>, text: &'a str) -> Option<&'a str> {
    regex
        .as_ref()?
        .captures(text)?
        .get(1)
        .map(|group| group.as_str())
}

fn is_schema_type(lowered: &str) -> bool {
    JSON_SCHEMA_TYPES.iter().any(|set| set.contains(&lowered))
}

/// Whether `candidate` is a better site name than the one `metadata` holds.
// upstream: json_metadata.py::is_plausible_sitename
pub fn is_plausible_sitename(
    metadata: &Document,
    candidate: Option<&str>,
    content_type: Option<&str>,
) -> bool {
    let Some(candidate) = candidate.filter(|candidate| !candidate.is_empty()) else {
        return false;
    };
    let sitename = metadata.sitename.as_deref().unwrap_or_default();
    if sitename.is_empty()
        || (sitename.chars().count() < candidate.chars().count() && content_type != Some("webpage"))
    {
        return true;
    }
    sitename.starts_with("http") && !candidate.starts_with("http")
}

/// The name an author node gives, `None` where upstream finds none. The node is a string, an object or
/// a list; every other value raises on `in`.
fn author_name(
    json: &Json,
    author: JsonRef,
    call: &Call<'_>,
) -> Result<Option<String>, JsonFailure> {
    match json.kind(author) {
        // `{"name": author}`
        Kind::Str => Ok(json.as_str(author).map(str::to_owned)),
        Kind::Object => {
            let is_person = json.get(author, "@type").is_none_or(|type_| {
                as_list(json, Some(type_))
                    .into_iter()
                    .any(|item| json.as_str(item) == Some("Person"))
            });
            if !is_person {
                return Ok(None);
            }
            if let Some(name) = json.get(author, "name") {
                return match json.kind(name) {
                    Kind::List => joined_names(json, name, call),
                    Kind::Object => Ok(json
                        .get(name, "name")
                        .and_then(|inner| json.as_str(inner))
                        .map(str::to_owned)),
                    Kind::Str => Ok(json.as_str(name).map(str::to_owned)),
                    _ => Ok(None),
                };
            }
            if json.get(author, "givenName").is_some() && json.get(author, "familyName").is_some() {
                let parts = AUTHOR_ATTRS
                    .iter()
                    .filter_map(|key| json.get(author, key))
                    .map(|part| json.as_str(part).ok_or(Raised))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Some(parts.join(" ")));
            }
            Ok(None)
        }
        // `"@type" not in author` is a list membership test; each branch that follows reads the list as
        // an object and raises.
        Kind::List => {
            let mut found = [false; 4];
            let mut visited = 0;
            for item in json.items(author) {
                tick(&mut visited, call)?;
                for (flag, key) in
                    found
                        .iter_mut()
                        .zip(["@type", "name", "givenName", "familyName"])
                {
                    *flag |= json.as_str(item) == Some(key);
                }
            }
            let [has_type, has_name, has_given, has_family] = found;
            if has_type || has_name || (has_given && has_family) {
                Err(Raised.into())
            } else {
                Ok(None)
            }
        }
        _ => Err(Raised.into()),
    }
}

/// `"; ".join(names).strip("; ")` over a list of strings; a list item that is no string raises.
fn joined_names(
    json: &Json,
    names: JsonRef,
    call: &Call<'_>,
) -> Result<Option<String>, JsonFailure> {
    let mut visited = 0;
    let mut texts = Vec::new();
    for item in json.items(names) {
        tick(&mut visited, call)?;
        texts.push(json.as_str(item).ok_or(Raised)?);
    }
    let names = texts;
    Ok(Some(names.join("; ").trim_matches([';', ' ']).to_owned()))
}

/// Adds the names of the authors in `list_authors`, a node of `json`: one author or a list of them.
fn process_authors(
    json: &Json,
    list_authors: JsonRef,
    metadata: &mut Document,
    call: &Call<'_>,
) -> Result<(), JsonFailure> {
    let mut visited = 0;
    for author in as_list(json, Some(list_authors)) {
        tick(&mut visited, call)?;
        if let Some(name) = author_name(json, author, call)? {
            metadata.author = normalize_authors(metadata.author.as_deref(), &name, call)?;
        }
    }
    Ok(())
}

/// The `articleSection` of an article as a category list: a string alone, a list's truthy strings, an
/// object's non-empty keys. A list item that is no string is dropped (D-19); any other value raises.
fn categories_of(
    json: &Json,
    section: JsonRef,
    call: &Call<'_>,
) -> Result<Vec<String>, JsonFailure> {
    let mut visited = 0;
    match json.kind(section) {
        Kind::Str => Ok(json
            .as_str(section)
            .map(|text| vec![text.to_owned()])
            .unwrap_or_default()),
        Kind::List => {
            let mut categories = Vec::new();
            for item in json.items(section) {
                tick(&mut visited, call)?;
                // deviation: D-19 (a list item that is no string is dropped)
                if let Some(text) = json.as_str(item).filter(|_| json.is_truthy(item)) {
                    categories.push(text.to_owned());
                }
            }
            Ok(categories)
        }
        Kind::Object => {
            let mut categories = Vec::new();
            for (key, _) in json.entries(section) {
                tick(&mut visited, call)?;
                if !key.is_empty() {
                    categories.push(key.to_owned());
                }
            }
            Ok(categories)
        }
        _ => Err(Raised.into()),
    }
}

/// Finds and extracts the selected metadata from the JSON parts in `parent`.
// upstream: json_metadata.py::process_parent
pub fn process_parent(
    json: &Json,
    parent: &[JsonRef],
    metadata: &mut Document,
    call: &Call<'_>,
) -> Result<(), JsonFailure> {
    let mut visited = 0;
    for &content in parent {
        tick(&mut visited, call)?;
        if !json.is_truthy(content) {
            continue;
        }
        // `content.get` raises on a string, number, `true` or list
        if json.kind(content) != Kind::Object {
            return Err(Raised.into());
        }
        // publisher may be a bare string, not a dict
        if let Some(publisher) = json
            .get(content, "publisher")
            .filter(|&publisher| json.kind(publisher) == Kind::Object)
        {
            let name = json
                .get(publisher, "name")
                .and_then(|name| json.as_str(name));
            if is_plausible_sitename(metadata, name, None) {
                metadata.sitename = name.map(str::to_owned);
            }
        }

        let Some(type_value) = json
            .get(content, "@type")
            .filter(|&type_value| json.is_truthy(type_value))
        else {
            continue;
        };

        // Prefer the first recognized type, preserving order among recognized types.
        let content_types = as_list(json, Some(type_value));
        let recognized = content_types.iter().copied().find(|&schema_type| {
            json.as_str(schema_type)
                .is_some_and(|text| is_schema_type(&pystr::lower(text)))
        });
        let Some(chosen) = recognized.or_else(|| content_types.first().copied()) else {
            continue;
        };
        // `.lower()` on a type that is no string raises
        let content_type = pystr::lower(json.as_str(chosen).ok_or(Raised)?);

        // The "pagetype" should only be returned if the page is some kind of an article, category, website...
        if JSON_OGTYPE_SCHEMA.contains(&content_type.as_str())
            && metadata.pagetype.as_deref().is_none_or(str::is_empty)
        {
            metadata.pagetype = Some(normalize_json(&content_type)?);
        }

        if JSON_PUBLISHER_SCHEMA.contains(&content_type.as_str()) {
            // `a or b or c`: the first truthy value, else the last
            let [name, legal_name, alternate_name] =
                ["name", "legalName", "alternateName"].map(|key| json.get(content, key));
            let candidate = [name, legal_name]
                .into_iter()
                .flatten()
                .find(|&value| json.is_truthy(value))
                .or(alternate_name);
            let candidate = candidate.and_then(|value| json.as_str(value));
            if is_plausible_sitename(metadata, candidate, Some(&content_type)) {
                metadata.sitename = candidate.map(str::to_owned);
            }
        } else if content_type == "person" {
            let name = json
                .get(content, "name")
                .and_then(|name| json.as_str(name))
                .filter(|name| !name.starts_with("http"));
            if let Some(name) = name {
                metadata.author = normalize_authors(metadata.author.as_deref(), name, call)?;
            }
        } else if JSON_ARTICLE_SCHEMA.contains(&content_type.as_str()) {
            if let Some(list_authors) = json.get(content, "author") {
                match json.as_str(list_authors) {
                    // try to convert to json object
                    Some(text) => match try_loads(text, true, call)? {
                        Ok(parsed) => process_authors(&parsed, parsed.root(), metadata, call)?,
                        // it is a normal string
                        Err(Invalid::Decode) => {
                            metadata.author =
                                normalize_authors(metadata.author.as_deref(), text, call)?;
                            process_authors(json, list_authors, metadata, call)?;
                        }
                        Err(Invalid::Raised) => return Err(Raised.into()),
                    },
                    None => process_authors(json, list_authors, metadata, call)?,
                }
            }

            // category
            if metadata.categories.as_ref().is_none_or(Vec::is_empty)
                && let Some(section) = json.get(content, "articleSection")
            {
                metadata.categories = Some(categories_of(json, section, call)?);
            }

            // try to extract title
            if metadata.title.as_deref().is_none_or(str::is_empty) {
                let name = json.get(content, "name");
                let source = if name.is_some() && content_type == "article" {
                    name
                } else {
                    json.get(content, "headline")
                };
                match source {
                    Some(value) if json.kind(value) == Kind::Str => {
                        metadata.title = json.as_str(value).map(str::to_owned);
                    }
                    Some(value) if json.kind(value) == Kind::Null => metadata.title = None,
                    // deviation: D-19 (a value that is no string leaves the title as it was)
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// Parses and extracts the metadata of JSON-LD data: `schema` is one node or a list of them. Each
/// node with a schema.org `@context` gives its own parts or the nodes of its `@graph` or
/// `liveBlogUpdate`; the parts are processed once.
// upstream: json_metadata.py::extract_json
pub fn extract_json(
    json: &Json,
    schema: JsonRef,
    metadata: &mut Document,
    call: &Call<'_>,
) -> Result<(), JsonFailure> {
    // collect content from every valid block, then process once (no short-circuit on a flat object)
    let mut parents: Vec<JsonRef> = Vec::new();
    let mut visited = 0;
    for parent in as_list(json, Some(schema)) {
        tick(&mut visited, call)?;
        // `parent.get` raises on a value that is no object
        if json.kind(parent) != Kind::Object {
            return Err(Raised.into());
        }
        let is_schema_org = json
            .get(parent, "@context")
            .and_then(|context| json.as_str(context))
            .is_some_and(|context| {
                !context.is_empty()
                    && JSON_SCHEMA_ORG
                        .as_ref()
                        .is_some_and(|regex| regex.is_match(context))
            });
        if !is_schema_org {
            continue;
        }
        if let Some(graph) = json.get(parent, "@graph") {
            parents.extend(as_list(json, Some(graph)));
        } else if json
            .get(parent, "@type")
            .and_then(|type_| json.as_str(type_))
            .is_some_and(|type_| pystr::lower(type_).contains("liveblogposting"))
            && let Some(update) = json.get(parent, "liveBlogUpdate")
        {
            parents.extend(as_list(json, Some(update)));
        } else {
            parents.push(parent);
        }
    }
    process_parent(json, &parents, metadata, call)
}

/// Crudely extracts the author names that `regular_expression` finds in a JSON-LD text. A match whose
/// name has no space ends the search. Checks `call` once per match.
// upstream: json_metadata.py::extract_json_author
pub fn extract_json_author(
    elemtext: &str,
    regular_expression: &Regex,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut authors: Option<String> = None;
    let mut text = elemtext.to_owned();
    let mut visited = 0;
    while let Some(found) = regular_expression.captures(&text) {
        tick(&mut visited, call)?;
        // first matching group (JSON_AUTHOR_1 has two)
        let name = found
            .iter()
            .skip(1)
            .flatten()
            .map(|group| group.as_str())
            .find(|name| !name.is_empty())
            .filter(|name| name.contains(' '))
            .map(str::to_owned);
        let Some(name) = name else { break };
        let Some(whole) = found.get(0).map(|whole| whole.range()) else {
            break;
        };
        authors = normalize_authors(authors.as_deref(), &name, call)?;
        text.replace_range(whole, "");
    }
    Ok(authors.filter(|authors| !authors.is_empty()))
}

/// Crudely extracts the metadata of a JSON-LD text that does not parse.
// upstream: json_metadata.py::extract_json_parse_error
pub fn extract_json_parse_error(
    elem: &str,
    metadata: &mut Document,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    // author info
    let element_text_author = sub_all(&JSON_AUTHOR_REMOVE, elem, "");
    let mut author = None;
    for pattern in [&JSON_AUTHOR_1, &JSON_AUTHOR_2] {
        if let Some(regex) = pattern.as_ref() {
            author = extract_json_author(&element_text_author, regex, call)?;
        }
        if author.is_some() {
            break;
        }
    }
    if author.is_some() {
        metadata.author = author;
    }

    // try to extract page type as an alternative to og:type
    if elem.contains("@type")
        && let Some(found) = first_group(&JSON_TYPE, elem)
    {
        let candidate = normalize_json(&pystr::lower(found))?;
        if JSON_OGTYPE_SCHEMA.contains(&candidate.as_str()) {
            metadata.pagetype = Some(candidate);
        }
    }

    // try to extract publisher
    if elem.contains("\"publisher\"")
        && let Some(found) = first_group(&JSON_PUBLISHER, elem).filter(|found| !found.contains(','))
    {
        let candidate = normalize_json(found)?;
        if is_plausible_sitename(metadata, Some(&candidate), None) {
            metadata.sitename = Some(candidate);
        }
    }

    // category
    if elem.contains("\"articleSection\"")
        && let Some(found) = first_group(&JSON_CATEGORY, elem)
    {
        metadata.categories = Some(vec![normalize_json(found)?]);
    }

    // try to extract title
    for (key, regex) in [("\"name\"", &JSON_NAME), ("\"headline\"", &JSON_HEADLINE)] {
        if elem.contains(key)
            && metadata.title.as_deref().is_none_or(str::is_empty)
            && let Some(found) = first_group(regex, elem)
        {
            metadata.title = Some(normalize_json(found)?);
            break;
        }
    }
    Ok(())
}

/// Normalizes the escapes of a JSON string that was cut out of a text, and trims it. Fails with
/// `InvalidValue`, the `ValueError` of `html.unescape` on a character reference of more than 4300
/// digits.
// upstream: json_metadata.py::normalize_json
pub fn normalize_json(string: &str) -> Result<String, ExtractError> {
    let mut text = Cow::Borrowed(string);
    if string.contains('\\') {
        let stripped = string
            .replace("\\n", "")
            .replace("\\r", "")
            .replace("\\t", "");
        // a surrogate escape gives no character, which upstream's filter of U+D800 to U+DFFF drops too
        let replaced = match JSON_UNICODE_REPLACE.as_ref() {
            Some(regex) => regex
                .replace_all(&stripped, |found: &Captures<'_>| {
                    found
                        .get(1)
                        .and_then(|digits| u32::from_str_radix(digits.as_str(), 16).ok())
                        .and_then(char::from_u32)
                        .map(String::from)
                        .unwrap_or_default()
                })
                .into_owned(),
            None => stripped,
        };
        text = Cow::Owned(unescape(&replaced)?.into_owned());
    }
    Ok(trim(&sub_all(&HTML_STRIP_TAGS, &text, "")))
}

/// Normalizes author info to the author names: appends the names of `author_string` to those of
/// `current_authors` (separated by `; `), keeping only the fullest form of each. Checks `call` once per
/// author. Fails with `InvalidValue` on a character reference of more than 4300 digits.
// upstream: json_metadata.py::normalize_authors
pub fn normalize_authors(
    current_authors: Option<&str>,
    author_string: &str,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let unchanged = || current_authors.map(str::to_owned);
    let is_email = AUTHOR_EMAIL
        .as_ref()
        .is_some_and(|regex| regex.is_match(author_string));
    if pystr::lower(author_string).starts_with("http") || is_email {
        return Ok(unchanged());
    }
    let mut new_authors: Vec<String> = current_authors
        .map(|current| current.split("; ").map(str::to_owned).collect())
        .unwrap_or_default();
    let mut author_string = Cow::Borrowed(author_string);
    // fix to code with unicode
    if author_string.contains("\\u")
        && let Some(decoded) = pystr::unicode_escape(&author_string)
    {
        author_string = Cow::Owned(decoded);
    }
    // fix html entities
    if author_string.contains("&#") || author_string.contains("&amp;") {
        author_string = Cow::Owned(unescape(&author_string)?.into_owned());
    }
    // remove html tags
    let author_string = sub_all(&HTML_STRIP_TAGS, &author_string, "");
    // examine names
    let names: Vec<&str> = match AUTHOR_SPLIT.as_ref() {
        Some(regex) => regex.split(&author_string).collect(),
        None => vec![&*author_string],
    };
    let mut visited = 0;
    for name in names {
        tick(&mut visited, call)?;
        let author = trim(name);
        // remove emoji
        let author = sub_all(&AUTHOR_EMOJI_REMOVE, &author, "");
        // remove @username
        let author = sub_all(&AUTHOR_TWITTER, &author, "");
        // replace special characters with space
        let author = trim(&sub_all(&AUTHOR_REPLACE_JOIN, &author, " "));
        let author = sub_all(&AUTHOR_REMOVE_NICKNAME, &author, "");
        // remove special characters
        let author = sub_all(&AUTHOR_REMOVE_SPECIAL, &author, "");
        let author = match AUTHOR_PREFIX.as_ref() {
            Some(regex) => regex.replace(&author, NoExpand("")),
            None => author,
        };
        let author = sub_all(&AUTHOR_REMOVE_NUMBERS, &author, "");
        let author = match AUTHOR_REMOVE_PREPOSITION.as_ref() {
            Some(regex) => regex.replace_all(&author, "${1}"),
            None => author,
        };
        // skip empty or improbably long strings
        // simple heuristics, regex or vowel tests also possible
        if author.is_empty()
            || (author.chars().count() >= 50 && !author.contains(' ') && !author.contains('-'))
        {
            continue;
        }
        // title case
        let author = if author.chars().next().is_some_and(pystr::is_upper) {
            author.into_owned()
        } else {
            pystr::title(&author)
        };
        if !new_authors.contains(&author) {
            new_authors.push(author);
        }
    }
    // keep only the fullest form of each name (drop names contained in another)
    let mut fullest = Vec::with_capacity(new_authors.len());
    for name in &new_authors {
        tick(&mut visited, call)?;
        if !new_authors
            .iter()
            .any(|other| name != other && other.contains(name.as_str()))
        {
            fullest.push(name.as_str());
        }
    }
    if fullest.is_empty() {
        return Ok(unchanged());
    }
    Ok(Some(fullest.join("; ").trim_matches([';', ' ']).to_owned()))
}

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
