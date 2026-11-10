//! The public extraction API: [`Options`] builds an immutable [`Extractor`], which runs on an
//! [`Input`].
//!
//! The API is this crate's own; the pipeline under it follows upstream's `extract` and
//! `bare_extraction`, whose defaults [`Options`] keeps.

use std::borrow::Cow;
use std::collections::HashSet;

use crate::baseline;
use crate::cancel::Call;
use crate::context::Ctx;
use crate::dom::ParseLimits;
use crate::error::ExtractError;
use crate::extraction::{self, Source};
use crate::settings::{self, Config, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_NODES, Document, Focus};
use crate::utils::HtmlInput;

mod batch;

pub use batch::{BatchEnd, BatchError, BatchOptions, Skipped};

/// Output format of an extraction.
// upstream: settings.py::SUPPORTED_FORMATS
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// Plain text.
    Txt,
    /// Markdown; implies text formatting unless [`Options::formatting`] says otherwise.
    Markdown,
    /// HTML.
    Html,
    /// XML, as upstream's `control_xml_output` writes it.
    Xml,
    /// TEI XML with a `teiHeader`. Forces [`Options::with_metadata`] on, as upstream does.
    XmlTei,
    /// JSON, as upstream's `build_json_output` writes it.
    Json,
    /// CSV: one tab-separated row of the metadata fields, the text and the comments, as upstream's
    /// `xmltocsv` writes it.
    Csv,
    /// The [`Document`] with its `text` and `comments` as strings; only
    /// [`Extractor::bare_extraction`] serves it.
    Python,
}

/// The internal format of a public one. A function, not a `From` impl: the default build cannot
/// name `settings::Format`, so a public impl would show in the docs as `From<Format> for Format`.
pub fn settings_format(format: Format) -> settings::Format {
    match format {
        Format::Txt => settings::Format::Txt,
        Format::Markdown => settings::Format::Markdown,
        Format::Html => settings::Format::Html,
        Format::Xml => settings::Format::Xml,
        Format::XmlTei => settings::Format::XmlTei,
        Format::Json => settings::Format::Json,
        Format::Csv => settings::Format::Csv,
        Format::Python => settings::Format::Python,
    }
}

/// The settings of an extraction, with upstream's defaults. Each method takes and returns the
/// options, and [`Options::build`] makes the [`Extractor`].
// upstream: settings.py::Extractor
#[derive(Clone, Debug)]
pub struct Options {
    format: Option<Format>,
    focus: Focus,
    comments: bool,
    formatting: Option<bool>,
    links: bool,
    images: bool,
    tables: bool,
    target_language: Option<String>,
    config: Config,
    max_nodes: Option<usize>,
    max_input_bytes: Option<usize>,
    with_metadata: bool,
    only_with_metadata: bool,
    author_blacklist: HashSet<String>,
    url_blacklist: HashSet<String>,
    keep_script_text: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}

impl Options {
    /// Upstream's defaults: no format (unset means [`Format::Txt`] for [`Extractor::extract`] and
    /// [`Format::Python`] for [`Extractor::bare_extraction`]), [`Focus::Balanced`], comments and
    /// tables on, formatting unset (on for Markdown), links and images off, no target language,
    /// `Config::default()`, `max_nodes` at `DEFAULT_MAX_NODES` (500,000) and
    /// `max_input_bytes` at `DEFAULT_MAX_INPUT_BYTES` (64 MiB). Metadata is not extracted, both
    /// blacklists are empty, and the text of unread `style` and `script` elements is dropped at the
    /// parse.
    pub fn new() -> Self {
        Self {
            format: None,
            focus: Focus::Balanced,
            comments: true,
            formatting: None,
            links: false,
            images: false,
            tables: true,
            target_language: None,
            config: Config::default(),
            max_nodes: Some(DEFAULT_MAX_NODES),
            max_input_bytes: Some(DEFAULT_MAX_INPUT_BYTES),
            with_metadata: false,
            only_with_metadata: false,
            author_blacklist: HashSet::new(),
            url_blacklist: HashSet::new(),
            keep_script_text: false,
        }
    }

