"""Generates tests/output_format_cases/cases.rs: upstream's XML, XML-TEI, JSON and CSV output.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_output_format_cases import main; main()' > tests/output_format_cases/cases.rs

`BlankCase`: an `etree.fromstring` snippet and the output of upstream's `control_xml_output` parse (`xml.py`,
`CONTROL_PARSER`) after the text round trip of `xml.py:255`. The inner `tostring` writes text, not character
references, as upstream does: non-ASCII text reaches libxml2 as text. The snippet follows `check_blank_snippet`
(the `check_snippet` rules of `gen_xml_output_cases`, but `&#13;` is allowed), and also has no comment and no
processing instruction, which `tests/trees` does not keep.

`DocCase`: a page through `extract` with `fast=True` (D-1), as XML, XML-TEI, JSON without metadata, JSON with
metadata and CSV with metadata. The page is parsed with html5lib (D-2), and the copy rule must not fire.

`BUFFER_SPLIT_KEPT`: a probe of libxml2's parse buffer. A run of blanks between two elements straddles a
character offset near 4,000, counted in characters and in bytes at the width of the variant's widest character.
The probe reports every offset whose run survives the round trip (stderr) and is true when there is one.
"""

import re
import sys
from dataclasses import dataclass

from parity import MAX_DATE
from parity.gen_main_extractor_cases import para, prose
from parity.gen_xpath_cases import rust_str
from parity.generate import pinned_version


@dataclass
class BlankCase:
    name: str
    xml: str
    expected: str


@dataclass
class DocCase:
    name: str
    html: str
    url: str | None
    record_id: str | None
    xml: str
    xmltei: str
    json: str
    json_meta: str
    csv: str


@dataclass
class Page:
    name: str
    html: str
    url: str | None = None
    record_id: str | None = None


def nested_snippet(depth: int) -> str:
    "`depth` elements, each with a blank before its child, around one leaf."
    opening = "".join(f"<n{i}>\n  " for i in range(depth))
    closing = "".join(f"\n</n{i}>" for i in reversed(range(depth)))
    return f"{opening}<leaf/>{closing}"


# (name, snippet): one or more per rule of the blank removal decisions in the phase 6 spec
BLANK_SNIPPETS: list[tuple[str, str]] = [
    ("element_only_blanks_dropped", "<a>\n  <b/>\n  <c/>\n</a>"),
    ("blank_before_end_tag_without_child_kept", "<a>   </a>"),
    ("first_child_text_keeps_later_blanks", "<a>x<b/>  <c/></a>"),
    ("tab_counts_as_blank", "<a><b/>\t<c/></a>"),
    ("nbsp_is_not_blank", "<a><b/> <c/></a>"),
    ("sticky_after_delivered_chunk", "<p><hi>a</hi>text<hi>b</hi> <hi>c</hi></p>"),
    ("child_of_sticky_parent_keeps_blank_state", "<p>t<x>\n  <y/>\n</x></p>"),
    ("entity_splits_blank_run", "<a><b/>&amp;<c/>  <d/></a>"),
    ("non_ascii_text_then_blanks", "<a>中文<b/>  <c/></a>"),
    ("e_acute_text_then_blanks", "<a>café<b/>\n  <c/></a>"),
    ("long_blank_run_with_tab_before_tag", "<a><b/>" + " \t" * 200 + "<c/></a>"),
    ("nesting_past_indent_cap", nested_snippet(32)),
    # a blank chunk followed by a reference or a non-ASCII byte is kept, whatever the element holds
    ("blank_before_reference_kept", "<a><b/>  &amp;<c/></a>"),
    ("blank_before_non_ascii_kept", "<a><b/> é<c/></a>"),
    # the first child is text (a reference, which does not set the sticky state)
    ("first_child_reference_keeps_later_blank", "<a>&amp;<b/>  <c/></a>"),
    # the last child is text (a reference) and the element is not sticky
    ("last_child_reference_keeps_blank", "<a><b/>&amp;  </a>"),
    # `<`, `>` and CR references are never dropped and do not set the sticky state
    ("less_than_reference_not_sticky", "<a><b/>&lt;<c/>  <d/></a>"),
    ("greater_than_reference_not_sticky", "<a><b/>&gt;<c/>  <d/></a>"),
    ("cr_reference_not_sticky", "<a><b/>&#13;<c/>  <d/></a>"),
]


