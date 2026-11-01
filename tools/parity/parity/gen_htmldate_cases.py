"""Generates src/pydatetime/cases.rs (what CPython 3.13's datetime and strptime return on fixed inputs),
src/dom/html_cases.rs (what lxml.html.tostring returns on fixed pages, `serialize_cases()`) and
src/htmldate/validator_cases.rs (htmldate's validators on fixed inputs, `validator_cases()`) and
src/htmldate/extractor_cases.rs (htmldate's string extractors on fixed inputs, `extractor_cases()`) and
src/htmldate/search_cases.rs (htmldate's page-text search on fixed pages, `search_cases()`) and
src/htmldate/examine_cases.rs (htmldate's tree examiners on fixed pages, `examine_cases()`) and
src/htmldate/find_date_cases.rs (htmldate's `find_date` on fixed pages, `find_date_cases()`).

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_htmldate_cases import datetime_cases; datetime_cases()' > src/pydatetime/cases.rs

Each table holds the value Python returns, or `None` where it raises ValueError (or OverflowError or OSError
for a timestamp). The crate's `DateTime` holds the wall clock and the UTC offset in microseconds; a naive
value has no offset.

`oracle_setup()` runs first in every case function. It pins the zone to UTC and turns off the two htmldate
calls that are not ported (D-13): `_external_date` finds nothing and `dateutil_parse` raises.
"""

import itertools
import os
import random
import sys
import time
from datetime import datetime, timedelta, timezone

from parity.gen_xpath_cases import rust_str

UTC = timezone.utc
T = "2020-01-01T10:20:30"

ISOFORMAT_INPUTS = [
    "2020-01-01",
    "2020-01-01T10:20:30",
    "2020-01-01 10:20",
    "2020-01-01T10",
    "2020-01-01T10:20:30.5",
    "2020-01-01T10:20:30.1234567",
    "2020-01-01T10:20:30,5",
    T + "Z",
    T + "+05:30",
    T + "+0530",
    T + "+05",
    T + "-00:00",
    T + "+05:30:15.5",
    T + "+24:00",
    "2020-W01-1",
    "2020W011",
    "20200101",
    "20200101T102030",
    "2020-1-01",
    "2020-02-30",
    "2020-01-01T24:00",
    "2020-01-01X10:20",
    "2020-01-01é" + "10:20",
    "２０２０-01-01",
    "2020",
    "2020-01",
    "2020-01-01 ",
    "",
    "0000-01-01",
    # Calendar and offset bounds: an ISO week 53 year, the last week 53 day of a 52-week year, the offset limits.
    "2020-W53-1",
    "2021-W53-1",
    T + "-24:00",
    T + "+23:59:59.999999",
    T + ".",
    # Where C and the pure-Python _pydatetime differ: a separator with nothing after it, a fraction right after the
    # hour, a second `:` before the fraction, a zone with leftover text, a `Z` before digits.
    "2020-01-01T",
    "2020-01-011",
    "2020-W01 ",
    "2020-01-01T06.8",
    "2020-01-01T10:20:30:5",
    "2020-01-01T10:20:30.123456x+05:00",
    "2020-01-01T10:20Z05",
    "2020-01-01T10:20-05Z",
    "2020-01-01T10+05",
    "2020-01-01T10:20:30.5x",
    # A fraction mark right before a zone: CPython 3.13.13 accepts these, the pinned 3.13.16 oracle rejects them.
    "2020-01-01T10:20:30.+05:00",
    "2020-01-01T10:20:30,Z",
    "2020-01-01T10:20:30.-05:00",
    "2020-01-01T10.+05:00",
]

# A seeded sample of strings over the characters the grammar reads, so the table covers more than the rows above.
FUZZ_SEED = 20261101
FUZZ_COUNT = 300
FUZZ_TAILS = ["2020-01-01", "2020W011", "20200101", "2020-W01-1", "2020-01-01T", "2020-01-01 "]
FUZZ_TAIL_CHARS = "0123456789:.,+-Z T"
FUZZ_CHARS = "0123456789-:T +.,ZWxé１"

STRPTIME_YMD_INPUTS = [
    "2020-1-5",
    "2020-01-01",
    " 2020-01-01",
    "2020-01- 5",
    "2020-13-01",
    "２０２０-01-01",
    "2020-02-29",
    "2019-02-29",
    "2020-01-011",
    "20-01-01",
]

TIMESTAMP_INPUTS = [
    0,
    -1,
    -86400,
    10**10,
    253402300799,
    253402300800,
    -62135596800,
    -62135596801,
]

NEW_INPUTS = [
    (2020, 2, 29),
    (2019, 2, 29),
    (0, 1, 1),
    (10000, 1, 1),
    (2020, 0, 1),
    (2020, 12, 31),
    (1, 1, 1),
    (9999, 12, 31),
]


def oracle_setup() -> None:
    os.environ["TZ"] = "UTC"
    time.tzset()
    assert datetime(2020, 1, 1).timestamp() == 1577836800
    import htmldate.extractors as extractors

    def external_date(string):
        return None

    def dateutil_parse(string, fuzzy=False):
        raise ValueError("D-13: dateutil_parse is not ported")

    extractors._external_date = external_date
    extractors.dateutil_parse = dateutil_parse


def version() -> str:
    return ".".join(map(str, sys.version_info[:3]))


def offset_micros(value: timedelta) -> int:
    return (value.days * 86400 + value.seconds) * 10**6 + value.microseconds


def struct(value: datetime) -> str:
    "A `DateTime` struct literal."
    offset = value.utcoffset()
    micros = "None" if offset is None else f"Some({offset_micros(offset)})"
    return (
        f"DateTime {{ year: {value.year}, month: {value.month}, day: {value.day}, "
        f"hour: {value.hour}, minute: {value.minute}, second: {value.second}, "
        f"microsecond: {value.microsecond}, offset: {micros} }}"
    )


def literal(value: datetime | None) -> str:
    "An `Option<DateTime>` literal."
    return "None" if value is None else f"Some({struct(value)})"


def attempt(function, *args):
    "The value of `function(*args)`, or None where Python raises ValueError, OverflowError or OSError."
    try:
        return function(*args)
    except (ValueError, OverflowError, OSError):
        return None


def strftime_inputs() -> list[datetime]:
    offsets = [None, UTC]
    offsets += [
        timezone(timedelta(hours=5, minutes=30)),
        timezone(timedelta(hours=5, minutes=30, seconds=15)),
        timezone(timedelta(hours=5, minutes=30, seconds=15, microseconds=1)),
        timezone(-timedelta(hours=5, minutes=30)),
    ]
    values = [datetime(2020, 1, 1, 10, 20, 30, microsecond, tzinfo=zone) for zone in offsets for microsecond in (0, 7)]
    # glibc does not pad a year below 1000
    values += [datetime(999, 1, 1), datetime(1, 1, 1, tzinfo=timezone(timedelta(hours=-1)))]
    return values


def isoformat_lines(inputs: list[str]) -> list[str]:
    lines = []
    for text in inputs:
        value = attempt(datetime.fromisoformat, text)
        lines.append(f"    ({rust_str(text)}, {literal(value)}),")
    return lines


def fuzz_inputs() -> list[str]:
    "Seeded strings: random over the grammar's characters, and tails after date forms."
    rng = random.Random(FUZZ_SEED)
    inputs = ["".join(rng.choice(FUZZ_CHARS) for _ in range(rng.randint(0, 24))) for _ in range(FUZZ_COUNT)]
    for _ in range(FUZZ_COUNT):
        head = rng.choice(FUZZ_TAILS)
        inputs.append(head + "".join(rng.choice(FUZZ_TAIL_CHARS) for _ in range(rng.randint(0, 12))))
    return inputs