    /// Sets the output format for both entry points.
    pub fn format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }

    /// Sets the trade-off between the text kept and the noise let in.
    pub fn focus(mut self, focus: Focus) -> Self {
        self.focus = focus;
        self
    }

    /// Sets whether comments are extracted along with the main text.
    pub fn comments(mut self, comments: bool) -> Self {
        self.comments = comments;
        self
    }

    /// Sets whether text formatting (headings, emphasis, lists) is kept; unset, Markdown keeps it
    /// and the other formats do not.
    pub fn formatting(mut self, formatting: bool) -> Self {
        self.formatting = Some(formatting);
        self
    }

    /// Sets whether links are kept along with their targets.
    pub fn links(mut self, links: bool) -> Self {
        self.links = links;
        self
    }

    /// Sets whether images are kept.
    pub fn images(mut self, images: bool) -> Self {
        self.images = images;
        self
    }

    /// Sets whether tables are kept.
    pub fn tables(mut self, tables: bool) -> Self {
        self.tables = tables;
        self
    }

    /// Sets the target language, a two-letter code, to discard pages of another language.
    pub fn target_language(mut self, language: impl Into<String>) -> Self {
        self.target_language = Some(language.into());
        self
    }

    /// Sets the values of upstream's `settings.cfg`.
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// Sets the element count above which a page is rejected with `Rejection::TreeTooLarge`,
    /// counted while parsing; `None` lifts the limit.
    pub fn max_nodes(mut self, max_nodes: Option<usize>) -> Self {
        self.max_nodes = max_nodes;
        self
    }

    /// Sets the raw input length, in bytes, above which a page is rejected with
    /// `Rejection::InputTooLarge(InputLimit::MaxInputBytes)` before decompression, decoding or
    /// parsing; `None` lifts the limit.
    pub fn max_input_bytes(mut self, max_input_bytes: Option<usize>) -> Self {
        self.max_input_bytes = max_input_bytes;
        self
    }

    /// Sets whether metadata is extracted: title, author, URL, hostname, date, sitename and the other
    /// fields go into the [`Document`] of [`Extractor::bare_extraction`], and into the header of the
    /// text, Markdown and HTML outputs, and into the XML, XML-TEI, JSON and CSV outputs of
    /// [`Extractor::extract`]. A URL or author blacklist, or [`Options::only_with_metadata`],
    /// turns it on too.
    pub fn with_metadata(mut self, on: bool) -> Self {
        self.with_metadata = on;
        self
    }

    /// Sets whether a page without a date, a title or a URL is rejected with
    /// `Rejection::MissingMetadata`, which names the missing fields. Implies metadata extraction.
    pub fn only_with_metadata(mut self, on: bool) -> Self {
        self.only_with_metadata = on;
        self
    }

    /// Sets the authors to drop from the metadata, replacing the set given before. A name matches
    /// without regard to case, one `;`-separated name at a time. A non-empty set turns metadata
    /// extraction on.
    pub fn author_blacklist<I, S>(mut self, authors: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.author_blacklist = authors.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the URLs of pages to reject with `Rejection::BlacklistedUrl`, replacing the set given
    /// before. A non-empty set turns metadata extraction on, since the URL is a metadata field.
    pub fn url_blacklist<I, S>(mut self, urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.url_blacklist = urls.into_iter().map(Into::into).collect();
        self
    }

    /// Sets whether the text of `script` and `style` elements is kept in the parse. Off, the text
    /// of a `style` element and of a `script` whose `type` upstream does not read is not stored,
    /// unless metadata is extracted: a date can sit in such a script, so metadata keeps the text
    /// either way. A page whose text readers reach dropped text is parsed again with the text kept.
    pub fn keep_script_text(mut self, keep: bool) -> Self {
        self.keep_script_text = keep;
        self
    }

    /// Resolves the options once for each format the [`Extractor`] may run: with no format set,
    /// [`Format::Txt`] for `extract` and [`Format::Python`] for `bare_extraction`.
    pub fn build(&self) -> Extractor {
        let resolve = |format: Format| {
            let mut settings = settings::Extractor::new(
                self.config.clone(),
                settings_format(format),
                self.focus == Focus::Precision,
                self.focus == Focus::Recall,
                self.formatting,
                self.with_metadata,
                self.only_with_metadata,
                self.author_blacklist.clone(),
                self.url_blacklist.clone(),
            );
            settings.comments = self.comments;
            settings.links = self.links;
            settings.images = self.images;
            settings.tables = self.tables;
            settings.lang = self.target_language.clone();
            settings
        };
        let text = resolve(self.format.unwrap_or(Format::Txt));
        Extractor {
            document: resolve(self.format.unwrap_or(Format::Python)),
            limits: ParseLimits {
                max_nodes: self.max_nodes,
                max_input_bytes: self.max_input_bytes,
                // deviation: D-14
                // Metadata reads dates from script text, so it keeps the text.
                drop_script_text: !self.keep_script_text && !text.with_metadata,
                ..ParseLimits::default()
            },
            text,
        }
    }
}

