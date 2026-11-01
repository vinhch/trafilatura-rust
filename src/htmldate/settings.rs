//! The module-wide settings of htmldate 1.11.0 (`settings.py`) that the port reads. `CACHE_SIZE` and
//! `MAX_FILE_SIZE` belong to the lru caches and the download, which the port does not have.

use crate::pydatetime::DateTime;

// upstream: settings.py::MIN_DATE
/// The earliest plausible date, 1995-01-01 (inclusive), naive.
pub(crate) const MIN_DATE: DateTime = DateTime {
    year: 1995,
    month: 1,
    day: 1,
    hour: 0,
    minute: 0,
    second: 0,
    microsecond: 0,
    offset: None,
};

// upstream: settings.py::MAX_POSSIBLE_CANDIDATES
/// The most date candidates a search keeps.
pub(crate) const MAX_POSSIBLE_CANDIDATES: usize = 1000;

// upstream: settings.py::MIN_SEGMENT_LEN
/// The shortest text segment a date is searched in, in code points.
pub(crate) const MIN_SEGMENT_LEN: usize = 6;

// upstream: settings.py::MAX_SEGMENT_LEN
/// The longest text segment a date is searched in, in code points.
pub(crate) const MAX_SEGMENT_LEN: usize = 52;

// upstream: settings.py::CLEANING_LIST
/// The elements `clean_html` deletes before the text search.
pub(crate) const CLEANING_LIST: &[&str] = &[
    "applet", "audio", "canvas", "datalist", "embed", "frame", "frameset", "iframe", "label",
    "map", "math", "noframes", "object", "picture", "rdf", "svg", "track", "video",
];
