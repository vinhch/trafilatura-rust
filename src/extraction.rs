//! The extraction entry points of upstream `core.py`: the output step now, the cascade and the
//! public functions in later tasks.

use crate::cancel::Call;
use crate::error::ExtractError;
use crate::html_processing::build_html_output;
use crate::pyjson::dumps_str;
use crate::pystr::{self, is_alpha};
use crate::settings::{Document, Extractor, Field, Format};
use crate::utils::normalize_unicode;
use crate::xml::{build_json_output, control_xml_output, meta_items, xml_to_csv, xml_to_text};

/// The output formats whose result is text.
// upstream: core.py::TXT_FORMATS
pub const TXT_FORMATS: &[Format] = &[Format::Markdown, Format::Txt];

/// The metadata fields of the text header, in output order.
// upstream: core.py::_YAML_FIELDS
pub const YAML_FIELDS: &[&str] = &[
    "title",
    "author",
    "url",
    "hostname",
    "description",
    "sitename",
    "date",
    "categories",
    "tags",
    "fingerprint",
    "id",
    "license",
];

/// The words a YAML reader takes for a non-string, compared in lower case.
// upstream: core.py::_YAML_RESERVED
pub const YAML_RESERVED: &[&str] = &[
    "true", "false", "yes", "no", "on", "off", "y", "n", "null", "none", "~",
];

/// `value` as a plain YAML scalar when that reads back as the same string, else as a double-quoted
/// one (a JSON string).
// upstream: core.py::_yaml_scalar
fn yaml_scalar(value: &str) -> String {
    let plain = value.chars().next().is_some_and(is_alpha)
        && value == pystr::strip(value)
        && !value.contains(": ")
        && !value.contains(" #")
        && !value.ends_with(':')
        // `lower()` of a non-ASCII character is never an ASCII letter a reserved word holds (U+212A
        // gives `k`, which none has), so an ASCII-insensitive comparison finds the same words
        && !YAML_RESERVED
            .iter()
            .any(|word| value.eq_ignore_ascii_case(word))
        && value.chars().all(|ch| ch >= ' ' && ch != '\x7f');
    if plain {
        value.to_owned()
    } else {
        dumps_str(value)
    }
}

/// The header of the text formats: the non-empty `YAML_FIELDS` of `doc` between two `---` lines,
/// a list as Python's `repr` writes it.
// upstream: core.py::determine_returnstring
fn metadata_header(doc: &Document) -> String {
    let mut header = String::from("---\n");
    for &name in YAML_FIELDS {
        match doc.field(name) {
            Some(Field::Text(text)) => header.push_str(&format!("{name}: {}\n", yaml_scalar(text))),
            Some(Field::List(items)) => {
                let items: Vec<String> = items.iter().map(|item| pystr::repr(item)).collect();
                header.push_str(&format!("{name}: [{}]\n", items.join(", ")));
            }
            None => {}
        }
    }
    header.push_str("---\n");
    header
}

/// Converts the body of `doc` to the output format of `options`, as a string in Unicode normal
/// form NFC. The XML, CSV and JSON formats are stubs until phase 6. Fails with
/// `Rejection::InvalidValue` where `xml_to_text` and `build_html_output` do.
// upstream: core.py::determine_returnstring
pub fn determine_returnstring(
    doc: &mut Document,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let returnstring = match options.format {
        Format::Xml | Format::XmlTei => {
            // todo(phase 6): xml (the last cleaning of empty elements, then the output tree)
            control_xml_output(doc, options)
        }
        Format::Csv => xml_to_csv(doc, options.formatting),
        Format::Json => build_json_output(doc, options.with_metadata),
        Format::Html => {
            let items = options.with_metadata.then(|| meta_items(doc));
            let pairs: Option<Vec<(&str, &str)>> = items.as_ref().map(|items| {
                items
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect()
            });
            build_html_output(&mut doc.tree, doc.body, pairs.as_deref(), call)?
        }
        // Markdown and TXT, which is also upstream's `else` for the Python format
        Format::Markdown | Format::Txt | Format::Python => {
            let header = if options.with_metadata {
                metadata_header(doc)
            } else {
                String::new()
            };
            let text = xml_to_text(&mut doc.tree, Some(doc.body), options.formatting, call)?;
            // the comments body is never missing (`Document` always has one), so the join and the
            // strip always run
            let comments = xml_to_text(
                &mut doc.tree,
                Some(doc.comments_body),
                options.formatting,
                call,
            )?;
            pystr::strip(&format!("{header}{text}\n{comments}")).to_owned()
        }
    };
    Ok(normalize_unicode(&returnstring).into_owned())
}

#[cfg(test)]
mod tests;