/// One page to extract from, with the URL and record id of this call.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    html: HtmlInput<'a>,
    url: Option<&'a str>,
    record_id: Option<&'a str>,
}

impl<'a> Input<'a> {
    /// A page as raw bytes in an unknown encoding, possibly compressed (feature `decompress`).
    pub fn bytes(bytes: &'a [u8]) -> Self {
        Self::new(HtmlInput::Bytes(bytes))
    }

    /// A page as text.
    pub fn text(text: &'a str) -> Self {
        Self::new(HtmlInput::Str(text))
    }

    fn new(html: HtmlInput<'a>) -> Self {
        Self {
            html,
            url: None,
            record_id: None,
        }
    }

    /// Sets the URL of the page, which resolves relative links. It belongs to this call, so one
    /// [`Extractor`] serves pages of many URLs.
    pub fn url(mut self, url: &'a str) -> Self {
        self.url = Some(url);
        self
    }

    /// Sets the id of the record. It appears in the metadata header that the text, Markdown and
    /// HTML outputs get from [`Options::with_metadata`].
    pub fn record_id(mut self, record_id: &'a str) -> Self {
        self.record_id = Some(record_id);
        self
    }
}

/// An immutable set of resolved settings; it is `Send + Sync` and serves any number of calls on any
/// number of threads.
#[derive(Clone, Debug)]
pub struct Extractor {
    /// Settings of `extract`.
    text: settings::Extractor,
    /// Settings of `bare_extraction`.
    document: settings::Extractor,
    limits: ParseLimits,
}

impl Extractor {
    /// Extracts the main text of `input` as a string in the format of the options ([`Format::Txt`]
    /// when unset). `Err(Rejected(_))` stands for upstream's `None` and for an exception that
    /// escapes `extract`: [`Format::Python`] fails with `Rejection::InvalidValue` after a
    /// successful extraction.
    // upstream: core.py::extract
    pub fn extract(&self, input: &Input<'_>, call: &Call<'_>) -> Result<String, ExtractError> {
        let ctx = context(&self.text, input, call);
        let document = extraction::internal_extraction(
            Source::Input(input.html),
            input.record_id,
            &ctx,
            &self.limits,
        )?;
        Ok(document.text.unwrap_or_default())
    }

    /// Extracts `input` into a [`Document`]. The format of the options ([`Format::Python`] when
    /// unset) decides which fields are set: `Python` fills `text` and `comments`, any other format
    /// only `raw_text`. `Err(Rejected(_))` stands for upstream's `None` and for an exception that
    /// escapes `bare_extraction`.
    // upstream: core.py::bare_extraction
    pub fn bare_extraction(
        &self,
        input: &Input<'_>,
        call: &Call<'_>,
    ) -> Result<Document, ExtractError> {
        let ctx = context(&self.document, input, call);
        extraction::bare_extraction(Source::Input(input.html), &ctx, &self.limits)
    }
}

/// The context of one call: the stored settings, or a copy with the URL of the input.
fn context<'a>(
    settings: &'a settings::Extractor,
    input: &Input<'_>,
    call: &'a Call<'a>,
) -> Ctx<'a> {
    let options = match input.url {
        Some(url) => {
            let mut owned = settings.clone();
            owned.url = Some(url.to_owned());
            Cow::Owned(owned)
        }
        None => Cow::Borrowed(settings),
    };
    Ctx { options, call }
}

/// The text of `input`'s `body` with word boundaries kept at block tags, `clean` removing
/// potentially undesirable elements first. An empty document gives `Ok("")`. The URL and record id
/// of `input` are not used.
// upstream: baseline.py::html2txt
pub fn html2txt(input: &Input<'_>, clean: bool, call: &Call<'_>) -> Result<String, ExtractError> {
    baseline::html2txt(input.html, clean, &ParseLimits::default(), call)
}

#[cfg(test)]
mod tests;
