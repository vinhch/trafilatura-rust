"""Generates src/pydatetime/cases.rs (what CPython 3.13's datetime and strptime return on fixed inputs) and
src/dom/html_cases.rs (what lxml.html.tostring returns on fixed pages, `serialize_cases()`).

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_htmldate_cases import datetime_cases; datetime_cases()' > src/pydatetime/cases.rs

Each table holds the value Python returns, or `None` where it raises ValueError (or OverflowError or OSError
for a timestamp). The crate's `DateTime` holds the wall clock and the UTC offset in microseconds; a naive
value has no offset.

`oracle_setup()` runs first in every case function. It pins the zone to UTC and turns off the two htmldate
calls that are not ported (D-13): `_external_date` finds nothing and `dateutil_parse` raises.
"""

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


if __name__ == "__main__":
    datetime_cases()