def check_blank_snippet(source: str) -> None:
    "`check_snippet` of gen_xml_output_cases without its `&#` rule: CR needs `&#13;`, which tests/trees decodes."
    from lxml import etree

    assert "\r" not in source and "<!--" not in source and "<?" not in source, repr(source[:200])
    for tag in re.findall(r"<[A-Za-z][^<>]*>", source):
        for double, single in re.findall(r"""=\s*(?:"([^"]*)"|'([^']*)')""", tag):
            assert not re.search(r"[\t\n\r]", double + single), repr(tag)
    root = etree.fromstring(source)
    assert not any(el.tag in ("svg", "math") for el in root.iter()), repr(source[:200])


def control_parse(xml: str, pretty_print: bool) -> str:
    "Upstream's control parse of `xml` (`xml.py:255`), written back."
    from lxml import etree
    from trafilatura.xml import CONTROL_PARSER

    text = etree.tostring(etree.fromstring(xml), encoding="unicode")
    tree = etree.fromstring(text, CONTROL_PARSER)
    return etree.tostring(tree, pretty_print=pretty_print, encoding="unicode").strip()


def blank_cases() -> list[BlankCase]:
    names = [name for name, _ in BLANK_SNIPPETS]
    assert len(set(names)) == len(names), names
    cases = []
    for name, xml in BLANK_SNIPPETS:
        check_blank_snippet(xml)
        cases.append(BlankCase(name, xml, control_parse(xml, True)))
    return cases


# ---------------------------------------------------------------- the buffer probe

PROBE_VARIANTS = (("latin1", "é", 1), ("bmp", "中", 2), ("astral", "\U0001F600", 4))
PROBE_OFFSETS = range(3900, 4101)
PROBE_RUN = 200
PROBE_HEAD = "<root><t>"
PROBE_SPLIT = "</t><e1/>"
PROBE_KEPT_MARK = "<e1/><e2/>"
# The pinned upstream version appears in the xmltei header; the pin lives only in Cargo.toml, so it is a token
UPSTREAM_VERSION_TOKEN = "@@UPSTREAM_VERSION@@"


def probe_document(char: str, offset: int) -> str:
    "A run of blanks between `<e1/>` and `<e2/>` covering character `offset` of the document."
    pad = offset - PROBE_RUN // 2 - len(PROBE_HEAD) - len(char) - len(PROBE_SPLIT)
    return f"{PROBE_HEAD}{char}{'x' * pad}{PROBE_SPLIT}{' ' * PROBE_RUN}<e2/></root>"


