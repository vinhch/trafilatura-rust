//! The compiled date patterns of htmldate's `core.py`.

use std::sync::LazyLock;

use regex::Regex;

use crate::pystr::compile_py;

use crate::htmldate::extractors::{DAY_RE, MONTH_RE, TIME_TZ_RE, YEAR_RE};
use crate::htmldate::utils::{NotAfter, Pattern};

/// A `Pattern` named `$name` over the lazily compiled regex `$regex` of `$source`, which is a `format!` or a
/// raw string.
macro_rules! pattern {
    ($name:ident, $regex:ident, $source:expr) => {
        static $regex: LazyLock<Option<Regex>> = LazyLock::new(|| compile_py(&$source));
        pub(crate) static $name: Pattern = Pattern {
            regex: &$regex,
            not_after: None,
        };
    };
}

// upstream: core.py::TIMESTAMP_PATTERN
pattern!(
    TIMESTAMP_PATTERN,
    TIMESTAMP_REGEX,
    format!(r"({YEAR_RE}-{MONTH_RE}-{DAY_RE})({TIME_TZ_RE})")
);

// upstream: core.py::TIMESTAMP_LOOSE_PATTERN
pattern!(
    TIMESTAMP_LOOSE_PATTERN,
    TIMESTAMP_LOOSE_REGEX,
    format!(r"({YEAR_RE}-{MONTH_RE}-{DAY_RE})({TIME_TZ_RE})?")
);

// upstream: core.py::THREE_COMP_REGEX_A
pattern!(
    THREE_COMP_REGEX_A,
    THREE_COMP_REGEX_A_REGEX,
    format!(r"({DAY_RE})[/.-]({MONTH_RE})[/.-]({YEAR_RE})")
);

// upstream: core.py::THREE_COMP_REGEX_B
pattern!(
    THREE_COMP_REGEX_B,
    THREE_COMP_REGEX_B_REGEX,
    format!(r"({DAY_RE})/({MONTH_RE})/([0-9]{{2}})|({DAY_RE})[.-]({MONTH_RE})[.-]([0-9]{{2}})")
);

// upstream: core.py::TWO_COMP_REGEX
pattern!(
    TWO_COMP_REGEX,
    TWO_COMP_REGEX_REGEX,
    format!(r"({MONTH_RE})[/.-]({YEAR_RE})")
);

// upstream: core.py::YEAR_PATTERN
pattern!(YEAR_PATTERN, YEAR_REGEX, format!(r"^[^@D@]?({YEAR_RE})"));

// upstream: core.py::COPYRIGHT_PATTERN
// bounded gap \D{0,99} (not \D*) avoids ReDoS
pattern!(
    COPYRIGHT_PATTERN,
    COPYRIGHT_REGEX,
    format!(r"(?:©|&copy;|Copyright|\(c\))[^@D@]{{0,99}}(?:{YEAR_RE})?-?({YEAR_RE})[^@D@]")
);

// upstream: core.py::THREE_PATTERN
pattern!(
    THREE_PATTERN,
    THREE_REGEX,
    r"/([0-9]{4}/[0-9]{2}/[0-9]{2})[01/]"
);

// upstream: core.py::THREE_LOOSE_PATTERN
pattern!(
    THREE_LOOSE_PATTERN,
    THREE_LOOSE_REGEX,
    r"[^@D@]([0-9]{4}[/.-][0-9]{2}[/.-][0-9]{2})[^@D@]"
);

// upstream: core.py::THREE_LOOSE_CATCH
pattern!(
    THREE_LOOSE_CATCH,
    THREE_LOOSE_CATCH_REGEX,
    r"([0-9]{4})[/.-]([0-9]{2})[/.-]([0-9]{2})"
);

// upstream: core.py::SELECT_YMD_PATTERN
pattern!(
    SELECT_YMD_PATTERN,
    SELECT_YMD_REGEX,
    format!(r"[^@D@]({DAY_RE}[/.-]{MONTH_RE}[/.-][0-9]{{4}})[^@D@]")
);

// upstream: core.py::SELECT_YMD_YEAR
pattern!(
    SELECT_YMD_YEAR,
    SELECT_YMD_YEAR_REGEX,
    format!(r"({YEAR_RE})[^@D@]?\n?\z")
);

// upstream: core.py::DATESTRINGS_PATTERN
pattern!(
    DATESTRINGS_PATTERN,
    DATESTRINGS_REGEX,
    r"([^@D@]19[0-9]{2}[01][0-9][0-3][0-9][^@D@]|[^@D@]20[0-9]{2}[01][0-9][0-3][0-9][^@D@])"
);

// upstream: core.py::DATESTRINGS_CATCH
pattern!(
    DATESTRINGS_CATCH,
    DATESTRINGS_CATCH_REGEX,
    format!(r"({YEAR_RE})([01][0-9])([0-3][0-9])")
);

// upstream: core.py::SLASHES_PATTERN
pattern!(
    SLASHES_PATTERN,
    SLASHES_REGEX,
    format!(r"[^@D@]({DAY_RE}/{MONTH_RE}/[0129][0-9]|[0-3][0-9]\.[01][0-9]\.[0129][0-9])[^@D@]")
);

// upstream: core.py::SLASHES_YEAR
pattern!(SLASHES_YEAR, SLASHES_YEAR_REGEX, r"([0-9]{2})\n?\z");

// upstream: core.py::YYYYMM_PATTERN
pattern!(
    YYYYMM_PATTERN,
    YYYYMM_REGEX,
    r"[^@D@]([12][0-9]{3}[/.-](?:1[0-2]|0[1-9]))[^@D@]"
);

// upstream: core.py::YYYYMM_CATCH
pattern!(
    YYYYMM_CATCH,
    YYYYMM_CATCH_REGEX,
    format!(r"({YEAR_RE})[/.-](1[0-2]|0[1-9])")
);

// upstream: core.py::MMYYYY_PATTERN
pattern!(
    MMYYYY_PATTERN,
    MMYYYY_REGEX,
    format!(r"[^@D@]({MONTH_RE}[/.-][12][0-9]{{3}})[^@D@]")
);

// `(?<!w3.org)` before the `\D`: a six code point lookbehind, `.` matching any code point but `\n`
static W3_ORG_WINDOW: LazyLock<Option<Regex>> = LazyLock::new(|| compile_py(r"\Aw3[^\n]org\z"));

// upstream: core.py::SIMPLE_PATTERN
pub(crate) static SIMPLE_PATTERN: Pattern = Pattern {
    regex: &SIMPLE_REGEX,
    not_after: Some(NotAfter {
        anchored: &W3_ORG_WINDOW,
        width: 6,
    }),
};
static SIMPLE_REGEX: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(&format!(r"[^@D@]({YEAR_RE})[^@D@]")));
