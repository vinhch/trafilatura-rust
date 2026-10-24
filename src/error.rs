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
    /// The tree exceeds the node limit.
    TreeTooLarge,
    /// The extracted text is shorter than the minimum.
    TooShort,
    /// The text is a duplicate of one seen before.
    Duplicate,
    /// The detected language does not match the target language.
    WrongLanguage,
    /// The input exceeds the size limit.
    InputTooLarge,
    /// A value upstream raises on: either a `ValueError` upstream catches (it returns no result)
    /// or a raise that escapes `extract` (a span or character reference of more than 4300 digits,
    /// a URL `urljoin` rejects, a `hi` rend with no HTML tag, a heading level `int` rejects).
    InvalidValue,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyTree => "empty tree",
            Self::WrongHtmlLang => "html lang attribute does not match the target language",
            Self::BlacklistedUrl => "URL is blacklisted",
            Self::MissingMetadata => "required metadata is missing",
            Self::TreeTooLarge => "tree is too large",
            Self::TooShort => "extracted text is too short",
            Self::Duplicate => "duplicate text",
            Self::WrongLanguage => "detected language does not match the target language",
            Self::InputTooLarge => "input is too large",
            Self::InvalidValue => "a value upstream cannot parse",
        })
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