def probe_kept(char: str, width: int) -> list[str]:
    "A report line for each offset whose run survives the control parse."
    lines = []
    for offset in sorted({*PROBE_OFFSETS, *(o // width for o in PROBE_OFFSETS)}):
        text = probe_document(char, offset)
        if PROBE_KEPT_MARK not in control_parse(text, False):
            window = text[offset - 8 : offset + 8]
            utf8_offset = len(text[:offset].encode())
            lines.append(f"char={offset} utf8_offset={utf8_offset} window={window!r} "
                         f"window_utf8={window.encode().hex(' ')}")
    return lines


def buffer_probe() -> list[str]:
    "Report lines of the offsets whose run survives the buffer split; the control run must be dropped."
    reports = []
    for name, char, width in PROBE_VARIANTS:
        assert PROBE_KEPT_MARK in control_parse(probe_document(char, 300), False), f"control kept: {name}"
        reports += [f"variant={name} width={width} {line}" for line in probe_kept(char, width)]
    return reports


# ---------------------------------------------------------------- the pages


def document(body: str, head: str = "<title>Test page</title>") -> str:
    return f"<html><head>{head}</head><body>{body}</body></html>"


def doc_pages() -> list[Page]:
    metadata = (
        '<meta property="og:site_name" content="Example Site">'
        '<meta property="article:published_time" content="2025-03-04T10:00:00+00:00">'
        '<meta name="author" content="Jane Doe">'
    )
    table = (
        f"<article><p>{prose(11, 300)}</p>"
        "<table><tr><th>Name</th><th>Value</th></tr>"
        "<tr><td>alpha</td><td></td></tr>"
        "<tr><td>beta</td><td> </td></tr></table>"
        f"<p>{prose(12, 300)}</p></article>"
    )
    lists = (
        f"<article><p>{prose(21, 200)}</p>"
        '<ul><li>first <hi rend="#b">bold</hi> item</li><li>second item</li></ul>'
        '<p>See <ref target="https://example.org/docs">the docs</ref> and <code>run()</code>   '
        '<hi rend="#i">then</hi>\n  <graphic src="https://example.org/fig.png"/>   after the figure.</p>'
        f"<p>{prose(22, 200)}</p>"
        "<pre><code>x = 1</code>  \n  <code>y = 2</code></pre></article>"
    )
    special = (
        '<article><p>He said "hello" and left \\ the room.\tA tab, a line break, café, '
        "中文, an emoji \U0001F600. "
        f"{prose(41, 250)}</p><p>{prose(42, 250)}</p></article>"
    )
    csv_quoting = (
        "<article><p>Alpha, beta, \"gamma\" and delta.</p>"
        f"<p>{prose(51, 260)}</p>"
        '<p>Line one,\nline "two" ends.</p>'
        f"<p>{prose(52, 260)}</p></article>"
    )
    comments = (
        f"<article><p>{prose(61, 300)}</p></article>"
        '<div id="comments"><h3>Comments</h3><ol class="commentlist">'
        f'<li class="comment"><p>{prose(62, 120)}</p></li>'
        f'<li class="comment"><p>{prose(63, 120)}</p></li></ol></div>'
    )
    return [
        Page("table_with_padding_cell", document(table, metadata), url="https://example.org/tables/1"),
        Page("lists_inline_and_blank_tails", document(lists), url="https://example.org/lists"),
        # xml leaves the title None (metadata off), so its fingerprint input starts with "None"; xmltei sets
        # with_metadata itself (settings.py:161), so its title is read.
        Page("no_sitename_no_date", document(f"<article><p>{prose(31, 300)}</p><p>{prose(32, 300)}</p></article>",
                                             "<title>Untitled</title>")),
        Page("special_characters", document(special)),
        Page("several_paragraphs_csv_quoting", document(csv_quoting)),
        Page("comments", document(comments)),
        Page("record_id", document(f"<article>{para(71, 300)}</article>"), record_id="doc-1"),
    ]


def doc_case(page: Page, extensive: bool) -> DocCase:
    from trafilatura import extract

    def run(output_format: str, with_metadata: bool) -> str:
        text = extract(
            page.html, url=page.url, record_id=page.record_id, fast=True, include_comments=True,
            output_format=output_format, with_metadata=with_metadata,
            date_extraction_params={"original_date": True, "extensive_search": extensive, "max_date": MAX_DATE},
        )
        assert isinstance(text, str), (page.name, output_format)
        assert UPSTREAM_VERSION_TOKEN not in text, (page.name, output_format)
        return text

    version = pinned_version()
    return DocCase(
        page.name, page.html, page.url, page.record_id,
        xml=run("xml", False), xmltei=run("xmltei", False).replace(version, UPSTREAM_VERSION_TOKEN),
        json=run("json", False), json_meta=run("json", True), csv=run("csv", True),
    )


def doc_cases() -> list[DocCase]:
    from trafilatura.settings import DEFAULT_CONFIG

    from parity.generate import html5_parsers

    extensive = DEFAULT_CONFIG.getboolean("DEFAULT", "EXTENSIVE_DATE_SEARCH")
    cases = []
    for page in doc_pages():
        normalized: list[bool] = []
        with html5_parsers(normalized):
            cases.append(doc_case(page, extensive))
        assert not any(normalized), f"page {page.name} needs the copy rule"
    names = [case.name for case in cases]
    assert len(set(names)) == len(names), names
    assert any(UPSTREAM_VERSION_TOKEN in case.xmltei for case in cases), "the pinned version is not in any xmltei value"
    return cases


# ---------------------------------------------------------------- output


def rust_opt(value: str | None) -> str:
    return "None" if value is None else f"Some({rust_str(value)})"


HEADER = """\
// @generated by tools/parity/parity/gen_output_format_cases.py. Do not edit. Regenerate from the repository root with:
//   scripts/oracle.sh 'from parity.gen_output_format_cases import main; main()' > tests/output_format_cases/cases.rs
// Upstream's XML, XML-TEI, JSON and CSV output: blank removal on etree.fromstring snippets, and extract on pages
// with fast=True.

/// A snippet through upstream's control parse (`CONTROL_PARSER`) after the text round trip, pretty printed.
pub(crate) struct BlankCase {
    pub(crate) name: &'static str,
    pub(crate) xml: &'static str,
    pub(crate) expected: &'static str,
}

/// A page through `extract` with `fast=True`: XML, XML-TEI, JSON without and with metadata, CSV with metadata.
pub(crate) struct DocCase {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) url: Option<&'static str>,
    pub(crate) record_id: Option<&'static str>,
    pub(crate) xml: &'static str,
    pub(crate) xmltei: &'static str,
    pub(crate) json: &'static str,
    pub(crate) json_meta: &'static str,
    pub(crate) csv: &'static str,
}
"""


def render(blanks: list[BlankCase], docs: list[DocCase], kept: bool) -> str:
    out = [HEADER]
    out.append("/// The `max_date` the pages are extracted with (`Call::max_date`); XML-TEI writes `filedate` from it.")
    out.append(f'pub(crate) const MAX_DATE: &str = {rust_str(MAX_DATE)};')
    out.append("/// Whether the sliding-offset probe kept a blank run (the buffer split case); see the generator.")
    out.append(f"pub(crate) const BUFFER_SPLIT_KEPT: bool = {'true' if kept else 'false'};")
    out.append("/// The upstream version in `xmltei` values; the test substitutes `PKG_VERSION`.")
    out.append(f"pub(crate) const UPSTREAM_VERSION_TOKEN: &str = {rust_str(UPSTREAM_VERSION_TOKEN)};")
    out.append("")
    out.append("#[rustfmt::skip]")
    out.append("pub(crate) const BLANK_CASES: &[BlankCase] = &[")
    for case in blanks:
        out += [
            "    BlankCase {",
            f"        name: {rust_str(case.name)},",
            f"        xml: {rust_str(case.xml)},",
            f"        expected: {rust_str(case.expected)},",
            "    },",
        ]
    out += ["];", "", "#[rustfmt::skip]", "pub(crate) const DOC_CASES: &[DocCase] = &["]
    for case in docs:
        out += [
            "    DocCase {",
            f"        name: {rust_str(case.name)},",
            f"        html: {rust_str(case.html)},",
            f"        url: {rust_opt(case.url)},",
            f"        record_id: {rust_opt(case.record_id)},",
            f"        xml: {rust_str(case.xml)},",
            f"        xmltei: {rust_str(case.xmltei)},",
            f"        json: {rust_str(case.json)},",
            f"        json_meta: {rust_str(case.json_meta)},",
            f"        csv: {rust_str(case.csv)},",
            "    },",
        ]
    out += ["];", ""]
    return "\n".join(out)


def main() -> None:
    blanks = blank_cases()
    reports = buffer_probe()
    for line in reports:
        print(f"buffer split kept: {line}", file=sys.stderr)
    kept = bool(reports)
    docs = doc_cases()
    sys.stdout.write(render(blanks, docs, kept))
    print(f"{len(blanks)} blank cases, {len(docs)} doc cases; BUFFER_SPLIT_KEPT={str(kept).lower()} "
          f"({len(reports)} kept offsets)", file=sys.stderr)