def strptime_lines() -> list[str]:
    lines = []
    for text in STRPTIME_YMD_INPUTS:
        value = attempt(datetime.strptime, text, "%Y-%m-%d")
        lines.append(f"    ({rust_str(text)}, {literal(value)}),")
    return lines


def timestamp_lines() -> list[str]:
    lines = []
    for seconds in TIMESTAMP_INPUTS:
        value = attempt(lambda s: datetime.fromtimestamp(s, tz=UTC), seconds)
        lines.append(f"    ({seconds}, {literal(value)}),")
    return lines


def new_lines() -> list[str]:
    lines = []
    for year, month, day in NEW_INPUTS:
        value = attempt(datetime, year, month, day)
        lines.append(f"    ({year}, {month}, {day}, {literal(value)}),")
    return lines


def strftime_lines() -> list[str]:
    lines = []
    for value in strftime_inputs():
        ymd = value.strftime("%Y-%m-%d")
        reference = value.strftime("%Y-%m-%dT%H:%M:%S.%f%z")
        lines.append(f"    ({struct(value)}, {rust_str(ymd)}, {rust_str(reference)}),")
    return lines


def datetime_cases() -> None:
    oracle_setup()
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import datetime_cases; datetime_cases()' > src/pydatetime/cases.rs",
        "// CPython 3.13's datetime and strptime on fixed inputs, in UTC with the htmldate calls of D-13 turned off.",
        "",
        "use super::DateTime;",
        "",
        "/// `datetime.fromisoformat(text)`.",
        "#[rustfmt::skip]",
        "pub(super) const ISOFORMAT_CASES: &[(&str, Option<DateTime>)] = &[",
        *isoformat_lines(ISOFORMAT_INPUTS),
        "];",
        "",
        "/// `datetime.fromisoformat(text)` on the seeded strings of `fuzz_inputs()`.",
        "#[rustfmt::skip]",
        "pub(super) const FUZZ_ISOFORMAT_CASES: &[(&str, Option<DateTime>)] = &[",
        *isoformat_lines(fuzz_inputs()),
        "];",
        "",
        "/// `datetime.strptime(text, \"%Y-%m-%d\")`.",
        "#[rustfmt::skip]",
        "pub(super) const STRPTIME_YMD_CASES: &[(&str, Option<DateTime>)] = &[",
        *strptime_lines(),
        "];",
        "",
        "/// `datetime.fromtimestamp(seconds, tz=timezone.utc)`.",
        "#[rustfmt::skip]",
        "pub(super) const TIMESTAMP_CASES: &[(i64, Option<DateTime>)] = &[",
        *timestamp_lines(),
        "];",
        "",
        "/// `datetime(year, month, day)`.",
        "#[rustfmt::skip]",
        "pub(super) const NEW_CASES: &[(i64, i64, i64, Option<DateTime>)] = &[",
        *new_lines(),
        "];",
        "",
        "/// A value, then its `strftime(\"%Y-%m-%d\")` and its `strftime(\"%Y-%m-%dT%H:%M:%S.%f%z\")`.",
        "#[rustfmt::skip]",
        "pub(super) const STRFTIME_CASES: &[(DateTime, &str, &str)] = &[",
        *strftime_lines(),
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


# `Tree::tostring_html` (src/dom/html_cases.rs): whole pages, parsed by html5tree; each row covers one rule of
# libxml2 2.14.6 HTMLtree.c as lxml.html.tostring runs it. `isindex` has no page row: html5lib expands it into a
# search form, which the crate's parser (html5ever) does not, so its void rule is an element row.
VOID_NAMES = "area base basefont br col frame hr img input link meta param".split()
NON_VOID_NAMES = "embed wbr source track".split()
BOOLEAN_NAMES = "checked compact declare defer disabled ismap multiple nohref noresize noshade nowrap readonly selected".split()
TOSTRING_PAGES = [
    *[f"<div>a<{name} data-x=1>b</div>" for name in VOID_NAMES],
    *[f"<div>a<{name}>b</div>" for name in NON_VOID_NAMES],
    "<table><colgroup><col></colgroup></table>",
    "<table><tr><td></td><td>x</td></tr></table>",
    "<ul><li></li><li>x</li></ul>",
    "<dl><dt></dt><dd></dd></dl>",
    "<select><option></option><option>o</option></select>",
    "<p></p><span></span><div></div>",
    "<p>a &amp; b &lt; c &gt; d \" '   é &#13; e</p>",
    "<div><p>x</p>a &amp; &lt; &gt; \" ' tail</div>",
    "<script>if (a < b && c) x('&amp;');</script>",
    '<style>p > a { content: "<" } & </style>',
    '<textarea>a &amp; <b> "x"</textarea>',
    "<title>a &amp; <b></title>",
    "<xmp>a &amp; <b>x</b></xmp>",
    "<p a='q\"r' b=\"s't\" c=\"u&quot;v'w\">x</p>",
    '<p a="&{x} & y &{ z">x</p>',
    '<p a="<!--x--> <y> & z">x</p>',
    "<p alt>x</p>",
    '<p a="">x</p>',
    '<p a="x&#13;y&#9;z&#10;w">x</p>',
    *[f'<p {name}="v">x</p>' for name in BOOLEAN_NAMES],
    '<p CHECKED="v" data-checked="w">x</p>',
    "<p checked>x</p>",
    '<li selected="x">x</li>',
    '<a href=" a b">x</a>',
    '<a href="\ta\tb">x</a>',
    '<a href="\n\r a">x</a>',
    '<a href="é">x</a>',
    '<a href="100%">x</a>',
    '<a href="#x">x</a>',
    '<a href="a&amp;b&quot;c&lt;d">x</a>',
    '<div><link href="é"></div>',
    '<svg><a href="é b"></a></svg>',
    '<img src="é b" alt="x">',
    '<form action="é b"><input name="a b"><input name="é"></form>',
    '<a name="a b">x</a><a name="é">y</a>',
    '<img srcset="a b é, c%d 2x">',
    '<p>x<svg viewBox="0 0 1 1"><path d="M1"/>tail</svg>y</p>',
    "<svg><foreignObject><p>x</p></foreignObject></svg>",
    "<math><mi>x</mi></math>",
    "<div><svg></svg><svg></svg></div>",
    "<svg><svg></svg></svg>",
    '<html><head><meta http-equiv="Content-Type" content="text/html; charset=utf-8"><title>t</title></head><body>x</body></html>',
    '<html><head><meta http-equiv="content-type" content="text/html; charset=utf-8"><title>t</title></head><body>x</body></html>',
    '<div>a<meta http-equiv="Content-Type"\ncontent="x">b</div>',
    '<script><meta http-equiv="Content-Type" content="x">var a;</script>',
    # Attribute comment regions: `<!--` closes at the first `-->` found from its third byte, raw inside, `"` escaped
    '<p a="<!-->x-->">x</p>',
    '<p a="<!-->">x</p>',
    '<p a="<!--->">x</p>',
    '<p a="<!--a&b-->">x</p>',
    "<p a='<!--a\"b&quot;c-->'>x</p>",
    '<p a="<!--<!--x-->">x</p>',
    '<p a="<!--a>b">x</p>',
    '<p a="<!--a--\'>&quot;b">x</p>',
    '<p a="&{x}y&">x</p>',
    "<p a=\"&{x}'y&quot;\">x</p>",
    '<p a="&{a">x</p>',
    # href bytes: printable ASCII but space kept, every other byte as %XX
    '<a href="x&quot;y\'z">x</a>',
    "<a href=\"x y\">x</a>",
    "<a href=\"x\U0001F600y\">x</a>",
    "<a href=\"x\u007fy\">x</a>",
    '<a href=" ">x</a>',
    '<a href=" \t\n a b\t">x</a>',
    '<a href="">x</a>',
    # Foreign elements: raw script and style text by name, void and URI rules only for HTML elements
    "<svg><script>a&lt;b</script></svg>",
    "<svg><style>a&lt;b</style></svg>",
    # The tail of a child of a raw text element is raw, in the element's namespace or not
    "<svg><style><g></g>a&lt;b</style><script><g></g>c&lt;d</script></svg>",
    "<svg><style>x<g></g>a&amp;b<g></g></style></svg>",
    "<svg><area></area><track>x</track></svg>",
    "<svg><base>t</base></svg>",
    '<svg><path checked="v" selected=v nowrap=x></path></svg>',
    '<math><mi checked=v href="é b">x</mi></math>',
    '<svg viewBox="a&amp;b<c">a&amp;b&lt;c</svg>',
    "<math><svg></svg></math>",
]
# Elements built with lxml (`etree.Element`), with text None or a string: the empty-element rules without a parser.
TOSTRING_ELEMENTS = [
    ("li", None),
    ("li", ""),
    ("li", "x"),
    ("p", None),
    ("p", ""),
    ("script", None),
    ("script", ""),
    ("img", "x"),
    ("br", ""),
    ("embed", None),
    ("isindex", None),
]
# `Tree::tostring_html` with `with_tail` on the first `<p>`: the page, its output with the tail, its output without.
TOSTRING_TAIL_PAGES = [
    "<div><p>a</p>TAIL<span>s</span></div>",
    "<div><p>a</p></div>",
]


