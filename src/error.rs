//! Error types of the extraction pipeline.

use std::fmt;

/// Why an extraction call returned no result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtractError {
    /// The page was rejected by one of upstream's checks.
    Rejected(Rejection),
    /// The caller's `CancelToken` was cancelled.
    Cancelled,
    /// The caller's deadline passed.
    DeadlineExceeded,
}

/// The upstream check that rejected a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rejection {
    /// The document has no usable tree.
    EmptyTree,
    /// The `html` language attribute does not match the target language.
    WrongHtmlLang,
    /// The URL is blacklisted.
    BlacklistedUrl,
    /// Required metadata is missing.
    MissingMetadata,
    /// The tree exceeds a limit; the payload names which one.
    TreeTooLarge(TreeLimit),
    /// The extracted text is shorter than the minimum.
    TooShort,
    /// The text is a duplicate of one seen before.
    Duplicate,
    /// The detected language does not match the target language.
    WrongLanguage,
    /// The input exceeds a size limit; the payload names which one.
    InputTooLarge(InputLimit),
    /// A value upstream raises on: either a `ValueError` or `TypeError` upstream catches (a tag name
    /// lxml refuses to create; it returns no result) or a raise that escapes `extract` (a span or
    /// character reference of more than 4300 digits, a URL `urljoin` rejects, a `hi` rend with no
    /// HTML tag, a heading level `int` rejects, a Discourse `posts` value that is not iterable).
    InvalidValue,
}

/// The limit a [`Rejection::TreeTooLarge`] hit, so the caller knows which setting to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeLimit {
    /// The node budget: `ParseLimits::max_nodes`, or the budget of html5ever's clones and work on
    /// formatting elements derived from it (D-5).
    MaxNodes,
    /// More than `MAX_TAG_ATTRIBUTES` attributes on one tag, or on an `html` or `body` element
    /// merged from repeated tags (D-5). Always on.
    TagAttributes,
    /// More children of the extracted body than `Config::max_tree_size`, counted after the `hi`
    /// elements are dissolved. `children` is that count.
    MaxTreeSize {
        /// The children of the body after the `hi` elements are dissolved.
        children: usize,
    },
    /// Table construction minted more cells than its budget in one tree (D-9).
    TableCells,
}

impl fmt::Display for TreeLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxNodes => f.write_str("max_nodes"),
            Self::TagAttributes => f.write_str("attributes per tag"),
            Self::MaxTreeSize { children } => write!(f, "max_tree_size ({children} children)"),
            Self::TableCells => f.write_str("table cells"),
        }
    }
}

/// The limit an [`Rejection::InputTooLarge`] hit, so the caller knows which setting to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputLimit {
    /// The raw input is longer than `max_input_bytes` (D-11). Checked before decompression.
    MaxInputBytes,
    /// The decompressed input is longer than `Config::max_file_size` (D-29).
    MaxFileSize,
    /// A JSON node count or offset does not fit 32 bits, which is text over 4 GiB.
    JsonSize,
}

impl fmt::Display for InputLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxInputBytes => f.write_str("max_input_bytes"),
            Self::MaxFileSize => f.write_str("max_file_size"),
            Self::JsonSize => f.write_str("JSON node count or offset past 32 bits"),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TreeTooLarge(limit) => write!(f, "tree is too large: {limit}"),
            Self::InputTooLarge(limit) => write!(f, "input is too large: {limit}"),
            Self::EmptyTree => f.write_str("empty tree"),
            Self::WrongHtmlLang => {
                f.write_str("html lang attribute does not match the target language")
            }
            Self::BlacklistedUrl => f.write_str("URL is blacklisted"),
            Self::MissingMetadata => f.write_str("required metadata is missing"),
            Self::TooShort => f.write_str("extracted text is too short"),
            Self::Duplicate => f.write_str("duplicate text"),
            Self::WrongLanguage => {
                f.write_str("detected language does not match the target language")
            }
            Self::InvalidValue => f.write_str("a value upstream cannot parse"),
        }
    }
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(reason) => write!(f, "page rejected: {reason}"),
            Self::Cancelled => f.write_str("extraction cancelled"),
            Self::DeadlineExceeded => f.write_str("extraction deadline exceeded"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// The result of a step upstream wraps in `except Exception`: `Rejected(_)` stands for that
/// exception and gives `Ok(None)`. `Cancelled` and `DeadlineExceeded` are the caller's, so they
/// stay errors.
pub(crate) fn caught<T>(result: Result<T, ExtractError>) -> Result<Option<T>, ExtractError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(ExtractError::Rejected(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
