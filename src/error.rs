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
    /// The tree exceeds a node limit: the parse limit, `max_tree_size` on the extracted body, or the
    /// cell budget of table construction (D-9).
    TreeTooLarge,
    /// The extracted text is shorter than the minimum.
    TooShort,
    /// The text is a duplicate of one seen before.
    Duplicate,
    /// The detected language does not match the target language.
    WrongLanguage,
    /// The raw input exceeds `max_input_bytes`, or a JSON node count or offset does not fit 32 bits.
    InputTooLarge,
    /// A value upstream raises on: either a `ValueError` or `TypeError` upstream catches (a tag name
    /// lxml refuses to create; it returns no result) or a raise that escapes `extract` (a span or
    /// character reference of more than 4300 digits, a URL `urljoin` rejects, a `hi` rend with no
    /// HTML tag, a heading level `int` rejects, a Discourse `posts` value that is not iterable).
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
mod tests {
    use super::*;

    #[test]
    fn caught_maps_errors() {
        assert_eq!(caught::<u8>(Ok(3)), Ok(Some(3)));
        for rejection in [
            Rejection::EmptyTree,
            Rejection::TreeTooLarge,
            Rejection::InvalidValue,
        ] {
            assert_eq!(
                caught::<u8>(Err(ExtractError::Rejected(rejection))),
                Ok(None)
            );
        }
        assert_eq!(
            caught::<u8>(Err(ExtractError::Cancelled)),
            Err(ExtractError::Cancelled)
        );
        assert_eq!(
            caught::<u8>(Err(ExtractError::DeadlineExceeded)),
            Err(ExtractError::DeadlineExceeded)
        );
    }
}