def serialize_cases() -> None:
    from lxml import etree
    from lxml import html as lxml_html
    from parity import html5tree

    oracle_setup()
    lines = []
    for page in TOSTRING_PAGES:
        tree = html5tree.parse_document(page)
        assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {page[:80]!r}"
        expected = lxml_html.tostring(tree, encoding="unicode")
        lines.append(f"    ({rust_str(page)}, {rust_str(expected)}),")
    element_lines = []
    for name, text in TOSTRING_ELEMENTS:
        element = etree.Element(name)
        if text is not None:
            element.text = text
        expected = lxml_html.tostring(element, encoding="unicode")
        text_literal = "None" if text is None else f"Some({rust_str(text)})"
        element_lines.append(f"    ({rust_str(name)}, {text_literal}, {rust_str(expected)}),")
    tail_lines = []
    for page in TOSTRING_TAIL_PAGES:
        tree = html5tree.parse_document(page)
        assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {page[:80]!r}"
        p = tree.find(".//p")
        assert p is not None, f"no <p> in {page!r}"
        with_tail = lxml_html.tostring(p, encoding="unicode", with_tail=True)
        without_tail = lxml_html.tostring(p, encoding="unicode", with_tail=False)
        tail_lines.append(f"    ({rust_str(page)}, {rust_str(with_tail)}, {rust_str(without_tail)}),")
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import serialize_cases; serialize_cases()' > src/dom/html_cases.rs",
        "// lxml.html.tostring (libxml2 2.14.6, method html) on whole pages parsed with the HTML5 algorithm, and on elements.",
        "",
        '/// A page, then `lxml.html.tostring(html, encoding="unicode")` of its `html` element.',
        "#[rustfmt::skip]",
        "pub(super) const TOSTRING_HTML_CASES: &[(&str, &str)] = &[",
        *lines,
        "];",
        "",
        '/// An element name and its text (`None` for no text), then `lxml.html.tostring(element, encoding="unicode")`.',
        "#[rustfmt::skip]",
        "pub(super) const TOSTRING_HTML_ELEMENT_CASES: &[(&str, Option<&str>, &str)] = &[",
        *element_lines,
        "];",
        "",
        "/// A page, then `lxml.html.tostring` of its first `p` with `with_tail` true, then with `with_tail` false.",
        "#[rustfmt::skip]",
        "pub(super) const TOSTRING_HTML_TAIL_CASES: &[(&str, &str, &str)] = &[",
        *tail_lines,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


# htmldate's validators (src/htmldate/validator_cases.rs): date strings, reference comparison, the YMD filter and
# the date-input check. The bounds are MIN_DATE and a max_date of "2024-01-31" (midnight), as the Rust tests build them.
VALIDATOR_FORMAT = "%Y-%m-%d"
VALIDATOR_EARLIEST = datetime(1995, 1, 1)
VALIDATOR_LATEST = datetime(2024, 1, 31)
VALIDATOR_DEFAULT = datetime(2000, 1, 1)

IS_VALID_DATE_STR_INPUTS = [
    "2020-01-15",
    "2020-1-15",
    "2020-11-1",
    "2020-01-1x",
    " 2020-01-15",
    "2020-+1-15",
    "2020-01-32",
    "2020-02-30",
    "2020-01-15T10:00",
    "1994-12-31",
    "1995-01-01",
    "2024-01-31",
    "2024-02-01",
    "２０２０-01-15",
    "2015abc-01-01",
    "20151-01-01",
    "0999-01-01",
    "9999-01-01",
    "2020-01-",
    "",
]

# (reference, attempt, original): the reference is a datetime string or None, the attempt is in REFERENCE_FORMAT
COMPARE_VALUES_INPUTS = [
    (None, "2020-01-01T10:20:30.000000", True),
    (None, "2020-01-01T10:20:30.000000+0000", False),
    (None, "2020-01-01T10:20:30.000007+0530", True),
    ("2020-01-01T10:00:00.000000+0100", "2020-01-01T10:00:00.000000+0000", True),
    ("2020-01-01T10:00:00.000000+0100", "2020-01-01T10:00:00.000000+0000", False),
    ("2020-01-01T10:00:00.000000", "2019-12-31T23:00:00.000000+0000", True),
    ("2020-01-01T10:00:00.000000", "2019-12-31T23:00:00.000000+0000", False),
    ("2020-01-01T10:00:00.000000", "2021-01-01T00:00:00.000000", False),
    ("2020-01-01T10:00:00.000000", "2021-01-01T00:00:00.000000", True),
    (None, "garbage", True),
    ("2020-01-01T10:00:00.000000", "garbage", True),
    (None, "999-01-01T00:00:00.000000", True),
    ("2020-01-01T10:00:00.000000", "999-01-01T00:00:00.000000", True),
    (None, "2020-01-01T10:00:00.000000+053015.000001", True),
]

# (bestmatch groups or None, copyear)
FILTER_YMD_INPUTS = [
    (None, 0),
    (("2020", "01", "15"), 0),
    (("2020", "01", "15"), 2019),
    (("2020", "01", "15"), 2020),
    (("2020", "01", "15"), 2021),
    (("2019", "12", "31"), 2020),
    (("2020", "1", "5"), 0),
    (("2020", "11", "1"), 0),
    (("2020", "02", "30"), 0),
    (("2024", "01", "31"), 0),
    (("2024", "02", "01"), 0),
    (("1994", "12", "31"), 0),
    (("1995", "01", "01"), 0),
    (("２０２０", "01", "15"), 0),
    (("２０２０", "01", "15"), 2021),
]

CHECK_DATE_INPUT_INPUTS = [None, "2024-01-31", "2024-01-31T10:00+02:00", "garbage", "", "2024-1-31"]


