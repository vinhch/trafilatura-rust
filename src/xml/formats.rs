//! The JSON and CSV outputs of upstream `xml.py`: `build_json_output` and `xmltocsv`.

use crate::cancel::Call;
use crate::error::ExtractError;
use crate::pyjson::dumps_str;
use crate::settings::Document;

use super::xml_to_text;

/// The text of an empty CSV field.
// upstream: xml.py::xmltocsv (`null="null"`)
const NULL: &str = "null";

/// The CSV field delimiter.
// upstream: xml.py::xmltocsv (`delim="\t"`)
const DELIM: &str = "\t";

/// The CSV line of `doc`: one row of eleven fields, each empty one written as `null`. The body and
/// the comments are converted with `include_formatting`.
// upstream: xml.py::xmltocsv
pub fn xml_to_csv(
    doc: &mut Document,
    include_formatting: bool,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let text = xml_to_text(&mut doc.tree, Some(doc.body), include_formatting, call)?;
    let comments = xml_to_text(
        &mut doc.tree,
        Some(doc.comments_body),
        include_formatting,
        call,
    )?;
    let fields = [
        doc.url.as_deref(),
        doc.id.as_deref(),
        doc.fingerprint.as_deref(),
        doc.hostname.as_deref(),
        doc.title.as_deref(),
        doc.image.as_deref(),
        doc.date.as_deref(),
        Some(text.as_str()),
        Some(comments.as_str()),
        doc.license.as_deref(),
        doc.pagetype.as_deref(),
    ];
    let row: Vec<String> = fields
        .into_iter()
        .map(|field| csv_field(field.filter(|value| !value.is_empty()).unwrap_or(NULL)))
        .collect();
    Ok(format!("{}\r\n", row.join(DELIM)))
}

/// `value` as one field of a `csv.writer` row with `QUOTE_MINIMAL`: quoted when it holds the
/// delimiter, a quote or a line break, with each quote doubled.
fn csv_field(value: &str) -> String {
    if value.contains(DELIM) || value.contains(['"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// The JSON object of `doc`, as `json.dumps(..., ensure_ascii=False)` writes it. With metadata the
/// keys follow upstream's `Document` slot order and `categories` and `tags` are joined with `;`.
/// `text` and `comments` are the plain text of the body and of the comments, whatever the
/// `text` field holds.
// upstream: xml.py::build_json_output
pub fn build_json_output(
    doc: &mut Document,
    with_metadata: bool,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let text = xml_to_text(&mut doc.tree, Some(doc.body), false, call)?;
    let comments = xml_to_text(&mut doc.tree, Some(doc.comments_body), false, call)?;
    let categories = doc.categories.as_deref().unwrap_or_default().join(";");
    let tags = doc.tags.as_deref().unwrap_or_default().join(";");
    let members: Vec<(&str, Option<&str>)> = if with_metadata {
        vec![
            ("title", doc.title.as_deref()),
            ("author", doc.author.as_deref()),
            ("hostname", doc.hostname.as_deref()),
            ("date", doc.date.as_deref()),
            ("fingerprint", doc.fingerprint.as_deref()),
            ("id", doc.id.as_deref()),
            ("license", doc.license.as_deref()),
            ("comments", Some(comments.as_str())),
            ("raw_text", doc.raw_text.as_deref()),
            ("text", Some(text.as_str())),
            ("language", doc.language.as_deref()),
            ("image", doc.image.as_deref()),
            ("pagetype", doc.pagetype.as_deref()),
            ("filedate", doc.filedate.as_deref()),
            ("source", doc.url.as_deref()),
            ("source-hostname", doc.sitename.as_deref()),
            ("excerpt", doc.description.as_deref()),
            ("categories", Some(categories.as_str())),
            ("tags", Some(tags.as_str())),
        ]
    } else {
        vec![
            ("text", Some(text.as_str())),
            ("comments", Some(comments.as_str())),
        ]
    };
    let members: Vec<String> = members
        .into_iter()
        .map(|(key, value)| {
            let value = value.map_or_else(|| "null".to_owned(), dumps_str);
            format!("{}: {value}", dumps_str(key))
        })
        .collect();
    Ok(format!("{{{}}}", members.join(", ")))
}
