//! The author readers of upstream `json_metadata.py`: author names, their joining and normalization.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{NoExpand, Regex};

use crate::cancel::Call;
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pyhtml::unescape;
use crate::pyjson::{Json, JsonRef, Kind};
use crate::pystr::{self, compile_py, ignore_case_i};
use crate::settings::Document;
use crate::utils::{HTML_STRIP_TAGS, as_list, trim};

use super::{JsonFailure, Raised, compile, sub_all};

/// The name parts of a `Person`, in the order upstream joins them.
// upstream: json_metadata.py::AUTHOR_ATTRS
const AUTHOR_ATTRS: [&str; 3] = ["givenName", "additionalName", "familyName"];

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

/// The name an author node gives, `None` where upstream finds none. The node is a string, an object or
/// a list; every other value raises on `in`.
pub(super) fn author_name(
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
pub(super) fn joined_names(
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
pub(super) fn process_authors(
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
    for name in names {
        call.check()?;
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
    let mut visited = 0;
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