def validator_cases() -> None:
    oracle_setup()
    from htmldate.utils import Extractor
    from htmldate.validators import check_date_input, compare_values, filter_ymd_candidate, is_valid_date

    is_valid_lines = [
        f"    ({rust_str(text)}, {str(is_valid_date(text, VALIDATOR_FORMAT, VALIDATOR_EARLIEST, VALIDATOR_LATEST)).lower()}),"
        for text in IS_VALID_DATE_STR_INPUTS
    ]
    compare_lines = []
    for reference, attempt, original in COMPARE_VALUES_INPUTS:
        before = None if reference is None else datetime.fromisoformat(reference)
        options = Extractor(
            extensive=False, max=VALIDATOR_LATEST, min=VALIDATOR_EARLIEST, original=original, format=VALIDATOR_FORMAT
        )
        result = compare_values(before, attempt, options)
        compare_lines.append(
            f"    ({literal(before)}, {rust_str(attempt)}, {str(original).lower()}, {literal(result)}),"
        )
    filter_lines = []
    for bestmatch, copyear in FILTER_YMD_INPUTS:
        groups = None if bestmatch is None else tuple(bestmatch)
        result = filter_ymd_candidate(groups, copyear, VALIDATOR_FORMAT, VALIDATOR_EARLIEST, VALIDATOR_LATEST)
        rust_groups = "None" if bestmatch is None else f"Some([{', '.join(rust_str(g) for g in bestmatch)}])"
        rust_result = "None" if result is None else f"Some({rust_str(result)})"
        filter_lines.append(f"    ({rust_groups}, {copyear}, {rust_result}),")
    check_lines = []
    for text in CHECK_DATE_INPUT_INPUTS:
        result = check_date_input(text, VALIDATOR_DEFAULT)
        rust_text = "None" if text is None else f"Some({rust_str(text)})"
        check_lines.append(f"    ({rust_text}, {struct(result)}),")
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import validator_cases; validator_cases()' > src/htmldate/validator_cases.rs",
        "// htmldate 1.11.0's validators on fixed inputs, in UTC with the htmldate calls of D-13 turned off.",
        "",
        "use crate::pydatetime::DateTime;",
        "",
        "/// `is_valid_date(text, \"%Y-%m-%d\", MIN_DATE, 2024-01-31)`.",
        "#[rustfmt::skip]",
        "pub(super) const IS_VALID_DATE_STR_CASES: &[(&str, bool)] = &[",
        *is_valid_lines,
        "];",
        "",
        "/// `compare_values(reference, attempt, options)` with `options.original` as given, then the returned reference.",
        "#[rustfmt::skip]",
        "pub(super) const COMPARE_VALUES_CASES: &[(Option<DateTime>, &str, bool, Option<DateTime>)] = &[",
        *compare_lines,
        "];",
        "",
        "/// `filter_ymd_candidate(bestmatch, copyear, \"%Y-%m-%d\", MIN_DATE, 2024-01-31)`.",
        "#[allow(clippy::type_complexity)]",
        "#[rustfmt::skip]",
        "pub(super) const FILTER_YMD_CASES: &[(Option<[&str; 3]>, i64, Option<&str>)] = &[",
        *filter_lines,
        "];",
        "",
        "/// `check_date_input(text, 2000-01-01)`, `None` for no input.",
        "#[rustfmt::skip]",
        "pub(super) const CHECK_DATE_INPUT_CASES: &[(Option<&str>, DateTime)] = &[",
        *check_lines,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


# htmldate's string extractors (src/htmldate/extractor_cases.rs): try_date_expr, regex_parse, extract_url_date,
# idiosyncrasies_search and pattern_search on fixed inputs, with min_date 1995-01-01 and max_date 2024-01-31.
EXTRACTOR_MIN = datetime(1995, 1, 1)
EXTRACTOR_MAX = datetime(2024, 1, 31)

TRY_DATE_EXPR_INPUTS = [
    "2017-09-01",
    "2017-09-01T10:20:30",
    "2017-09-01T10:20:30+05:30",
    "2017-09-01T10:20:30Z",
    "2017-09-01T10:20:30-0500",
    "2017-09-01 10:20",
    "2024-01-31T23:30:00-05:00",
    "201709011234",
    "20170901",
    "2017-9-1",
    "1.9.17",
    "01/09/2017",
    "13/25/2017",
    "9/2017",
    "2017/09",
    "September 1, 2017",
    "1st of September 2017",
    "1. September 2017",
    "SmithMarch 5, 2020",
    "Şubat 3, 2020",
    "3 Kasım 2020",
    "İnce 3, 2020",
    "12:30",
    "2017",
    "$ 2017-09-01",
    "ABC 2017-09-01",
    "+49 2017-09-01",
    "x.com 2017-09-01",
    "DE89 2017-09-01",
    "2017-09-01®",
    # 60 code points, the date starts at 50 and is cut by the 52-code-point segment
    "a" * 49 + " 2017-09-01",
    # the date starts after the 52-code-point segment
    "a" * 52 + " 2017-09-01",
    # digits that `\d`, `isdigit` and `int` read differently
    "２０２０-01-15",
    "٢٠٢٠-٠١-١٥",
    "²⁰²⁰0115",
    "Veröffentlicht am 1.9.17",
    # a string only dateparser reads: no date here (D-13)
    "15 de marzo de 2017",
]

REGEX_PARSE_INPUTS = [
    "March 5, 2020",
    "5 March 2020",
    "5th of March 2020",
    "5. März 2020",
    "March 5 2020",
    "Mar 5, 2020",
    "MARCH 5, 2020",
    "SmithMarch 5, 2020",
    "xMarch 5, 2020",
    "Junk1March 5, 2020",
    "Şubat 3, 2020",
    "3 Kasım 2020",
    "ŞUBAT 3, 2020",
    "mayıs 5, 2020",
    "1 Ocak 2020",
    "DEC 31, 2019",
    "Dezember 31, 2019",
    "February 30, 2020",
    "Foo 5, 2020",
    "abc 5, 2020",
    "abc 5, 2020 March 5, 2020",
    "Sept 5, 2020",
    "5 de March 2020",
    "İnce 3, 2020",
    "15 de marzo de 2017",
    "a" * 50 + " 5, 2020",
]

EXTRACT_URL_DATE_INPUTS = [
    "https://example.com/2017/09/01/",
    "https://example.com/blog_2017-9-1.html",
    "https://example.com/2017/02/30/",
    "https://example.com/2017/13/01/",
    "https://example.com/2045/09/01/",
    "https://example.com/about",
    None,
]

IDIOSYNCRASIES_INPUTS = [
    "date: 01.09.2017",
    "DATE: 01.09.2017",
    "updated 01.09.2017",
    "last-modified 2017.09.01",
    "published 01.09.2017",
    "posted 01/09/17",
    "on 01.09.2017",
    "Stand: 1.9.17",
    "Datum 1.9.17",
    "Veröffentlicht am 1.9.17",
    "yayınlanma tarihi: 01.09.2017",
    "yayinlanma tarihi: 01.09.2017",
    "Yayımlanma tarihi: 01.09.2017",
    "güncelleme tarihi: 01.09.2017",
    "01.09.2017 tarihinde yayInlandi",
    "01.09.2017'de güncellendi",
    # the gap after a cue is at most 20 code points
    "date" + "x" * 19 + " 01.09.2017",
    "date" + "x" * 20 + " 01.09.2017",
    # a cue 61 code points before its core: outside the window
    "on" + " " * 59 + "01.09.2017",
    # non-ASCII text before the window
    "ü" * 100 + " date: 01.09.2017",
    # more than 1,000 cores, the cue after the last one: the unbounded search finds it
    "01.09.2017 " * 1500 + "date: 02.09.2017",
    # more than 1,000 cores and no cue
    "01.09.2017 " * 1500,
]

PATTERN_SEARCH_INPUTS = [
    ('{"datePublished": "2017-09-01T10:20:30+05:30"}', True),
    ('{"dateModified": "2017-09-01"}', False),
    ('{"dateModified": "2017-09-01"}', True),
    ('{"datepublished":"2017-09-01"}', True),
    ('{"DATEPUBLISHED": "2017-09-01"}', True),
    ('{"datePublıshed": "2017-09-01"}', True),
    ('{"dateModıfied": "2017-09-01"}', False),
    ('{"datePublished": "2045-12-31"}', True),
    ('{"datePublished": "2024-02-01"}', True),
    ('{"datePublished": "1995-01-01T00:00:00Z"}', True),
    ('{"datePublished": "2017-9-1"}', True),
    ('{"datePublished": "2017-09-01T10:20:30.5Z"}', True),
]


def rust_option_str(value: str | None) -> str:
    return "None" if value is None else f"Some({rust_str(value)})"


def extractor_cases() -> None:
    oracle_setup()
    from htmldate.extractors import (
        JSON_MODIFIED,
        JSON_PUBLISHED,
        extract_url_date,
        idiosyncrasies_search,
        pattern_search,
        regex_parse,
        try_date_expr,
    )
    from htmldate.validators import REFERENCE_FORMAT
    from htmldate.utils import Extractor

    def options(extensive: bool = False, original: bool = False) -> Extractor:
        return Extractor(
            extensive=extensive, max=EXTRACTOR_MAX, min=EXTRACTOR_MIN, original=original, format=VALIDATOR_FORMAT
        )

    formats = [("Format::Ymd", VALIDATOR_FORMAT), ("Format::Reference", REFERENCE_FORMAT)]
    try_lines = []
    for text in TRY_DATE_EXPR_INPUTS:
        for variant, fmt in formats:
            for extensive in (True, False):
                result = try_date_expr(text, fmt, extensive, EXTRACTOR_MIN, EXTRACTOR_MAX)
                try_lines.append(
                    f"    ({rust_str(text)}, {variant}, {str(extensive).lower()}, {rust_option_str(result)}),"
                )
    regex_lines = []
    for text in REGEX_PARSE_INPUTS:
        regex_lines.append(f"    ({rust_str(text)}, {literal(regex_parse(text))}),")
    url_lines = []
    for text in EXTRACT_URL_DATE_INPUTS:
        result = extract_url_date(text, options())
        url_lines.append(f"    ({rust_option_str(text)}, {rust_option_str(result)}),")
    idiosyncrasy_lines = []
    for text in IDIOSYNCRASIES_INPUTS:
        result = idiosyncrasies_search(text, options())
        idiosyncrasy_lines.append(f"    ({rust_str(text)}, {rust_option_str(result)}),")
    pattern_lines = []
    for text, published in PATTERN_SEARCH_INPUTS:
        pattern = JSON_PUBLISHED if published else JSON_MODIFIED
        result = pattern_search(text, pattern, options())
        pattern_lines.append(f"    ({rust_str(text)}, {str(published).lower()}, {rust_option_str(result)}),")
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import extractor_cases; extractor_cases()' > src/htmldate/extractor_cases.rs",
        "// htmldate 1.11.0's string extractors on fixed inputs, in UTC with the htmldate calls of D-13 turned off.",
        "",
        "use super::validators::Format;",
        "use crate::pydatetime::DateTime;",
        "",
        "/// A string, the output format, `extensive_search`, then `try_date_expr(string, format, extensive, 1995-01-01, 2024-01-31)`.",
        "#[rustfmt::skip]",
        "pub(super) const TRY_DATE_EXPR_CASES: &[(&str, Format, bool, Option<&str>)] = &[",
        *try_lines,
        "];",
        "",
        "/// A string, then `regex_parse(string)`.",
        "#[rustfmt::skip]",
        "pub(super) const REGEX_PARSE_CASES: &[(&str, Option<DateTime>)] = &[",
        *regex_lines,
        "];",
        "",
        "/// A URL, then `extract_url_date(url, options)`.",
        "#[rustfmt::skip]",
        "pub(super) const EXTRACT_URL_DATE_CASES: &[(Option<&str>, Option<&str>)] = &[",
        *url_lines,
        "];",
        "",
        "/// A page text, then `idiosyncrasies_search(text, options)`.",
        "#[rustfmt::skip]",
        "pub(super) const IDIOSYNCRASIES_CASES: &[(&str, Option<&str>)] = &[",
        *idiosyncrasy_lines,
        "];",
        "",
        "/// A text, whether its pattern is JSON_PUBLISHED (else JSON_MODIFIED), then `pattern_search(text, pattern, options)`.",
        "#[rustfmt::skip]",
        "pub(super) const PATTERN_SEARCH_CASES: &[(&str, bool, Option<&str>)] = &[",
        *pattern_lines,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


# htmldate's page-text search (src/htmldate/search_cases.rs): search_page on fixed pages, examine_text on text
# segments, and search_page on a 20,000-run of "w3.org/2020/" (the SIMPLE_PATTERN lookbehind retry). The pages use
# min_date 1995-01-01 and max_date 2024-01-31; SEARCH_PAGE_MAX_2045_CASES uses max_date 2045-12-31.
SEARCH_PAGE_INPUTS = [
    ("<p>© 2015-2019 Example</p>", False),
    ("<p>&copy; 2019</p>", False),
    ("<p>(c) 2015-2019</p>", True),
    ("<p>Copyright " + "x" * 120 + " 2019 </p>", False),
    ("<p>/2019/05/12/</p>", False),
    ("<p> 2019.05.12 </p>", False),
    ("<p>12.05.2019</p>", False),
    ("<p>12-5-2019 </p>", False),
    ("<p>20190512</p>", False),
    ("<p>12/05/19</p>", False),
    ("<p>2019-05</p>", False),
    ("<p>05/2019</p>", False),
    ("<p>May 12, 2019</p>", False),
    ("<p>Copyright 2010 Example</p>", False),
    ("<p>© 2015-2019</p><p>May 12, 2019</p>", True),
    ("<p>© 2015-2019</p><p>May 12, 2018</p>", False),
    ('<svg xmlns="http://www.w3.org/2000/svg"><p>x</p></svg>', False),
    ('<p>xmlns:ns0="http://www.w3.org/1998/Math/MathML" x 2019 y</p>', False),
    (" w3xorg/2020/ ", False),
    (" w3\norg/2020/ ", False),
    ("<p>/2030/05/12/</p>", False),
    # runner-up rules: counts 3 and 2 (other year), 3 and 1, a tie; each with both values of original
    (" 2019-05-12 " * 3 + " 2018-05-12 " * 2, False),
    (" 2019-05-12 " * 3 + " 2018-05-12 " * 2, True),
    (" 2019-05-12 " * 3 + " 2018-05-12 ", False),
    (" 2019-05-12 " * 2 + " 2018-05-12 " * 2, False),
    (" 2019-05-12 " * 2 + " 2018-05-12 " * 2, True),
]

SEARCH_PAGE_MAX_2045_INPUTS = [
    (" 12.05.2040 12.05.2041 12.05.2040 ", False),
    (" 12.05.2038 ", False),
]

EXAMINE_TEXT_INPUTS = [
    "2019-5",
    "2019-05",
    "12.05.19x",
    "Published 12.05.2019 by admin",
    "  12.05.2019  ",
    "a" * 50 + " 2019-05-12",
    "2019-05-12 " + "x" * 60,
    "２０２０-01-15 abc",
]

W3_RUN_PAGE = "w3.org/2020/ " * 20000 + " 2021 "


def search_cases() -> None:
    from parity import html5tree
    from parity.generate import html5_parsers

    oracle_setup()
    from htmldate.core import examine_text, search_page
    from htmldate.utils import Extractor

    def options(max_date: datetime = VALIDATOR_LATEST, original: bool = False) -> Extractor:
        return Extractor(
            extensive=False, max=max_date, min=VALIDATOR_EARLIEST, original=original, format=VALIDATOR_FORMAT
        )

    needs_copy_rule = []

    def checked(html: str, max_date: datetime, original: bool) -> str | None:
        html5tree.parse_document(html)
        if html5tree.last_parse_normalized():
            needs_copy_rule.append(html)
        return search_page(html, options(max_date, original))

    with html5_parsers():
        page_lines = []
        for html, original in SEARCH_PAGE_INPUTS:
            result = checked(html, VALIDATOR_LATEST, original)
            page_lines.append(f"    ({rust_str(html)}, {str(original).lower()}, {rust_option_str(result)}),")
        late_lines = []
        for html, original in SEARCH_PAGE_MAX_2045_INPUTS:
            result = checked(html, datetime(2045, 12, 31), original)
            late_lines.append(f"    ({rust_str(html)}, {str(original).lower()}, {rust_option_str(result)}),")
        examine_lines = []
        for text in EXAMINE_TEXT_INPUTS:
            result = examine_text(text, options())
            examine_lines.append(f"    ({rust_str(text)}, {rust_option_str(result)}),")
        w3_result = checked(W3_RUN_PAGE, VALIDATOR_LATEST, False)
    assert not needs_copy_rule, f"snippets need the copy rule: {[h[:60] for h in needs_copy_rule]!r}"
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import search_cases; search_cases()' > src/htmldate/search_cases.rs",
        "// htmldate 1.11.0's page-text search on fixed pages, in UTC with the htmldate calls of D-13 turned off.",
        "",
        "/// A page, `original`, then `search_page(page, options)` with min_date 1995-01-01 and max_date 2024-01-31.",
        "#[rustfmt::skip]",
        "pub(super) const SEARCH_PAGE_CASES: &[(&str, bool, Option<&str>)] = &[",
        *page_lines,
        "];",
        "",
        "/// A page, `original`, then `search_page(page, options)` with max_date 2045-12-31.",
        "#[rustfmt::skip]",
        "pub(super) const SEARCH_PAGE_MAX_2045_CASES: &[(&str, bool, Option<&str>)] = &[",
        *late_lines,
        "];",
        "",
        "/// A text segment, then `examine_text(text, options)` with min_date 1995-01-01 and max_date 2024-01-31.",
        "#[rustfmt::skip]",
        "pub(super) const EXAMINE_TEXT_CASES: &[(&str, Option<&str>)] = &[",
        *examine_lines,
        "];",
        "",
        '/// `search_page` of `"w3.org/2020/ "` repeated 20,000 times, then `" 2021 "`, with `original` false.',
        f"pub(super) const W3_RUN_CASE: Option<&str> = {rust_option_str(w3_result)};",
        "",
    ]
    sys.stdout.write("\n".join(out))


# htmldate's tree examiners (src/htmldate/examine_cases.rs): each page is one input with one examiner. The header,
# JSON, abbr and time examiners run with both values of `original`, `elements` with both values of `extensive`.
# The pages use min_date 1995-01-01 and max_date 2024-01-31.
EXAMINE_INPUTS = [
    # examine_header: name, property, itemprop, pubdate and http-equiv metas, the copyright year, og:url
    ('<meta name="date" content="2020-01-15">', "header"),
    ('<meta name="Date" content="2020-01-15T10:20:30+01:00">', "header"),
    ('<meta name="lastmod" content="2020-02-01"><meta name="date" content="2019-12-24">', "header"),
    ('<meta name="last-modified" content="2020-02-01">', "header"),
    ('<meta name="modified" content="garbage"><meta name="utime" content="2020-02-02">', "header"),
    ('<meta name="date" content="2020-01-15"><meta name="date" content="2021-06-01">', "header"),
    ('<meta property="article:published_time" content="2020-01-15T10:00:00+00:00">', "header"),
    ('<meta property="og:modified_time" content="2020-02-01"><meta property="article:modified_time" content="2020-03-01">', "header"),
    ('<meta property="og:updated_time" content="2020-02-01">', "header"),
    ('<meta itemprop="datePublished" content="2020-01-15">', "header"),
    ('<meta itemprop="datePublished" datetime="2020-01-15" content="2019-01-01">', "header"),
    ('<meta itemprop="datePublished" datetime="" content="2019-03-03">', "header"),
    ('<meta itemprop="datePublished" datetime="2020-01-15">', "header"),
    ('<meta itemprop="dateModified" content="2020-02-01">', "header"),
    ('<meta itemprop="copyrightYear" content="2015">', "header"),
    ('<meta itemprop="copyrightYear" content="15">', "header"),
    ('<meta itemprop="copyrightYear" content="2015"><meta name="date" content="2020-01-15">', "header"),
    ('<meta pubdate="pubdate" content="2020-01-15">', "header"),
    ('<meta pubdate="PubDate" content="2020-01-15">', "header"),
    ('<meta pubdate="other" content="2020-01-15">', "header"),
    ('<meta http-equiv="date" content="2020-01-15">', "header"),
    ('<meta http-equiv="last-modified" content="2020-02-01"><meta http-equiv="date" content="2019-12-24">', "header"),
    ('<meta name="date">', "header"),
    ('<meta name="date" datetime="2020-01-15">', "header"),
    ('<meta name="og:url" content="https://example.com/2019/05/12/post"><meta name="date" content="garbage">', "header"),
    ('<meta name="og:url" content="https://example.com/2019/05/12/post">', "header"),
    # examine_header with the same keys on each side of the first date: the first with a date wins
    ('<meta name="date" content="garbage"><meta name="date" content="2020-01-15">', "header"),
    # examine_json: the JSON-LD and settings scripts, mixed case, wrong type, out of range, no date key
    ('<script type="application/ld+json">{"@type": "Article", "datePublished": "2020-01-15T10:00:00+01:00", "dateModified": "2020-02-01"}</script>', "json"),
    ('<script type="application/settings+json">{"dateModified": "2020-03-01"}</script>', "json"),
    ('<script type="application/ld+json">{"DATEPUBLISHED": "2019-01-01"}</script>', "json"),
    ('<script type="text/javascript">{"datePublished": "2020-01-15"}</script>', "json"),
    ('<script type="application/ld+json">{"datePublished": "2020-01-15"}</script><script type="application/ld+json">{"datePublished": "2021-01-01"}</script>', "json"),
    ('<script type="application/ld+json">{"datePublished": "2045-12-31"}</script>', "json"),
    ('<script type="application/ld+json">{"name": "x"}</script>', "json"),
    ('<script type="application/ld+json"></script>', "json"),
    ('<script type="application/ld+json">{"datePublished": "2017-9-1"}</script>', "json"),
    # examine_abbr_elements: data-utime (valid, negative, huge, underscores, a float), class published with a title or text
    ('<abbr data-utime="1579082400">x</abbr>', "abbr"),
    ('<abbr data-utime="-86400">x</abbr>', "abbr"),
    ('<abbr data-utime="99999999999999">x</abbr>', "abbr"),
    ('<abbr data-utime=" 1_500_000_000 ">x</abbr>', "abbr"),
    ('<abbr data-utime="1.5">x</abbr>', "abbr"),
    ('<abbr data-utime="garbage" class="published" title="2020-01-15">x</abbr>', "abbr"),
    ('<abbr class="published" title="2020-01-15">x</abbr>', "abbr"),
    ('<abbr class="published" title="15.01.2020">x</abbr><abbr class="published" title="2021-03-03">y</abbr>', "abbr"),
    ('<abbr class="published">Published 15 January 2020 by admin</abbr>', "abbr"),
    ('<abbr class="published">15.01.2020</abbr>', "abbr"),
    ('<abbr class="published">Published 15.01.2020</abbr>', "abbr"),
    ('<abbr data-utime="1579082400">x</abbr><abbr class="published" title="2021-03-03">y</abbr>', "abbr"),
    ('<abbr>Posted 15 January 2020 here</abbr>', "abbr"),
    ('<abbr class="published" title="2020-01-15">x</abbr>' * 1001, "abbr"),
    # examine_time_elements: datetime attributes, the shortcut flags, bare text, the window bound, equal wall clocks
    ('<time pubdate="pubdate" datetime="2020-01-15T10:00:00+01:00">15 Jan</time>', "time"),
    ('<time class="entry-date" datetime="2020-01-15">x</time>', "time"),
    ('<time class="entry-time-x" datetime="2020-01-15">x</time>', "time"),
    ('<time class="updated" datetime="2020-02-01">x</time>', "time"),
    ('<time datetime="2020-01-15T10:00:00">x</time><time datetime="2021-06-01">y</time>', "time"),
    ('<time datetime="2020-01-15">x</time><time datetime="2019-06-01">y</time>', "time"),
    ('<time datetime="2024-01-31T23:30:00-05:00">x</time>', "time"),
    ('<time datetime="2024-01-30T23:30:00+05:00">a</time><time datetime="2024-01-30T23:30:00-05:00">b</time>', "time"),
    ('<time datetime="2024-01-30T23:30:00-05:00">a</time><time datetime="2024-01-30T23:30:00+05:00">b</time>', "time"),
    ('<time>2020-01-15 10:00</time>', "time"),
    ('<time>2020-01</time>', "time"),
    ('<time datetime="2020">x</time>', "time"),
    ('<time datetime="2020-01-15">x</time>' * 1000, "time"),
    ('<time datetime="2020-01-15">x</time>' * 1001, "time"),
    # examine_elements over date_candidates: the cues of id and class, the first of id and class, itemprop, folding
    ('<div id="publish_date">15.01.2020</div>', "elements"),
    ('<div class="byline">Jan 15, 2020</div>', "elements"),
    ('<div id="x" class="date">2020-01-15</div>', "elements"),
    ('<div class="date" id="x">2020-01-15</div>', "elements"),
    ('<div class="Datum">15.01.2020</div>', "elements"),
    ('<div class="DATE">15.01.2020</div>', "elements"),
    ('<span itemprop="datePublished">2020-01-15</span>', "elements"),
    ('<span itemprop="dateModified">2020-01-15</span>', "elements"),
    ('<div class="x" id="footer-info-lastmod">2020-01-15</div>', "elements"),
    ('<div id="main" class="byline">2020-01-15</div>', "elements"),
    ('<div id="entry-Meta">2020-01-15</div>', "elements"),
    ('<div id="metaX">2020-01-15</div>', "elements"),
    ('<small>15.01.2020</small>', "elements"),
    ('<footer>15.01.2020</footer>', "elements"),
    ('<p class="byline">2020-01-15</p>', "elements"),
    ('<svg><g id="publish">2020-01-15</g></svg>', "elements"),
    ('<svg><footer>15.01.2020</footer></svg>', "elements"),
    ('<div id="publish">2020-01-15</div>' * 1001, "elements"),
    # examine_elements over the title and h1 elements, in document order
    ('<html><head><title>Post 15.01.2020</title></head><body><h1 title="2020-01-15">x</h1></body></html>', "title_h1"),
    ('<html><head><title>x</title></head><body><h1>Post 15.01.2020</h1><h1>other 2019-05-12</h1></body></html>', "title_h1"),
    ('<svg><title>2020-01-15</title></svg>', "title_h1"),
    ('<h1 title="garbage">Post</h1><title title="2020-01-15">x</title>', "title_h1"),
    # FREE_TEXT_EXPRESSIONS: text and tails of the fast tags in document order, not of the others
    ('<div>Text one<p>para</p>tail after p<span>x</span>tail2</div>', "free_text"),
    ('<div>a<ul><li>b<em>c</em>d</li></ul>e</div>', "free_text"),
    ('<b>not fast</b>tail<p>q</p>end', "free_text"),
    ('<div>x</div><span>y</span>', "free_text"),
    # img_search: the first og:image with a content attribute
    ('<meta property="og:image" content="https://example.com/2020/01/15/a.jpg">', "img"),
    ('<meta property="og:image"><meta property="og:image" content="https://example.com/2020/01/15/a.jpg">', "img"),
    ('<meta property="og:image" content="https://example.com/x.jpg"><meta property="og:image" content="https://example.com/2020/01/15/a.jpg">', "img"),
    ('<meta property="og:image" content="https://example.com/2020/01/15/a.jpg?x=1">', "img"),
]


def examine_variants(function: str) -> list[tuple[bool, bool]]:
    "The (original, extensive) pairs a function is run with."
    if function == "elements":
        return [(False, extensive) for extensive in (False, True)]
    if function in ("header", "json", "abbr", "time"):
        return [(original, False) for original in (False, True)]
    return [(False, False)]


def run_examine(tree, function: str, original: bool, extensive: bool):
    from htmldate.core import date_candidates, examine_abbr_elements, examine_elements, examine_header, examine_time_elements
    from htmldate.extractors import FREE_TEXT_EXPRESSIONS, img_search, json_search
    from htmldate.utils import Extractor

    options = Extractor(
        extensive=extensive, max=VALIDATOR_LATEST, min=VALIDATOR_EARLIEST, original=original, format=VALIDATOR_FORMAT
    )
    if function == "header":
        return examine_header(tree, options)
    if function == "json":
        return json_search(tree, options)
    if function == "abbr":
        return examine_abbr_elements(tree, options)
    if function == "time":
        return examine_time_elements(tree, options)
    if function == "elements":
        return examine_elements(date_candidates(tree, extensive), options)
    if function == "title_h1":
        return examine_elements(tree.xpath(".//title|.//h1"), options)
    if function == "img":
        return img_search(tree, options)
    if function == "free_text":
        return [str(segment) for segment in FREE_TEXT_EXPRESSIONS(tree)]
    raise ValueError(function)


def examine_result(result) -> str:
    "An `Expected` literal: a date or None, or the text segments of free_text."
    if isinstance(result, list):
        return "Expected::Texts(&[" + ", ".join(rust_str(segment) for segment in result) + "])"
    return f"Expected::Date({rust_option_str(result)})"


def examine_cases() -> None:
    oracle_setup()
    from parity import html5tree
    from parity.generate import html5_parsers

    lines = []
    with html5_parsers():
        for html, function in EXAMINE_INPUTS:
            for original, extensive in examine_variants(function):
                tree = html5tree.parse_document(html)
                assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {html[:60]!r}"
                result = run_examine(tree, function, original, extensive)
                lines.append(
                    f"    ExamineCase {{ html: {rust_str(html)}, function: {rust_str(function)}, "
                    f"original: {str(original).lower()}, extensive: {str(extensive).lower()}, "
                    f"result: {examine_result(result)} }},"
                )
    print(f"examine cases: {len(lines)}", file=sys.stderr)
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import examine_cases; examine_cases()' > src/htmldate/examine_cases.rs",
        "// htmldate 1.11.0's tree examiners on fixed pages, in UTC with D-13 off, min_date 1995-01-01 and max_date 2024-01-31.",
        "",
        "/// What an examiner returns: a date or `None`, or the text segments of `free_text`.",
        "#[derive(Debug, PartialEq, Eq)]",
        "pub(super) enum Expected {",
        "    Date(Option<&'static str>),",
        "    Texts(&'static [&'static str]),",
        "}",
        "",
        "/// A page, the examiner `function`, `original`, `extensive`, then the result.",
        "pub(super) struct ExamineCase {",
        "    pub html: &'static str,",
        "    pub function: &'static str,",
        "    pub original: bool,",
        "    pub extensive: bool,",
        "    pub result: Expected,",
        "}",
        "",
        "#[rustfmt::skip]",
        "pub(super) const EXAMINE_CASES: &[ExamineCase] = &[",
        *lines,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


# The pages `find_date_cases` runs, each with its URL argument (`None`: the canonical link is probed) and in
# the four (`original`, `extensive`) combinations. The comment on each page names the return it ends at.
FIND_DATE_PAGES = [
    # the URL argument: its date is the first return
    ("<html><body><p>Hello</p></body></html>", "https://example.org/2017/09/01/content.html"),
    # the URL argument has no date, so the header date is the return
    ('<html><head><meta name="date" content="2019-04-02"></head><body></body></html>', "https://example.org/news/"),
    # the URL argument is too late for max_date 2024-01-31: the rest of the page decides
    ("<html><body><p>Hello</p></body></html>", "https://example.org/2030/01/01/x"),
    # the canonical link probe: its href holds the date
    ('<html><head><link rel="canonical" href="https://example.org/2018/03/05/a.html"></head><body></body></html>', None),
    # the canonical link without href gives no URL
    ('<html><head><link rel="canonical"></head><body><p>Hello</p></body></html>', None),
    # examine_header: a date by name
    ('<html><head><meta name="date" content="2019-04-02"></head><body></body></html>', None),
    # examine_header: og:published_time (the first of test_dates' pages)
    ('<html><head><meta property="og:published_time" content="2017-09-01"/></head><body></body></html>', None),
    # examine_header: og:url, whose date is a reserve: with extensive search off the result is None
    ('<html><head><meta property="og:url" content="https://example.org/2017/09/01/content.html"/></head><body></body></html>', None),
    # json_search: datePublished when original, dateModified otherwise
    ('<html><head><script type="application/ld+json">{"datePublished": "2016-05-06T10:00:00Z", "dateModified": "2016-07-08T10:00:00Z"}</script></head><body></body></html>', None),
    # examine_abbr_elements: data-utime 1500000000 is 2017-07-14 UTC
    ('<html><body><abbr data-utime="1500000000">a day</abbr></body></html>', None),
    # examine_elements over the date candidates: a class cue
    ('<html><body><div class="entry-date">March 3, 2015</div></body></html>', None),
    # examine_elements over title and h1, when no candidate has a date
    ("<html><head><title>Notes from 12 June 2013</title></head><body><p>Hello</p></body></html>", None),
    # examine_time_elements: a time element with its datetime
    ('<html><body><time datetime="2012-08-09T12:00:00">a time</time></body></html>', None),
    # the time element at the bound: the aware value is later than midnight of max_date
    ('<html><body><time datetime="2024-01-31T23:30:00-05:00">a time</time></body></html>', None),
    # pattern_search with TIMESTAMP_PATTERN over the page string: a timestamp in an attribute
    ('<html><body><div data-stamp="2021-06-07T08:00:00+00:00"><p>Hello</p></div></body></html>', None),
    # img_search: the date in the og:image URL
    ('<html><head><meta property="og:image" content="https://example.org/2011/07/08/pic.jpg"></head><body></body></html>', None),
    # idiosyncrasies_search: a date with a text cue
    ("<html><body><p>Hello</p><p>posted 01/09/17</p></body></html>", None),
    # fast mode: a loose date in text, which the strict timestamp misses
    ("<html><body><p>Updated 2020-02-02</p></body></html>", None),
    # the date only in a script: fast mode drops the script, extensive search reads the page string
    ('<html><body><p>Hello</p><script>var d = "2020-03-03";</script></body></html>', None),
    # extensive: the candidate element of test_dates' third page
    ("<html><body><p>Veröffentlicht am 1.9.17</p></body></html>", None),
    # free text: the element text is cut at 52 code points before the date, but the tail of the `b` is a short segment
    ("<html><body><div>Lorem ipsum dolor sit amet, consectetur adipiscing elit sed do <b>x</b> 12 May 2019</div></body></html>", None),
    # free text discriminates: the segments are read in order, and the last one in the window wins over search_page's first
    ("<html><body><p>Posted 3 March 2018</p><p>Posted 4 April 2017</p></body></html>", None),
    # free text discriminates in the other direction: a segment with a cue after another with a date in the page string
    ("<html><body><p>1 April 2017</p><div>updated 3 March 2018 here</div></body></html>", None),
    # free text: a date with a text cue, which the free-text stage returns
    ("<html><body><div>This article was written on 12 May 2019 by staff</div></body></html>", None),
    # lxml's live iteration in clean_html: the video after an object with audio is kept and its date is found
    ("<html><body><object><audio></audio></object><video><p>2019-04-05 10:00:00</p></video></body></html>", None),
    # the plain name `svg` never matches a namespaced svg in clean_html, so the video after it is dropped
    ("<html><body><object><svg></svg></object><video><p>2019-04-05 10:00:00</p></video></body></html>", None),
    # a surviving svg in serialize, with a year-only footer
    ("<html><body><svg><path d=\"M0 0\"/></svg><footer>© 2018</footer></body></html>", None),
    # the tail of a child of a svg style is raw, so `&copy;` reads as a copyright cue and 2021 wins over the link date
    ('<html><body><a href="/2019/05/12/post">post</a><svg><style><g></g>&amp;copy; 2021 </style></svg></body></html>', None),
    # non-ASCII digits in a candidate: the digit gates decide
    ('<html><body><div class="date">２０２０-01-15</div></body></html>', None),
    # no date at all
    ("<html><head></head><body><p>Nothing here.</p></body></html>", None),
]
FIND_DATE_MAX = "2024-01-31"


def find_date_cases() -> None:
    from parity import html5tree
    from parity.generate import html5_parsers

    oracle_setup()
    from htmldate.core import find_date

    lines = []
    with html5_parsers():
        for html, url in FIND_DATE_PAGES:
            for original, extensive in itertools.product((False, True), repeat=2):
                tree = html5tree.parse_document(html)
                assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {html[:60]!r}"
                result = find_date(
                    tree, original_date=original, extensive_search=extensive, max_date=FIND_DATE_MAX, url=url
                )
                lines.append(
                    f"    ({rust_str(html)}, {rust_option_str(url)}, {str(original).lower()}, "
                    f"{str(extensive).lower()}, {rust_option_str(result)}),"
                )
    print(f"find_date cases: {len(lines)}", file=sys.stderr)
    out = [
        f"// @generated by tools/parity/parity/gen_htmldate_cases.py with Python {version()}.",
        "// Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_htmldate_cases import find_date_cases; find_date_cases()' > src/htmldate/find_date_cases.rs",
        "// htmldate 1.11.0's find_date on fixed pages, in UTC with D-13 off, min_date 1995-01-01 and max_date 2024-01-31.",
        "",
        "/// A page, the URL argument, `original`, `extensive`, then the date `find_date` returns.",
        "pub(super) type FindDateCase = (",
        "    &'static str,",
        "    Option<&'static str>,",
        "    bool,",
        "    bool,",
        "    Option<&'static str>,",
        ");",
        "#[rustfmt::skip]",
        "pub(super) const FIND_DATE_CASES: &[FindDateCase] = &[",
        *lines,
        "];",
        "",
    ]
    sys.stdout.write("\n".join(out))


if __name__ == "__main__":
    datetime_cases()
