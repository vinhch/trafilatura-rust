"""Generates tests/main_extractor_cases/cases.rs: upstream's `main_extractor` handlers and extractors on snippets.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_main_extractor_cases import main; main()' > tests/main_extractor_cases/cases.rs

Two kinds of case, each with the options it sets (`tables`, `images`, `links`, `formatting`, `focus`,
`comments`, `url`). Every `Extractor` is built with `fast=True`: the crate takes upstream's fast path
(deviation D-1), and the default `fast=False` would prune a teaser block in `recover_wild_text` that the
crate keeps.

`XmlCase`: a handler runs on an `etree.fromstring` snippet of converted tags (`list`, `item`, `cell`, `hi`).
The handler gets the first element named `target` in document order, the root when `target` is empty; a
target below the root gives it a parent, which `handle_formatting` and `is_code_block_element` read. The
case holds the result's `treedump.dump_tree` (none when the handler returns None) and the dump of the whole
snippet after the call, since the handlers rename what they consume to `done`. A snippet holds no `svg` or
`math`: `treedump` tags those by name in a tree without namespaces, the crate's dump by namespace.

`HtmlCase`: an html5 snippet (`parity.html5tree`) goes through `tree_cleaning` and `convert_tags`, then
`op` runs:

    extract_content            on the converted tree
    comments_then_content      `extract_comments`, then `extract_content` on the tree it leaves

The case holds the dump of the parsed snippet (the Rust test checks its parse against it first, so a parser
difference changes the snippet, never the expectation), the dumps and texts of the comments body (for the
second op) and of the content body. Foreign elements (`svg`, `math`) appear in this kind only.

A case whose upstream call raises (`ValueError` from a span of more than 4300 digits or a URL `urljoin`
rejects) fails the generation: those are unit tests.

`report()` lists the executable lines of upstream's `main_extractor.py` that no case reaches.

    scripts/oracle.sh 'from parity.gen_main_extractor_cases import report; report()'
"""

import random
import sys
from dataclasses import dataclass

from parity.gen_xpath_cases import rust_str


@dataclass(frozen=True)
class Options:
    tables: bool = True
    images: bool = False
    links: bool = False
    formatting: bool = False
    focus: str = "balanced"
    comments: bool = True
    url: str | None = None


@dataclass
class XmlCase:
    xml: str
    handler: str
    target: str
    ptags: list[str] | None
    options: Options


@dataclass
class HtmlCase:
    html: str
    op: str
    options: Options


XML_CASES: list[XmlCase] = []
HTML_CASES: list[HtmlCase] = []

# handler -> takes potential_tags
HANDLERS = {
    "handle_table": True,
    "handle_lists": False,
    "handle_quotes": False,
    "handle_paragraphs": True,
    "handle_titles": False,
    "handle_formatting": False,
    "handle_textelem": True,
}
OPS = ("extract_content", "comments_then_content")


def default_tags(options: Options, div: bool = False) -> list[str]:
    "The `potential_tags` `_extract` builds for the options."
    from trafilatura.settings import TAG_CATALOG

    tags = set(TAG_CATALOG)
    if options.tables:
        tags |= {"table", "td", "th", "tr"}
    if options.images:
        tags.add("graphic")
    if options.links:
        tags.add("ref")
    if div:
        tags.add("div")
    return sorted(tags)


def xml(source: str, handler: str, target: str = "", *, ptags: list[str] | None = None, div: bool = False,
        **options) -> None:
    opts = Options(**options)
    if HANDLERS[handler] and ptags is None:
        ptags = default_tags(opts, div)
    assert HANDLERS[handler] or ptags is None, f"{handler} takes no potential_tags"
    XML_CASES.append(XmlCase(source, handler, target, ptags, opts))


def html(source: str, op: str = "extract_content", **options) -> None:
    assert op in OPS, op
    HTML_CASES.append(HtmlCase(source, op, Options(**options)))


def page(body: str) -> str:
    return f"<html><head><title>t</title></head><body>{body}</body></html>"


WORDS = (
    "alpha beta gamma delta epsilon zeta theta iota kappa lambda sigma omega river stone forest window garden "
    "bridge market harbor valley meadow orchard lantern compass anchor pillow blanket kettle ladder mirror "
    "saddle thimble button candle drawer feather hammer jacket mitten needle pencil quilt ribbon shovel "
    "teapot umbrella violin wagon yarn zipper basket cobbler dragon engine fiddle goblet hermit island "
    "jigsaw kennel lagoon magnet nutmeg oyster pepper quartz rocket spider tunnel utensil velvet walnut "
    "cabin dune ember fjord glacier hollow inlet jungle knoll lichen moss nectar oasis prairie ravine"
).split()


def prose(seed: int, length: int) -> str:
    "A sentence of at least `length` characters; two seeds share no run of 50 characters."
    rng = random.Random(seed)
    words: list[str] = []
    while len(" ".join(words)) < length:
        words.append(rng.choice(WORDS))
    sentence = " ".join(words)
    return sentence[0].upper() + sentence[1:] + "."


def para(seed: int, length: int = 300) -> str:
    return f"<p>{prose(seed, length)}</p>"


def links(seed: int, count: int, each: int = 25) -> str:
    return "".join(f'<a href="/l{i}">{prose(seed + i, each)}</a>' for i in range(count))


# ---------------------------------------------------------------- handle_table


def table_cases() -> None:
    table = "handle_table"
    grid = "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>"
    xml(grid, table)
    xml("<table><tr><th>h1</th><th>h2</th></tr><tr><td>a</td><td>b</td></tr></table>", table)
    # only the first row with a th is a header row
    xml("<table><tr><th>h1</th></tr><tr><th>h2</th><td>b</td></tr><tr><td>c</td></tr></table>", table)
    xml("<table><tr><td>a</td><th>h</th></tr><tr><th>h2</th><td>b</td></tr></table>", table)
    xml("<table><thead><tr><th>h</th></tr></thead><tbody><tr><td>a</td></tr></tbody>"
        "<tfoot><tr><td>f</td></tr></tfoot></table>", table)

    # spans: Unicode Nd digits, zero, signs, fractions, the cap of 100
    for attr in ("colspan", "rowspan"):
        for value in ("2", "3", "0", "1", "", "-1", "+2", "2.5", "2 ", " 2", "²", "٣", "١٢", "１", "１２", "०३",
                      "99", "100", "101", "1000", "007"):
            xml(f'<table><tr><td {attr}="{value}">a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>',
                table)
    xml('<table><tr><td colspan="2" rowspan="2">a</td><td>b</td></tr><tr><td>c</td></tr><tr><td>d</td><td>e</td>'
        "<td>f</td></tr></table>", table)
    xml('<table><tr><td rowspan="3">a</td><td>b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr>'
        "<tr><td>e</td><td>f</td></tr></table>", table)
    xml('<table><tr><td>a</td><td rowspan="2">b</td><td>c</td></tr><tr><td>d</td></tr></table>', table)
    xml('<table><tr><td>a</td><td rowspan="2" colspan="2">b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr></table>',
        table)
    xml('<table><tr><td rowspan="5">a</td></tr><tr></tr></table>', table)
    xml('<table><tr><td rowspan="2">a</td></tr><tr><td>b</td><td>c</td></tr></table>', table)
    xml('<table><tr><th colspan="3">head</th></tr><tr><td>a</td><td>b</td><td>c</td></tr></table>', table)
    xml('<table><tr><td colspan="100">a</td></tr><tr><td>b</td></tr></table>', table)
    xml('<table><tr><td colspan="150">a</td></tr><tr><td colspan="2">b</td></tr></table>', table)
    xml('<table><tr><td rowspan="2">a</td></tr><tr><td rowspan="2">b</td></tr><tr><td>c</td></tr></table>', table)

    # empty cells and rows: a row is kept only with content
    xml("<table><tr><td></td><td></td></tr></table>", table)
    xml("<table><tr><td>a</td></tr><tr></tr><tr><td></td></tr><tr><td>b</td></tr></table>", table)
    xml("<table><tr><td></td><td>b</td></tr></table>", table)
    xml("<table><tr><td> </td></tr></table>", table)
    xml("<table></table>", table)
    xml("<table><tr></tr></table>", table)

    # captions, with and without images
    xml("<table><caption>cap</caption><tr><td>a</td><td>b</td></tr></table>", table)
    xml("<table><caption> spaced  cap </caption><tr><td>a</td></tr></table>", table)
    xml("<table><caption>one</caption><caption>two</caption><tr><td>a</td></tr></table>", table)
    xml("<table><caption></caption><tr><td>a</td></tr></table>", table)
    xml("<table><caption>cap</caption></table>", table)
    xml("<table><caption>a <hi>b</hi> c<lb/>d</caption><tr><td>a</td><td>b</td><td>c</td></tr></table>", table)
    xml('<table><caption>cap<graphic src="a.png"/>tail</caption><tr><td>a</td></tr></table>', table, images=True)
    xml('<table><caption><graphic src="a.png" alt="x"/> after</caption><tr><td>a</td></tr></table>', table,
        images=True)
    xml('<table><caption><hi><graphic src="a.png"/></hi></caption><tr><td>a</td></tr></table>', table, images=True)
    xml('<table><caption><graphic src="a.png"/></caption><tr><td>a</td></tr></table>', table, images=False)
    xml("<table><tr><td>a</td></tr><caption>late</caption></table>", table)

    # nested tables: left to the main loop, the tail of a first-level one moves to its cell
    inner = "<table><tr><td>inner</td></tr></table>"
    xml(f"<table><tr><td>outer {inner} after</td><td>b</td></tr></table>", table)
    xml(f"<table><tr><td>{inner}</td></tr></table>", table)
    xml(f"<table><tr><td>outer<hi>bold</hi> {inner}</td></tr></table>", table)
    xml(f"<table><tr><td>o<table><tr><td>m<table><tr><td>deep</td></tr></table> t2</td></tr></table> t1</td></tr></table>",
        table)
    xml(f"<table><tr><td>a</td></tr>{inner}<tr><td>b</td></tr></table>", table)
    xml(f"<table>{inner}</table>", table)
    xml(f"<table><tr><td>a<div>{inner}</div></td></tr></table>", table)
    xml(f"<table><tr><td>a<list><item>{inner}</item></list></td></tr></table>", table, focus="recall")
    xml("<table><tr><td><table><tr><td>x</td></tr></table><table><tr><td>y</td></tr></table></td></tr></table>",
        table)

    # orphan cells join the current row and do not count for the width, stray ones sit in a cell
    xml("<table><td>a</td><tr><td>b</td><td>c</td></tr></table>", table)
    xml("<table><tr><td>a</td></tr><td>b</td><td>c</td></table>", table)
    xml("<table><td>a</td><td>b</td></table>", table)
    xml("<table><tr><td>a<td>stray</td> tail</td></tr></table>", table)
    xml("<table><tr><td><div><td>stray</td></div></td></tr></table>", table)
    xml("<table><tr><td><th>stray head</th></td></tr></table>", table)
    xml("<table><tr><td>a<td>b</td></td><td>c</td></tr></table>", table)
    # unexpected children of the table and of the row
    xml("<table><foo>x</foo><tr><td>a</td></tr></table>", table)
    xml("<table>text<tr><td>a</td></tr></table>", table)
    xml("<table><tr><td>a</td><foo>x</foo></tr></table>", table)
    xml("<table><tr><td>a</td></tr><table><tr><td>direct</td></tr></table></table>", table)
    xml("<table><tr><td>a</td></tr><p>para</p><list><item>i</item></list></table>", table)

    # cell content
    xml("<table><tr><td>a<lb/>b</td></tr></table>", table)
    xml("<table><tr><td>a<lb/><hi>b</hi></td></tr></table>", table)
    xml("<table><tr><td>a<lb/><graphic src=\"a.png\"/></td></tr></table>", table, images=True)
    xml("<table><tr><td>a<lb/> <hi>b</hi></td></tr></table>", table)
    xml("<table><tr><td>a<lb/>b<lb/>c</td></tr></table>", table)
    xml("<table><tr><td><lb/>a</td></tr></table>", table)
    xml("<table><tr><td>a <hi>b</hi> c <del>d</del> e</td></tr></table>", table)
    xml('<table><tr><td>a <ref target="/x">link</ref> b</td></tr></table>', table, links=True)
    xml('<table><tr><td>a <ref target="/x">link</ref> b</td></tr></table>', table, links=False)
    xml('<table><tr><td><ref target="/x"><hi>link text</hi></ref></td></tr></table>', table, links=True)
    xml("<table><tr><td><hi><hi>x</hi></hi></td></tr></table>", table)
    xml('<table><tr><td><hi rend="#b">x</hi> tail</td></tr></table>', table)
    xml("<table><tr><td><hi> </hi></td></tr></table>", table)
    xml("<table><tr><td><p>one</p><p>two <hi>b</hi></p></td></tr></table>", table)
    xml("<table><tr><td><p></p><p> </p></td></tr></table>", table)
    xml("<table><tr><td>a<p>one</p>tail text</td></tr></table>", table)
    xml("<table><tr><td><head>title</head>body</td></tr></table>", table)
    xml("<table><tr><td><quote>q</quote> t</td></tr></table>", table)
    xml('<table><tr><td><code>c = 1</code></td></tr></table>', table)
    xml("<table><tr><td><list><item>x</item><item>y</item></list></td></tr></table>", table, focus="balanced")
    xml("<table><tr><td><list><item>x</item><item>y</item></list> tail</td></tr></table>", table, focus="balanced")
    xml("<table><tr><td><list><item>x</item><item>y</item></list></td></tr></table>", table, focus="recall")
    xml("<table><tr><td><list><item>x</item></list> tail</td></tr></table>", table, focus="recall")
    xml("<table><tr><td>a<list><item></item></list></td></tr></table>", table, focus="recall")
    xml("<table><tr><td>a<div>in div</div> tail</td></tr></table>", table)
    xml("<table><tr><td><div><p>deep</p></div></td></tr></table>", table)
    xml("<table><tr><td><span>x</span></td></tr></table>", table)
    xml("<table><tr><td><foo>unknown</foo> tail</td></tr></table>", table)
    xml('<table><tr><td><graphic src="a.png" alt="pic"/></td></tr></table>', table, images=True)
    xml('<table><tr><td><graphic src="a.png"/></td></tr></table>', table, images=False)
    xml('<table><tr><td>t <graphic src="/img/a.png"/> u</td></tr></table>', table, images=True,
        url="http://example.org/dir/page")
    xml("<table><tr><td> text </td><td>\t</td></tr></table>", table)
    xml("<table><tr><td>Instagram</td></tr></table>", table)
    xml("<table><tr><td>cell</td></tr></table>", table, tables=False)
    xml("<table><tr><td>cell<table><tr><td>n</td></tr></table></td></tr></table>", table, ptags=["p", "table"])
    xml("<table><tr><td>a</td></tr></table>", table, ptags=["p"])
    xml('<table><tr><td rowspan="2"><p>a</p></td><td><hi>b</hi></td></tr><tr><td>c</td></tr></table>', table)


# ---------------------------------------------------------------- handle_lists


def list_cases() -> None:
    lst = "handle_lists"
    xml("<list><item>a</item><item>b</item></list>", lst)
    xml('<list rend="ol"><item>a</item></list>', lst)
    xml('<list><item rend="x">a</item><item>b</item></list>', lst)
    xml("<list>intro text<item>a</item></list>", lst)
    xml("<list>   <item>a</item></list>", lst)
    xml("<list><item> a  b </item></list>", lst)
    xml("<list><item>a</item>tail<item>b</item></list>", lst)
    xml("<list><item></item><item> </item></list>", lst)
    xml("<list></list>", lst)
    xml("<list><item>Instagram</item><item>kept</item></list>", lst)
    xml("<list><item>a</item><item>a</item></list>", lst)
    xml("<list><foo>x</foo><item>a</item></list>", lst)
    xml("<list><item>a</item><list><item>direct nested</item></list></list>", lst)
    # items with children: text, inline elements, tails
    xml("<list><item>a <hi>b</hi> c</item></list>", lst)
    xml("<list><item>a <hi>b</hi></item><item>c</item></list>", lst)
    xml("<list><item><hi>b</hi> c</item></list>", lst)
    xml("<list><item>a<lb/>b</item></list>", lst)
    xml("<list><item>a<lb/><hi>b</hi></item></list>", lst)
    xml('<list><item>a <ref target="/x">link</ref> b</item></list>', lst, links=True)
    xml('<list><item>a <ref target="/x"><hi>link</hi></ref></item></list>', lst, links=True)
    xml("<list><item><hi>a<hi>nested</hi></hi> t</item></list>", lst)
    xml("<list><item><del>a</del><code>b</code></item></list>", lst)
    xml("<list><item>a <hi>b</hi></item>tail<item>c</item></list>", lst)
    xml("<list><item>a<foo>x</foo>b</item></list>", lst)
    xml("<list><item>a<foo>x<hi>y</hi></foo> after</item></list>", lst)
    xml("<list><item><foo> </foo></item></list>", lst)
    xml("<list><item><span>in span</span> tail</item></list>", lst)
    # paragraphs, images and quotes inside items
    xml("<list><item><p>para <hi>b</hi></p></item></list>", lst)
    xml("<list><item><p>plain</p></item></list>", lst)
    xml("<list><item>lead<p>one <hi>b</hi></p> tail</item></list>", lst)
    xml("<list><item><p>one <hi>b</hi></p>tail<p>two <hi>c</hi></p>tail2</item></list>", lst)
    xml("<list><item><p></p> tail</item></list>", lst)
    xml("<list><item>a<p><hi></hi></p>b</item></list>", lst)
    xml("<list><item>a<p><hi></hi></p> b</item></list>", lst)
    xml("<list><item>a <p><hi></hi></p>b</item></list>", lst)
    xml("<list><item><p><hi></hi></p>b</item></list>", lst)
    xml("<list><item><hi>x</hi><p><hi></hi></p> tail</item></list>", lst)
    xml("<list><item><hi>x</hi><p><hi></hi></p>tail</item></list>", lst)
    xml("<list><item><hi>x</hi><p><hi></hi></p>  </item></list>", lst)
    xml('<list><item>a<graphic src="a.png"/>b</item></list>', lst, images=True)
    xml('<list><item><graphic src="a.png" alt="pic"/></item></list>', lst, images=True)
    xml('<list><item><graphic src="a.png"/> tail</item></list>', lst, images=True)
    xml('<list><item>x<graphic src="data:image/png;base64,AAAA"/>y</item></list>', lst, images=True)
    xml('<list><item>x<graphic alt="no source"/>  y</item></list>', lst, images=True)
    xml('<list><item>x<graphic alt="no source"/>y</item></list>', lst, images=True)
    xml('<list><item><graphic alt="no source"/></item></list>', lst, images=True)
    xml("<list><item><quote>quoted</quote> tail</item></list>", lst)
    xml("<list><item><head>title</head></item></list>", lst)
    # nested lists: the first nested item is skipped by the outer walk
    xml("<list><item>a<list><item>b</item><item>c</item></list></item><item>d</item></list>", lst)
    xml("<list><item>a<list><item>b</item></list> tail</item></list>", lst)
    xml("<list><item>a<list><item>b</item><item>c</item></list></item></list>", lst)
    xml("<list><item>a<list><item>b<list><item>c</item></list></item></list></item></list>", lst)
    xml("<list><item><list><item>only nested</item></list></item></list>", lst)
    xml("<list><item>a<list></list></item></list>", lst)
    xml("<list><item>a<list>intro<item>b</item></list></item></list>", lst)
    xml('<list rend="ul"><item>a<list rend="ol"><item>b</item></list></item></list>', lst)
    xml("<list><item>a<list><item>b</item></list></item><item>c<list><item>d</item></list></item></list>", lst)
    # a stripped successor ends the live walk
    xml("<list><item><p><foo>x</foo> y</p><p>z <hi>w</hi></p></item></list>", lst)
    xml("<list><item><p>x y</p><p>z <hi>w</hi></p></item></list>", lst)
    xml("<list><item><p><hi>x</hi> y</p><p>z <hi>w</hi></p></item></list>", lst)
    xml("<list><item><p>a <hi>b<lb/>c</hi> e <hi>f</hi> g</p></item></list>", lst)


# ---------------------------------------------------------------- handle_quotes


def quote_cases() -> None:
    quote = "handle_quotes"
    xml("<quote>text</quote>", quote)
    xml("<quote> spaced  text </quote>", quote)
    xml("<quote><p>a</p><p>b <hi>c</hi></p></quote>", quote)
    xml("<quote>lead<p>a <hi>b</hi></p>tail</quote>", quote)
    xml("<quote><quote>inner</quote> tail</quote>", quote)
    xml("<quote>a<quote>b<quote>c</quote></quote></quote>", quote)
    xml("<quote></quote>", quote)
    xml("<quote> </quote>", quote)
    xml("<quote><p></p></quote>", quote)
    xml("<quote><lb/></quote>", quote)
    xml("<quote>a<lb/>b</quote>", quote)
    xml("<quote>a <hi>b</hi> <ref target='/x'>c</ref></quote>", quote, links=True)
    xml("<quote><list><item>a</item><item>b</item></list></quote>", quote)
    xml("<quote>a<list><item>b<list><item>c</item></list></item></list></quote>", quote)
    xml('<quote><graphic src="a.png"/>cap</quote>', quote, images=True)
    xml('<quote><p><graphic src="a.png"/></p></quote>', quote, images=True)
    xml('<quote><p>a <graphic src="a.png"/> b</p></quote>', quote, images=True)
    xml("<quote><foo>unknown</foo> tail</quote>", quote)
    xml("<quote>Instagram</quote>", quote)
    # code blocks: lang attribute, a code tag, a highlight parent, a lone code child (highlightjs)
    xml('<quote lang="python">x = 1</quote>', quote)
    xml('<quote lang="">x = 1</quote>', quote)
    xml("<code>x = 1</code>", quote)
    xml("<code><hi>x</hi> = 1</code>", quote)
    xml('<wrap><div class="highlight"><quote>y = 2</quote></div></wrap>', quote, target="quote")
    xml('<div class="a highlight b"><code>y = 2</code></div>', quote, target="code")
    xml('<div class="hl"><quote>y = 2</quote></div>', quote, target="quote")
    xml("<quote><code>z = 3</code></quote>", quote)
    xml("<quote><code>z = 3</code> </quote>", quote)
    xml("<quote> <code>z = 3</code></quote>", quote)
    xml("<quote>lead<code>z = 3</code></quote>", quote)
    xml("<quote><code>z = 3</code>tail</quote>", quote)
    xml("<quote><code>z</code><code>y</code></quote>", quote)
    xml("<quote><hi><code>z</code></hi></quote>", quote)
    xml('<quote><code lang="x">z</code></quote>', quote)


# ---------------------------------------------------------------- handle_paragraphs, titles, formatting


def paragraph_cases() -> None:
    para_ = "handle_paragraphs"
    xml("<p>plain text</p>", para_)
    xml("<p>  </p>", para_)
    xml("<p></p>", para_)
    xml("<p>Instagram</p>", para_)
    xml("<p>a <hi>b</hi> c</p>", para_)
    xml("<p><hi>b</hi></p>", para_)
    xml("<p><hi>b</hi> </p>", para_)
    xml('<p>a <hi rend="#b">b</hi> c <hi rend="#i" x="y">d</hi></p>', para_, formatting=True)
    xml("<p>a <del>b</del> <code>c</code></p>", para_)
    xml("<p>a <hi>b<hi>c</hi></hi> d</p>", para_)
    xml("<p>a <hi>b<quote>q</quote></hi> c</p>", para_)
    xml("<p><hi>b<head>x</head></hi></p>", para_)
    xml("<p><hi>b<quote> </quote></hi></p>", para_)
    xml("<p>a<lb>text</lb></p>", para_)
    xml("<p>a <hi>b <hi>c <hi>d</hi></hi></hi> e</p>", para_)
    xml("<p><hi><lb/>x</hi></p>", para_)
    xml("<p>a <hi>b<lb/> c</hi> d</p>", para_)
    xml("<p>a <hi>b<lb/>c</hi></p>", para_)
    xml("<p>a <hi><code>b</code></hi> c</p>", para_)
    xml("<p>a <hi><ref target='/x'>b</ref></hi> c</p>", para_, links=True)
    xml("<p>a <ref target='/x'><hi>b</hi></ref> c</p>", para_, links=True)
    xml("<p>a <ref target='/x'>b</ref> c</p>", para_, links=True)
    xml("<p>a <ref target='/x'>b</ref> c</p>", para_, links=False)
    xml("<p>a <ref target='/x'><foo>b</foo></ref> c</p>", para_, links=True)
    xml("<p>a <del><hi>b</hi></del> c</p>", para_)
    xml("<p>a <hi><graphic src='a.png'/></hi> c</p>", para_, images=True)
    # line breaks
    xml("<p>a<lb/>b</p>", para_)
    xml("<p>a<lb/></p>", para_)
    xml("<p>a<lb/> </p>", para_)
    xml("<p>a<lb/><lb/>b</p>", para_)
    xml("<p><lb/>a</p>", para_)
    xml("<p><lb/></p>", para_)
    xml("<p>a<lb/><hi>x</hi></p>", para_)
    xml("<p>a<lb/> <hi>x</hi></p>", para_)
    xml("<p>a<lb/><graphic src='a.png'/></p>", para_, images=True)
    xml("<p>a<lb/><lb/><hi>x</hi></p>", para_)
    xml("<p>a <lb/>b <lb/> c</p>", para_)
    # unexpected children keep their text in place
    xml("<p>a <foo>b</foo> c</p>", para_)
    xml("<p>a <foo>b <hi>c</hi></foo> d</p>", para_)
    xml("<p>a <div>b</div> c</p>", para_)
    xml("<p>a <div>b</div> c</p>", para_, div=True)
    xml("<p>a <span>b</span> c</p>", para_)
    xml("<p><foo></foo>a</p>", para_)
    xml("<p><foo><bar>x</bar></foo></p>", para_)
    xml("<p>a <done>b</done> c</p>", para_)
    xml("<p>a <table><tr><td>t</td></tr></table> c</p>", para_)
    xml("<p>a <quote>q</quote> c</p>", para_)
    xml("<p>a <head>h</head> c</p>", para_)
    xml("<p>a <list><item>i</item></list> c</p>", para_)
    # a nested paragraph is folded into the text
    xml("<p>a <p>inner</p> c</p>", para_)
    xml("<p><p>inner</p></p>", para_)
    xml("<p>a <p>one</p> b <p>two</p> c</p>", para_)
    xml("<p><p></p>a</p>", para_)
    xml("<p>a <p>inner <hi>b</hi></p> c</p>", para_)
    # images
    xml("<p>a <graphic src='a.png' alt='x' title='t'/> b</p>", para_, images=True)
    xml("<p>a <graphic src='a.png'/> b</p>", para_, images=False)
    xml("<p>a <graphic data-src='b.jpg' src='a.png'/> b</p>", para_, images=True)
    xml("<p>a <graphic data-src-big='b.jpg'/> b</p>", para_, images=True)
    xml("<p>a <graphic src='//cdn.example/a.png'/> b</p>", para_, images=True)
    xml("<p>a <graphic src='//cdn.example/a.png'/> b</p>", para_, images=True, url="https://example.org/dir/p")
    xml("<p>a <graphic src='rel/a.png'/> b</p>", para_, images=True, url="https://example.org/dir/p")
    xml("<p>a <graphic src='/abs/a.png'/> b</p>", para_, images=True, url="https://example.org/dir/p")
    xml("<p>a <graphic src='https://x.test/a.png'/> b</p>", para_, images=True, url="https://example.org/dir/p")
    xml("<p>a <graphic src='http://x.test/a.png'/> b</p>", para_, images=True)
    xml("<p>a <graphic src='data:image/png;base64,AAAA'/> b</p>", para_, images=True)
    xml("<p>a <graphic alt='none'/> b</p>", para_, images=True)
    xml("<p><graphic src='a.png'/></p>", para_, images=True)
    xml("<p><graphic src='a.png'/>tail</p>", para_, images=True)
    xml("<p>a <graphic src='a.png'/><graphic src='b.png'/> b</p>", para_, images=True)
    # the walk ends when its precomputed successor is stripped
    xml("<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>", para_)
    xml("<p>A <hi>b c</hi> e <hi>f</hi> g</p>", para_)
    xml("<p>A <hi>b<foo>x</foo>c</hi> e <hi>f</hi> g</p>", para_)
    xml("<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>", para_, ptags=["p", "hi", "lb", "code", "del", "head", "list"])
    xml("<p>A <hi>b<lb/>c</hi></p>", para_)
    xml("<p>A <hi>b<lb/>c</hi><hi>d</hi></p>", para_)
    xml("<p>A <foo>x</foo> y <hi>z</hi> w</p>", para_)
    xml("<p><foo>x</foo> y <hi>z</hi> w</p>", para_)
    xml("<p><foo>x<bar>y</bar></foo><hi>z</hi>w</p>", para_)
    # narrower and wider tag sets
    xml("<p>a <hi>b</hi> c</p>", para_, ptags=["p"])
    xml("<p>a <hi>b</hi> c</p>", para_, ptags=["p", "hi"])
    xml("<p>a <del>b</del> c</p>", para_, ptags=["p", "hi"])
    xml("<p>a <ref target='/x'>b</ref> c</p>", para_, ptags=["p", "ref"], links=True)


def title_cases() -> None:
    head = "handle_titles"
    xml("<head>title</head>", head)
    xml('<head rend="h2">title</head>', head)
    xml("<head> spaced  title </head>", head)
    xml("<head></head>", head)
    xml("<head> </head>", head)
    xml("<head>Instagram</head>", head)
    xml("<head>a <hi>b</hi> c</head>", head)
    xml("<head><hi>b</hi></head>", head)
    xml("<head><done>x</done>y</head>", head)
    xml("<head><done>x</done></head>", head)
    xml("<head>a<lb/>b</head>", head)
    xml("<head>a<hi>b<hi>c</hi></hi></head>", head)
    xml("<head><graphic src='a.png'/></head>", head, images=True)
    xml("<head>a<foo>b</foo>c</head>", head)
    xml("<head><lb/></head>", head)
    xml('<wrap><head>a <hi>b</hi></head></wrap>', head, target="head")


def formatting_cases() -> None:
    fmt = "handle_formatting"
    xml("<hi>x</hi>", fmt)
    xml("<wrap><hi>x</hi> tail</wrap>", fmt, target="hi")
    xml("<hi></hi>", fmt)
    xml("<hi> </hi>", fmt)
    xml("<hi>Instagram</hi>", fmt)
    xml('<hi rend="#b">x</hi>', fmt, formatting=True)
    xml("<hi>a<hi>b</hi>c</hi>", fmt)
    xml("<ref target='/x'>x</ref>", fmt, links=True)
    xml("<del>x</del>", fmt)
    xml("<hi><lb/>x</hi>", fmt)
    for parent in ("cell", "head", "hi", "item", "p", "quote", "ref", "td", "div", "list", "body", "code", "row"):
        xml(f"<{parent}><hi>x</hi> tail</{parent}>", fmt, target="hi")
        xml(f"<{parent}>lead<hi>x</hi></{parent}>", fmt, target="hi")
    xml("<p>a<hi> </hi>b</p>", fmt, target="hi")
    xml("<div><hi></hi>tail</div>", fmt, target="hi")
    xml("<div><hi>x</hi></div>", fmt, target="hi")


# ---------------------------------------------------------------- handle_textelem


def textelem_cases() -> None:
    elem = "handle_textelem"
    xml("<list><item>a</item></list>", elem)
    xml("<quote>q</quote>", elem)
    xml("<code>c</code>", elem)
    xml('<quote lang="x">q</quote>', elem)
    xml("<head>h</head>", elem)
    xml("<p>para <hi>b</hi></p>", elem)
    xml("<p>plain</p>", elem)
    # lb: only a tail with text makes a paragraph
    xml("<lb/>", elem)
    xml("<lb>text</lb>", elem)
    xml('<wrap><lb/>tail text</wrap>', elem, target="lb")
    xml('<wrap><lb/> </wrap>', elem, target="lb")
    xml('<wrap><lb/></wrap>', elem, target="lb")
    xml("<wrap><lb/>Instagram</wrap>", elem, target="lb")
    xml("<wrap><lb/> spaced  tail </wrap>", elem, target="lb")
    xml("<wrap><lb/><hi>x</hi></wrap>", elem, target="lb")
    # inline elements outside paragraphs
    xml("<hi>x</hi>", elem)
    xml("<wrap><hi>x</hi></wrap>", elem, target="hi")
    xml("<item><hi>x</hi></item>", elem, target="hi")
    xml("<ref target='/x'>x</ref>", elem, links=True)
    xml("<del>x</del>", elem)
    # tables and images need their tag in potential_tags
    xml("<table><tr><td>a</td></tr></table>", elem)
    xml("<table><tr><td>a</td></tr></table>", elem, tables=False)
    xml("<table><tr><td>a</td></tr></table>", elem, ptags=["p"])
    xml('<graphic src="a.png" alt="x"/>', elem, images=True)
    xml('<graphic src="a.png"/>', elem, images=False)
    xml('<graphic src="a.png"/>', elem, ptags=["p"], images=True)
    xml('<graphic alt="none"/>', elem, images=True)
    xml('<graphic src="data:image/png;base64,AAAA"/>', elem, images=True)
    xml('<graphic src="/a.png" title="t"/>', elem, images=True, url="http://example.org/x/y")
    xml('<graphic src="//cdn.example/a.png"/>', elem, images=True)
    xml('<graphic data-src-1="//cdn.example/a.jpg"/>', elem, images=True)
    xml('<wrap><graphic src="a.png"/>tail</wrap>', elem, target="graphic", images=True)
    # other elements: w3schools code, div with and without div in potential_tags, unknown tags
    xml('<div class="w3-code notranslate">x = 1</div>', elem)
    xml('<div class="w3-code"><hi>x</hi></div>', elem, div=True)
    xml("<div>text</div>", elem)
    xml("<div>text</div>", elem, div=True)
    xml('<div id="a" class="b">text <hi>b</hi></div>', elem, div=True)
    xml("<div>  </div>", elem, div=True)
    xml("<div></div>", elem, div=True)
    xml("<div><p>inner</p></div>", elem, div=True)
    xml("<div>Instagram</div>", elem, div=True)
    xml("<div>a<lb/>b</div>", elem, div=True)
    xml("<div>  a  b  </div>", elem, div=True)
    xml("<div> text </div>", elem, div=True)
    xml("<div>a<hi>b</hi>c<lb/>d</div>", elem, div=True)
    xml("<wrap><div>text</div>tail</wrap>", elem, target="div", div=True)
    xml("<foo>unknown</foo>", elem)
    xml("<done>x</done>", elem)
    xml("<span>x</span>", elem)
    xml("<row><cell>x</cell></row>", elem)
    xml("<item>x</item>", elem)
    xml("<body><p>x</p></body>", elem)


# ---------------------------------------------------------------- HTML pipeline


def frame_cases() -> None:
    body = para(1) + para(2)
    outside = f'<div id="outside">{para(3, 120)}</div>'
    # every BODY_XPATH expression picks its frame; the content outside it stays out
    frames = [
        ('<div class="post">', "</div>"),
        ('<section itemprop="articleBody">', "</section>"),
        ('<div id="entry-content">', "</div>"),
        ("<article>", "</article>"),
        ('<div role="article">', "</div>"),
        ('<div id="story">', "</div>"),
        ('<div class="postarea">', "</div>"),
        ('<main class="main-content">', "</main>"),
        ('<div id="content">', "</div>"),
        ('<div class="content">', "</div>"),
        ('<div class="main">', "</div>"),
        ('<section id="main-area">', "</section>"),
        ("<main>", "</main>"),
    ]
    for opening, closing in frames:
        html(page(f"{outside}{opening}{body}{closing}"))
    for focus in ("precision", "recall"):
        html(page(f"{outside}<article>{body}</article>"), focus=focus)
    # frame text and the paragraph text of the document: the length gate that adds div
    for count in (0, 1, 2, 4):
        html(page(f"<article>{''.join(para(10 + i, 200) for i in range(count))}<div>wrapped words</div></article>"))
        html(page(f"<article>{''.join(para(20 + i, 200) for i in range(count))}<div>wrapped words</div></article>"),
             focus="precision")
    html(page(f"<article><div>{prose(5, 300)}</div>{para(6)}</article>"))
    html(page(f"<article><div>{prose(5, 300)}</div><div>{prose(6, 300)}</div></article>"))
    html(page(f"<article><div>{prose(5, 300)}</div><div>{prose(6, 300)}</div></article>"), focus="recall")
    # an empty frame after pruning, a frame with no children, the next expression takes over
    html(page(f'<div class="entry-content"><div class="sidebar">{para(7)}</div></div><article>{body}</article>'))
    html(page(f'<div class="entry-content">only text</div><article>{body}</article>'))
    html(page(f'<div class="entry-content"></div><article>{body}</article>'))
    # a lone image does not end the search: the next frame adds to the body
    img = '<img src="a.png" alt="pic">'
    html(page(f'<div class="entry-content">{img}</div><article>{body}</article>'), images=True)
    html(page(f'<div class="entry-content">{img}</div><article>{para(8, 20)}</article>'), images=True)
    html(page(f'<div class="entry-content">{img}</div>'), images=True)
    html(page(f"<article>{img}{para(9, 30)}</article>"), images=True)
    html(page(f"<article>{img}{img}</article>"), images=True)
    html(page(f"<article>{para(9, 30)}{img}</article>"), images=True)
    # nothing matches a frame: the recovery pass takes the paragraphs
    html(page(body))
    html(page(f"<div>{body}</div>"))
    html(page(f"<div>{para(11, 40)}</div>"))
    html(page(""))
    html(page("<p> </p>"))


def single_run_cases() -> None:
    # the frame is one text run (only lb and inline elements): processed as a whole, as a paragraph
    run = f"{prose(30, 120)}<br>{prose(31, 120)}<br>{prose(32, 120)}"
    html(page(f'<div class="entry-content">{run}</div>'))
    html(page(f"<article>{run}</article>"))
    html(page(f'<div class="entry-content">{prose(33, 100)} <b>bold</b> and <i>italic</i> {prose(34, 100)}</div>'),
         formatting=True)
    html(page(f'<div class="entry-content">{prose(33, 100)} <b>bold</b> and <i>italic</i> {prose(34, 100)}</div>'))
    html(page(f'<div class="entry-content">{prose(35, 300)} <a href="/x">link text</a> {prose(36, 100)}</div>'),
         links=True)
    html(page(f'<div class="entry-content">{prose(35, 300)} <code>code</code></div>'))
    html(page(f'<div class="entry-content">{prose(37, 300)}<br></div>'))
    html(page(f'<div class="entry-content"><br>{prose(37, 300)}</div>'))
    html(page('<div class="entry-content"><br><br></div>'))
    html(page(f'<div class="entry-content">short<br>text</div>'))
    html(page(f'<div class="entry-content">{prose(38, 300)}<img src="a.png"></div>'), images=True)
    html(page(f'<div class="entry-content">{prose(38, 300)}<sub>x</sub> more</div>'))
    # the lead text of a frame with children: kept only when the frame is accepted
    html(page(f'<div class="entry-content">Lead words of the frame {para(40)}{para(41)}</div>'))
    html(page(f'<article>Lead words of the frame {para(40)}{para(41)}</article>'))
    html(page(f'<article>Lead words of the frame {para(40)}</article>'))
    html(page(f'<article>   {para(40)}{para(41)}</article>'))
    html(page(f'<article>Instagram{para(40)}{para(41)}</article>'))
    html(page(f'<article>Lead <b>bold</b> words{para(40)}{para(41)}</article>'), formatting=True)
    html(page(f'<div class="entry-content">Lead {para(40)}</div><article>{para(41)}{para(42)}</article>'))
    html(page(f"<article>Lead text here<p>x</p></article>"))


def rebuilt_block_cases() -> None:
    # text after a rebuilt block (list, table, quote, code) is a paragraph of its own
    tail = "tail text after the block"
    blocks = {
        "list": "<ul><li>item one</li><li>item two</li></ul>",
        "table": "<table><tr><td>cell</td></tr></table>",
        "quote": "<blockquote>quoted words here</blockquote>",
        "code": "<pre>x = 1\ny = 2</pre>",
        "dl": "<dl><dt>term</dt><dd>definition</dd></dl>",
    }
    for name, block in blocks.items():
        html(page(f"<article>{para(50)}{block}{tail}{para(51)}</article>"))
        html(page(f"<article>{para(50)}{para(51)}{block}{tail}</article>"))
        html(page(f"<article>{para(50)}{para(51)}{block} </article>"))
        html(page(f"<article>{para(50)}{para(51)}{block}Instagram</article>"))
    html(page(f"<article>{para(50)}<pre>a</pre>{tail}<pre>b</pre>{tail}{para(51)}</article>"))
    html(page(f"<article>{para(50)}<ul><li>x</li></ul>{tail}<ul><li>y</li></ul>{tail}{para(51)}</article>"),
         focus="recall")
    html(page(f"<article>{para(50)}{blocks['table']}{tail}{para(51)}</article>"), tables=False)
    html(page(f"<article>{para(50)}{para(51)}<div>{blocks['list']}{tail}</div></article>"))
    html(page(f"<article>{para(50)}<ul><li></li></ul>{tail}{para(51)}</article>"))
    # trailing titles and links are removed; a title in front stays
    html(page(f"<article>{para(52)}{para(53)}<h2>Related</h2></article>"))
    html(page(f"<article>{para(52)}{para(53)}<h2>Related</h2><h3>More</h3></article>"))
    html(page(f"<article><h2>First</h2>{para(52)}{para(53)}</article>"))
    html(page(f"<article>{para(52)}{para(53)}<h2>Related</h2>tail</article>"))
    html(page(f'<article>{para(52)}{para(53)}<a href="/x">{prose(54, 30)}</a></article>'), links=True)
    html(page(f'<article>{para(52)}{para(53)}<a href="/x">{prose(54, 30)}</a>{para(55, 20)}</article>'),
         links=True)
    html(page(f"<article>{para(52)}{para(53)}<h2>Related</h2><p> </p></article>"))
    html(page(f"<article><h2>Only title</h2></article>"))
    html(page(f"<article><h2>Only title</h2>{para(52)}</article>"))


def dedup_cases() -> None:
    # adjacent repeats longer than MIN_DUPLICATE_LENGTH are dropped, short ones and spaced ones stay
    same = prose(60, 120)
    short = "Short repeat."
    html(page(f"<article><p>{same}</p><p>{same}</p><p>{prose(61, 200)}</p></article>"))
    html(page(f"<article><p>{same}</p><p>{same}</p><p>{same}</p><p>{prose(61, 200)}</p></article>"))
    html(page(f"<article><p>{short}</p><p>{short}</p><p>{prose(61, 300)}</p></article>"))
    html(page(f"<article><p>{same}</p><p>{prose(61, 200)}</p><p>{same}</p></article>"))
    html(page(f"<article><p>{same}</p><p>{same}</p></article>"))
    html(page(f"<article><p>{same}</p><p> </p><p>{same}</p><p>{prose(61, 200)}</p></article>"))
    html(page(f"<article><p>{same}</p><ul><li>x</li></ul><p>{same}</p><p>{prose(61, 200)}</p></article>"))
    html(page(f"<article><p>{same}</p><div><p>{same}</p></div><p>{prose(61, 200)}</p></article>"))
    # line breaks that only separated blocks lose their whitespace tails
    html(page(f"<article>{para(62)}<br> <br>{para(63)}</article>"))
    html(page(f"<article>{para(62)}<div><br> </div>{para(63)}</article>"))
    html(page(f"<article>{para(62)}<div>{prose(64, 300)}<br>   </div>{para(63)}</article>"))
    html(page(f"<article>{para(62)}<div>{prose(64, 300)}<br>line two</div>{para(63)}</article>"))
    html(page(f"<article>{para(62)}<pre>a<br>   b</pre>{para(63)}</article>"))
    html(page(f"<article><p>{prose(62, 300)}<br>   </p>{para(63)}</article>"))
    html(page(f"<article><div>{prose(62, 300)}<br>   </div><div>{prose(63, 300)}</div></article>"), focus="recall")
    # divs holding text are emitted as paragraphs once div is a potential tag
    html(page(f"<article><div>{prose(65, 300)}<div>{prose(66, 300)}</div></div></article>"))
    html(page(f"<article><div>{prose(65, 300)}<div>{prose(66, 300)}</div></div>{para(67)}</article>"))


def recovery_cases() -> None:
    # the main pass finds a short frame or none: recover_wild_text reads the whole document
    wild = prose(70, 150)
    html(page(f"<article><p>short one</p><p>short two</p></article><div><p>{wild}</p></div>"))
    html(page(f"<article><p>short one</p></article><p>{wild}</p>"))
    html(page(f"<p>{wild}</p><article><p>short one</p><p>short two</p></article>"))
    # dedup exits: exact text, a substring longer than the minimum, a fragment of a text run, an image-only block
    exact = prose(71, 30)
    html(page(f"<article><p>{exact}</p><p>short two</p></article><div><p>{exact}</p></div><p>{wild}</p>"))
    long = prose(72, 100)
    html(page(f"<article><div>{long} and a tail of the long run</div><p>short two</p></article>"
              f"<section><p>{long}</p></section>"), focus="recall")
    html(page(f"<article><p>{long} and a tail of the long run</p><p>x</p></article><section><p>{long}</p></section>"))
    html(page(f"<article><p>{prose(73, 40)}</p></article><section><p>{prose(73, 40)[:30]}</p></section>"))
    frag = prose(74, 20)
    html(page(f"<article><div>{frag} tail words of the run<br>x</div></article><p>{frag}</p> tail words of the run"),
         focus="recall")
    html(page(f"<article><p>{frag} tail words of the run</p></article><p>{frag}</p> tail words of the run"))
    html(page(f"<article><p>{frag}</p><p>{prose(75, 20)}</p></article><p>{prose(76, 20)}</p>{prose(77, 20)}"))
    html(page(f'<article><p>short</p>{"<img src=a.png>"}</article><div><img src="a.png"></div><p>{wild}</p>'),
         images=True)
    html(page(f'<article><p>short</p><img src="a.png"></article><p><img src="a.png"></p><p>{wild}</p>'), images=True)
    html(page(f'<article><p>short</p><img src="a.png"></article><p><img src="b.png"></p>'), images=True)
    html(page(f'<p><img src="a.png"></p><p><img src="a.png"></p><p><img src="b.png"></p><p>{wild}</p>'), images=True)
    html(page(f'<p><img src="a.png"> {wild}</p>'), images=True)
    # consumed twins: what the main pass took is not taken again; a copy left by a rebuilt block is
    html(page(f"<article><ul><li>{prose(78, 80)}</li></ul><p>short two</p></article>"))
    html(page(f"<article><dl><dt>Term</dt><dd><p>{prose(79, 80)}</p></dd></dl></article>"))
    html(page(f"<article><blockquote><p>{prose(80, 80)}</p></blockquote></article>"))
    html(page(f"<article><table><tr><td><p>{prose(81, 60)}</p></td></tr></table></article>"))
    html(page(f"<article><pre><code>{prose(82, 60)}</code></pre></article>"))
    # the search expression: code, quote, table, w3-code, and with recall div, lb and list
    html(page(f'<div><pre>x = 1</pre><blockquote>quoted wild words</blockquote>'
              f'<table><tr><td>wild cell</td></tr></table><div class="w3-code">w3 code</div><p>{wild}</p></div>'))
    html(page(f'<div><pre>x = 1</pre><blockquote>quoted wild words</blockquote>'
              f'<table><tr><td>wild cell</td></tr></table><div class="w3-code">w3 code</div><p>{wild}</p></div>'),
         tables=False)
    html(page(f"<div>plain div text<br>after break<ul><li>wild item</li></ul><p>{wild}</p></div>"))
    html(page(f"<div>plain div text<br>after break<ul><li>wild item</li></ul><p>{wild}</p></div>"), focus="recall")
    html(page(f"<section><div>a div {prose(83, 60)}</div><div>b div {prose(84, 60)}</div></section>"), focus="recall")
    html(page(f"<section><div>a div {prose(83, 60)}<div>inner div {prose(84, 60)}</div></div></section>"),
         focus="recall")
    html(page(f"<section>text<br>line</section><p>{wild}</p>"), focus="recall")
    html(page(f"<ul><li>{prose(85, 60)}</li></ul>"), focus="recall")
    html(page(f"<ul><li>{prose(85, 60)}</li><li>{prose(86, 60)}</li></ul><ul><li>{prose(85, 60)}</li></ul>"),
         focus="recall")
    html(page("<br><br>"), focus="recall")
    # a teaser-class block is kept on this path (fast mode), pruned on the main path
    teaser = f'<div class="teaser"><p>{prose(87, 150)}</p></div>'
    html(page(teaser))
    html(page(f"{teaser}<p>short</p>"))
    html(page(f"<article><p>short one</p></article>{teaser}"))
    html(page(f'<article><div class="teaser-box"><p>{prose(88, 150)}</p></div>{para(89)}</article>'))
    html(page(f'<article><div class="teaser-box"><p>{prose(88, 150)}</p></div>{para(89)}</article>'), focus="precision")
    html(page(f'<article><div class="teaser-box"><p>{prose(88, 150)}</p></div>{para(89)}</article>'), focus="recall")
    html(page(f'<div id="Teaser"><p>{prose(87, 150)}</p></div>'), focus="precision")
    html(page(f'<div id="Teaser"><p>{prose(87, 150)}</p></div>'), focus="recall")
    # the recovery runs on the unpruned backup: boilerplate is still pruned there
    html(page(f'<div class="sidebar"><p>{prose(90, 150)}</p></div><p>{wild}</p>'))
    html(page(f'<div class="sidebar"><p>{prose(90, 150)}</p></div><p>{wild}</p>'), focus="recall")
    html(page(f'<div>{links(91, 6, 30)}</div><p>{wild}</p>'), links=True)
    # other option sets on the same recovery
    html(page(f"<article><p>short one</p></article><p>{wild}</p>"), focus="precision")
    html(page(f"<article><p>short one</p></article><p>{wild}</p>"), tables=False, formatting=True)


def document_text_cases() -> None:
    # `//p//text()` in the body pass reads the whole document, even from a frame pruning detached
    outside = f"<div>{para(100, 900)}</div>"
    frame = f'<div class="entry-content">Intro words {links(110, 8)}</div>'
    html(page(f"{outside}{frame}"), links=True)
    html(page(f"{outside}{frame}"), links=False)
    html(page(f"{outside}{frame}"), links=True, focus="precision")
    html(page(f"{frame}{outside}"), links=True)
    html(page(f"{para(100, 400)}{frame}"), links=True)
    html(page(f"{para(100, 200)}{frame}"), links=True)
    html(page(f'{outside}<article>Intro words {links(110, 8)}</article>'), links=True)
    html(page(f"<div>{outside}{frame}</div>"), links=True)
    html(page(f"{frame}"), links=True)
    # lxml's itertext yields an empty text node, which the join counts: a line break that lost its tail still counts as a join piece
    html(page(f"<article><p>One comment here</p><br>Second line<p>third</p></article>"))
    html(page(f"<article><p>{prose(120, 120)}</p><br>{prose(121, 120)}<p>{prose(122, 120)}</p></article>"))
    html(page(f"<article><p>{prose(120, 80)}</p><br>{prose(121, 80)}<br>{prose(122, 80)}<p>third</p></article>"))
    html(page(f"<article><p>{prose(120, 60)}</p><br><br>{prose(121, 60)}<p>third</p></article>"))
    html(page("<article><p>a</p><br>b<p>c</p></article>"), focus="recall")
    html(page(f'<div class="comments"><p>One comment here</p><br>Second line<p>third</p></div>'),
         op="comments_then_content")
    html(page(f"<div id='comments'><p>One comment here</p><br>Second line<p>third</p></div>"),
         op="comments_then_content")
    # a foreign element among the frame's children keeps the single-run shortcut from applying
    html(page(f"<article>{para(130)}<svg><title>t</title></svg>{para(131)}</article>"))


def foreign_cases() -> None:
    # SVG and MathML survive cleaning; upstream treats their namespaced tags as plain strings
    svg = "<svg><title>t</title></svg>"
    html(page(f"<article>{para(140)}<p>a {svg} b {prose(141, 100)}</p>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<p>a <svg><g><text>deep</text></g></svg> b {prose(141, 100)}</p></article>"))
    html(page(f"<article>{para(140)}<p>a <math><mi>x</mi></math> b {prose(141, 100)}</p>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<p>a <b>b</b> {svg} <i>c</i> {prose(141, 100)}</p>{para(142)}</article>"),
         formatting=True)
    html(page(f"<article>{para(140)}<ul><li>a {svg} b</li><li>c</li></ul>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<ul><li>a <math><mi>x</mi></math> b</li></ul>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<blockquote>a {svg} b</blockquote>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<blockquote><p>a {svg} b</p></blockquote>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<table><tr><td>a {svg} b</td><td>c</td></tr></table>{para(142)}</article>"))
    html(page(f"<article>{para(140)}<table><tr><td>a <math><mi>x</mi></math> b</td></tr></table>{para(142)}</article>"))
    html(page(f"<article>{para(140)}{svg}{para(142)}</article>"))
    html(page(f"<article>{para(140)}<math><mi>x</mi></math>{para(142)}</article>"))
    html(page(f"<article>{svg}</article>"))
    html(page(f'<div class="entry-content">{prose(143, 200)}{svg}</div>'))
    html(page(f'<div class="entry-content">{prose(143, 200)}<br>{svg}</div>'))
    html(page(f"<article>{para(140)}<h2>a {svg} b</h2>{para(142)}</article>"))
    html(page(f"<div>{svg}<p>{prose(144, 200)}</p></div>"))
    html(page(f"<p>a {svg} b {prose(145, 200)}</p>"))
    html(page(f"<article><p>a {svg}</p><p>{prose(146, 100)}</p></article>"), focus="recall")


def table_pipeline_cases() -> None:
    cap = "<caption>Table caption</caption>"
    grid = "<table><tr><th>h1</th><th>h2</th></tr><tr><td>a</td><td>b</td></tr></table>"
    html(page(f"<article>{para(150)}{grid}{para(151)}</article>"))
    html(page(f"<article>{para(150)}{grid}{para(151)}</article>"), tables=False)
    html(page(f"<article>{para(150)}<table>{cap}<tr><td>a</td></tr></table>{para(151)}</article>"))
    html(page(f'<article>{para(150)}<table>{cap}<tr><td><img src="a.png"></td></tr></table>{para(151)}</article>'),
         images=True)
    html(page(f'<article>{para(150)}<table><caption><img src="a.png"> cap</caption><tr><td>a</td></tr></table>'
              f"{para(151)}</article>"), images=True)
    html(page(f'<article>{para(150)}<figure><img src="a.png"><figcaption>fig caption</figcaption></figure>'
              f"{para(151)}</article>"), images=True)
    html(page(f'<article>{para(150)}<figure><img src="a.png"><figcaption>fig caption</figcaption></figure>'
              f"{para(151)}</article>"), images=False)
    html(page(f"<article>{para(150)}<table><tr><td>outer<table><tr><td>inner</td></tr></table> after</td></tr></table>"
              f"{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td>o<table><tr><td>m<table><tr><td>deep</td></tr></table>"
              f"</td></tr></table></td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td>a</td></tr></table><table><tr><td>b</td></tr></table>"
              f"{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td rowspan='2'>a</td><td>b</td></tr><tr><td>c</td></tr></table>"
              f"{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td colspan='2'>a</td></tr><tr><td>b</td><td>c</td></tr></table>"
              f"{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td colspan='0'>a</td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td colspan='٣'>a</td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td colspan='1000'>a</td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td>a</td></tr><td>b</td></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><td>a</td><tr><td>b</td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td><div>in div</div> t</td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td><ul><li>x</li></ul></td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td><ul><li>x</li></ul></td></tr></table>{para(151)}</article>"),
         focus="recall")
    html(page(f'<article>{para(150)}<table><tr><td>a <a href="/x">link</a> b</td></tr></table>{para(151)}</article>'),
         links=True)
    html(page(f"<article>{para(150)}<table><tr><td>a<br>b</td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td>a<br><b>b</b></td></tr></table>{para(151)}</article>"),
         formatting=True)
    html(page(f"<article>{para(150)}<table><tr><td><p>one</p><p>two</p></td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td><blockquote>q</blockquote></td></tr></table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><thead><tr><th>h</th></tr></thead><tbody><tr><td>a</td></tr></tbody>"
              f"</table>{para(151)}</article>"))
    html(page(f"<article>{para(150)}<table><tr><td></td></tr></table>{para(151)}</article>"))
    # a link farm table is boilerplate
    farm = f"<table><tr><td>{links(160, 10, 30)}</td></tr></table>"
    html(page(f"<article>{para(150)}{farm}{para(151)}</article>"), links=True)
    html(page(f"<article>{para(150)}{farm}{para(151)}</article>"), links=True, tables=False)
    html(page(f"<article>{para(150)}{farm}{para(151)}</article>"), links=True, focus="precision")
    html(page(f"<article>{para(150)}{farm}{para(151)}</article>"), links=False)
    html(page(f"<article>{para(150)}<table><tr><td>{links(160, 2, 30)} {prose(161, 300)}</td></tr></table>"
              f"{para(151)}</article>"), links=True)
    html(page(f"<table><tr><td>{prose(162, 300)}</td></tr></table>"))
    html(page(f"<div>{grid}</div>"))


def list_pipeline_cases() -> None:
    html(page(f"<article>{para(170)}<ul><li>one</li><li>two</li></ul>{para(171)}</article>"))
    html(page(f"<article>{para(170)}<ol><li>one<ul><li>nested a</li><li>nested b</li></ul></li><li>two</li></ol>"
              f"{para(171)}</article>"))
    html(page(f"<article>{para(170)}<ul><li><p>one</p><p>more</p></li><li>two</li></ul>{para(171)}</article>"))
    html(page(f"<article>{para(170)}<ul><li><b>one</b> text</li><li>two</li></ul>{para(171)}</article>"),
         formatting=True)
    html(page(f'<article>{para(170)}<ul><li>one <a href="/x">link</a></li></ul>{para(171)}</article>'), links=True)
    html(page(f'<article>{para(170)}<ul><li><img src="a.png"> one</li></ul>{para(171)}</article>'), images=True)
    html(page(f"<article>{para(170)}<dl><dt>term</dt><dd>definition <b>b</b></dd></dl>{para(171)}</article>"))
    html(page(f"<article>{para(170)}<ul><li>one</li></ul>tail<ul><li>two</li></ul>{para(171)}</article>"))
    html(page(f'<article>{para(170)}<ul>{"".join(f"<li>{prose(172 + i, 40)}</li>" for i in range(6))}</ul>'
              f"{para(171)}</article>"))
    # a link-heavy list is boilerplate
    html(page(f'<article>{para(170)}<ul>{"".join(f"<li>{links(180 + i, 1, 30)}</li>" for i in range(6))}</ul>'
              f"{para(171)}</article>"), links=True)
    html(page(f'<article>{para(170)}<ul>{"".join(f"<li>{links(180 + i, 1, 30)}</li>" for i in range(6))}</ul>'
              f"{para(171)}</article>"), links=True, focus="precision")
    html(page(f"<article>{para(170)}<blockquote>{para(173, 80)}{para(174, 80)}</blockquote>{para(171)}</article>"))
    html(page(f"<article>{para(170)}<blockquote><ul><li>x</li></ul></blockquote>{para(171)}</article>"))
    html(page(f"<article>{para(170)}<pre>code line 1\ncode line 2</pre>{para(171)}</article>"))
    html(page(f'<article>{para(170)}<div class="highlight"><pre>highlighted code</pre></div>{para(171)}</article>'))
    html(page(f"<article>{para(170)}<pre><code>jscode()</code></pre>{para(171)}</article>"))
    html(page(f'<article>{para(170)}<pre class="x" lang="py">pycode()</pre>{para(171)}</article>'))
    html(page(f'<article>{para(170)}<p>a <code>inline</code> b</p>{para(171)}</article>'))
    html(page(f'<article>{para(170)}<h2>Heading <b>bold</b></h2>{para(171)}</article>'), formatting=True)
    html(page(f"<article>{para(170)}<h2><a href='/x'>Linked heading</a></h2>{para(171)}</article>"), links=True)


def prune_cases() -> None:
    body = para(190) + para(191)
    sidebar = f'<div class="sidebar"><p>{prose(192, 150)}</p></div>'
    # each prune branch inside a frame
    for focus in ("balanced", "precision", "recall"):
        html(page(f"<article>{body}{sidebar}</article>"), focus=focus)
        html(page(f'<article>{body}<div class="teaser"><p>{prose(193, 150)}</p></div></article>'), focus=focus)
        html(page(f"<article><header><p>{prose(194, 100)}</p></header>{body}</article>"), focus=focus)
        html(page(f'<article>{body}<div class="bottom"><p>{prose(195, 100)}</p></div></article>'), focus=focus)
        html(page(f'<article>{body}<div class="link"><p>{prose(195, 100)}</p></div></article>'), focus=focus)
        html(page(f'<article>{body}<div style="border: 1px solid"><p>{prose(195, 100)}</p></div></article>'),
             focus=focus)
        html(page(f'<article><div class="caption"><p>{prose(196, 100)}</p></div>{body}</article>'), focus=focus)
        html(page(f'<article>{body}<h2>Trailing</h2><h3>Titles</h3></article>'), focus=focus)
        html(page(f'<article>{body}<h2><a href="/x">Linked title</a></h2></article>'), focus=focus, links=True)
        html(page(f'<article>{body}<blockquote><a href="/x">{prose(197, 80)}</a></blockquote></article>'),
             focus=focus, links=True)
        html(page(f'<article>{body}<h2><a href="/x">Linked title</a></h2>{para(198)}</article>'), focus=focus,
             links=True)
    for images in (False, True):
        html(page(f'<article>{body}<div class="caption"><img src="a.png"></div></article>'), images=images)
        html(page(f'<article>{body}<div id="photo-caption"><p>{prose(199, 100)}</p></div></article>'), images=images)
    # link density: div, list, paragraph, repeated passes; refs are stripped unless links are kept
    farm = "".join(f'<li><a href="/l{i}">{prose(200 + i, 30)}</a></li>' for i in range(8))
    html(page(f"<article>{body}<ul>{farm}</ul></article>"), links=True)
    html(page(f"<article>{body}<ul>{farm}</ul></article>"), links=False)
    html(page(f'<article>{body}<div>{links(210, 8, 30)}</div></article>'), links=True)
    html(page(f'<article>{body}<p>{links(210, 3, 30)}</p></article>'), links=True)
    html(page(f'<article>{body}<div><div>{links(210, 8, 30)}</div></div></article>'), links=True)
    html(page(f'<article>{body}<div><div><p>{links(210, 4, 30)}</p></div></div></article>'), links=True)
    html(page(f'<article>{body}<p>text <a href="/x">{prose(211, 120)}</a></p></article>'), links=True)
    html(page(f'<article>{body}<p>text <a href="/x">{prose(211, 120)}</a></p></article>'), links=True,
         focus="precision")
    html(page(f"<article>{body}<p>a <span>kept span text</span> b</p></article>"))
    html(page(f"<article>{body}<p>a <a href='/x'>ref text</a> b</p></article>"), links=False)
    html(page(f"<article>{body}<p>a <a href='/x'>ref text</a> b</p></article>"), links=True)
    html(page(f'<article>{body}<div class="cookie-banner"><p>{prose(212, 100)}</p></div></article>'))
    html(page(f'<article>{body}<p class="hidden">{prose(213, 100)}</p></article>'))
    html(page(f'<article>{body}<div hidden aria-hidden="true"><p>{prose(214, 100)}</p></div></article>'))
    html(page(f'<article>{body}<div class="widget"><p>{prose(215, 100)}</p></div></article>'))
    # the frame itself is boilerplate-looking: the over-pruning guard of prune_unwanted_nodes
    html(page(f'<div class="entry-content sidebar">{body}</div>'))
    html(page(f'<article class="widget">{body}</article>'))
    html(page(f'<article class="nav">{body}</article><div>{para(216)}</div>'))
    # images
    img = '<img src="a.png" alt="alt text" title="the title">'
    html(page(f"<article>{body}<p>{prose(217, 50)} {img} tail</p></article>"), images=True)
    html(page(f"<article>{body}{img}</article>"), images=True)
    html(page(f"<article>{body}<div>{img}</div></article>"), images=True)
    html(page(f"<article>{body}<p>{img}</p></article>"), images=True)
    html(page(f'<article>{body}<picture><source srcset="a.webp"><img src="b.png"></picture></article>'),
         images=True)
    html(page(f'<article>{body}<img src="/rel/a.png"></article>'), images=True, url="https://example.org/a/b")
    html(page(f'<article>{body}<img src="//cdn.example/a.png"></article>'), images=True)
    html(page(f'<article>{body}<img src="//cdn.example/a.png"></article>'), images=True, url="https://example.org/")
    html(page(f'<article>{body}<img src="data:image/png;base64,AAAA"></article>'), images=True)
    html(page(f'<article>{body}<img data-src="https://x.test/a.png" src="b.png"></article>'), images=True)
    html(page(f'<article>{body}<img alt="no source"></article>'), images=True)
    html(page(f'<article>{body}<a href="/x"><img src="a.png"></a></article>'), images=True, links=True)
    html(page(f'<article>{body}<a href="/x"><img src="a.png"> text</a></article>'), images=True, links=True)
    html(page(f'<article>{body}<p><a href="/x"><img src="a.png"></a></p></article>'), images=True, links=True)


def comment_cases() -> None:
    comments = "comments_then_content"
    body = para(220) + para(221)
    post = f"<article>{body}</article>"
    one = "<p>One comment here</p><p>Second comment here</p>"
    # every COMMENTS_XPATH expression
    sections = [
        '<div id="commentlist">',
        '<div class="comment-page">',
        '<section class="comments-content">',
        '<div class="post-comments">',
        '<div id="comments">',
        '<section class="comments-area">',
        '<div class="Comments">',
        '<div class="article-comments">',
        '<div id="disqus_thread">',
        '<div id="dsq-comments">',
        '<div id="comol-1">',
        '<div id="social-box">',
        '<div class="user-comment">',
    ]
    for opening in sections:
        closing = "</section>" if opening.startswith("<section") else "</div>"
        html(page(f"{post}{opening}{one}{closing}"), op=comments)
    html(page(f'{post}<ul class="commentlist"><li>First comment text</li><li>Second comment text</li></ul>'),
         op=comments)
    html(page(f'{post}<ol id="comments"><li>First comment text</li><li>Second comment text</li></ol>'), op=comments)
    html(page(f'<article>{body}<div id="comments">{one}</div></article>'), op=comments)
    html(page(f'<div id="comments">{one}</div>{post}'), op=comments)
    html(page(f'<div id="comments">{one}</div><div id="comments">{one}</div>{post}'), op=comments)
    for focus in ("precision", "recall"):
        html(page(f'{post}<div id="comments">{one}</div>'), op=comments, focus=focus)
    html(page(f'{post}<div id="comments">{one}</div>'), op=comments, comments=False)
    # no section, an empty one, the next expression takes over, nothing survives
    html(page(post), op=comments)
    html(page(f'{post}<div id="comments"></div>'), op=comments)
    html(page(f'{post}<div id="comments"> </div>'), op=comments)
    html(page(f'{post}<div id="comments"><div class="cite"><cite>who</cite></div></div>'), op=comments)
    html(page(f'{post}<div id="comments"><img src="a.png"></div><div class="comment-body">{one}</div>'),
         op=comments)
    html(page(f'{post}<div id="commentlist"></div><div id="comments">{one}</div>'), op=comments)
    html(page(f'{post}<div id="comments"><span>only span</span></div>'), op=comments)
    # discards inside the section: respond, cite, quote, comments-title, hidden, replies
    html(page(f'{post}<div id="comments"><div id="respond"><p>Leave a reply</p></div>{one}</div>'), op=comments)
    html(page(f'{post}<div id="comments"><cite>author</cite>{one}</div>'), op=comments)
    html(page(f'{post}<div id="comments"><blockquote>old quote</blockquote>{one}</div>'), op=comments)
    html(page(f'{post}<div id="comments"><div class="comments-title"><p>3 comments</p></div>{one}</div>'),
         op=comments)
    html(page(f'{post}<div id="comments"><p style="display:none">hidden</p>{one}</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><div class="comment-reply-link"><p>Reply</p></div>{one}</div>'),
         op=comments)
    html(page(f'{post}<div id="comments"><div id="reply-title">Reply</div>{one}</div>'), op=comments)
    html(page(f'{post}<div id="comments"><div class="signin"><p>Sign in</p></div>{one}</div>'), op=comments)
    html(page(f'{post}<div id="comments"><p class="akismet">spam</p>{one}</div>'), op=comments)
    # ref and span are stripped before the walk, their text stays
    html(page(f'{post}<div id="comments"><p>By <a href="/u">user</a> said <span>this</span></p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p>By <a href="/u">user</a> said <span>this</span></p></div>'),
         op=comments, links=True)
    html(page(f'{post}<div id="comments"><span>span text</span><a href="/u">link</a><p>kept</p></div>'), op=comments)
    # elements walked: formatting, line breaks, nested divs, lists, a match moved out of an earlier match
    html(page(f'{post}<div id="comments"><p>a <b>bold</b> c</p><p>d<br>e</p></div>'), op=comments, formatting=True)
    html(page(f'{post}<div id="comments"><div><p>nested one</p><div><p>nested two</p></div></div></div>'),
         op=comments)
    html(page(f'{post}<div id="comments"><div>raw text in div<p>then para</p></div></div>'), op=comments)
    html(page(f'{post}<div id="comments"><ul><li>list comment</li></ul><p>para</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><blockquote><p>quoted</p></blockquote><pre>code</pre></div>'), op=comments)
    html(page(f'{post}<div id="comments"><h3>Heading</h3><p>text</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p>a<p>b</p></p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p>text one</p><br>Second line<p>third</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p>text one<br>Second line<br><br>third</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><br><p>text one</p><br></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p class="c" id="i" style="x">attrs</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p>one</p><p>one</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p> </p><p>text</p><p></p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p>Instagram</p><p>text</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><p><img src="a.png"></p><p>text</p></div>'), op=comments, images=True)
    html(page(f'{post}<div id="comments"><p>a <svg><title>t</title></svg> b</p></div>'), op=comments)
    html(page(f'{post}<div id="comments"><div id="comment-1">a one</div><div id="comment-2">b two</div></div>'),
         op=comments)
    html(page(f'{post}<div id="comments"><table><tr><td>cell</td></tr></table></div>'), op=comments)
    # the comments section sits inside the frame, and the deleted subtree takes its content out of the body
    html(page(f'<article>{body}<div id="comments">{one}</div>{para(222)}</article>'), op=comments)
    html(page(f'<div class="entry-content">{body}<div id="comments"><p>c</p></div></div>'), op=comments)
    html(page(f'<div id="comments">{post}</div>'), op=comments)
    html(page(f'<div id="comments"><article>{body}</article><p>comment text</p></div>'), op=comments)
    html(page(f'<div class="comments"><div id="comments"><p>outer and inner</p></div></div>{post}'), op=comments)
    html(page(f'<div class="comment-list"><div class="comment-list"><p>twice</p></div></div>{post}'), op=comments)
    # content after the comments
    html(page(f'<div id="comments">{one}</div>'), op=comments)
    html(page(f'<div id="comments">{one}</div><p>{prose(223, 100)}</p>'), op=comments)


def build() -> None:
    for builder in (table_cases, list_cases, quote_cases, paragraph_cases, title_cases, formatting_cases,
                    textelem_cases):
        builder()
    for builder in (frame_cases, single_run_cases, rebuilt_block_cases, dedup_cases, recovery_cases, document_text_cases,
                    foreign_cases, table_pipeline_cases, list_pipeline_cases, prune_cases, comment_cases):
        builder()


# ---------------------------------------------------------------- running upstream


class LineTrace:
    "The lines of one source file that run while the trace is on."

    def __init__(self, path: str) -> None:
        self.path = path
        self.hit: set[int] = set()

    def _local(self, frame, event, arg):
        if event == "line":
            self.hit.add(frame.f_lineno)
        return self._local

    def _global(self, frame, event, arg):
        return self._local if frame.f_code.co_filename == self.path else None

    def __enter__(self):
        sys.settrace(self._global)

    def __exit__(self, *exc):
        sys.settrace(None)


def make_options(options: Options):
    from trafilatura.settings import Extractor

    extractor = Extractor(
        fast=True, tables=options.tables, images=options.images, links=options.links,
        formatting=options.formatting, precision=options.focus == "precision", recall=options.focus == "recall",
        comments=options.comments, url=options.url,
    )
    assert extractor.focus == options.focus
    assert extractor.fast
    return extractor


def run_xml(item: XmlCase):
    "(dump of the result or None, dump of the snippet after the call)."
    from lxml import etree

    from parity import treedump
    from trafilatura import main_extractor

    root = etree.fromstring(item.xml)
    element = next(root.iter(item.target)) if item.target else root
    handler = getattr(main_extractor, item.handler)
    options = make_options(item.options)
    result = handler(element, set(item.ptags), options) if item.ptags is not None else handler(element, options)
    return (None if result is None else treedump.dump_tree(result)), treedump.dump_tree(root)


def run_html(item: HtmlCase):
    "(dump of the parsed snippet, comments dump, comments text, body dump, body text)."
    from parity import html5tree, treedump
    from trafilatura import htmlprocessing, main_extractor

    document = html5tree.parse_document(item.html)
    assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {item.html!r}"
    dump_in = treedump.dump_tree(document)
    options = make_options(item.options)
    tree = htmlprocessing.convert_tags(htmlprocessing.tree_cleaning(document, options), options)
    comments = comments_text = None
    if item.op == "comments_then_content":
        body, text, tree = main_extractor.extract_comments(tree, options)
        comments, comments_text = treedump.dump_tree(body), text
    body, text = main_extractor.extract_content(tree, options)
    return dump_in, comments, comments_text, treedump.dump_tree(body), text


def run_all(trace: LineTrace | None = None):
    "Results of every case, on upstream's code; a case that raises names itself."
    results: list[tuple] = []
    for kind, runner, cases in (("xml", run_xml, XML_CASES), ("html", run_html, HTML_CASES)):
        done: set = set()
        unique = []
        for item in cases:
            key = repr(item)
            if key not in done:
                done.add(key)
                unique.append(item)
        cases[:] = unique
        for item in unique:
            try:
                if trace is None:
                    results.append(runner(item))
                else:
                    with trace:
                        results.append(runner(item))
            except Exception as error:
                raise AssertionError(f"{kind} case raised {error!r}: {item!r}") from error
    return results


# ---------------------------------------------------------------- output


def rust_bool(value: bool) -> str:
    return "true" if value else "false"


def rust_opt(value: str | None) -> str:
    return "None" if value is None else f"Some({rust_str(value)})"


def rust_options(options: Options) -> str:
    return (
        f"CaseOptions {{ tables: {rust_bool(options.tables)}, images: {rust_bool(options.images)},"
        f" links: {rust_bool(options.links)}, formatting: {rust_bool(options.formatting)},"
        f" focus: {rust_str(options.focus)}, comments: {rust_bool(options.comments)}, url: {rust_opt(options.url)} }}"
    )


HEADER = """\
// @generated by tools/parity/parity/gen_main_extractor_cases.py. Do not edit. Regenerate from the repository root with:
//   scripts/oracle.sh 'from parity.gen_main_extractor_cases import main; main()' > tests/main_extractor_cases/cases.rs
// Upstream's main_extractor handlers and extractors on snippets: the trees and texts they leave.

use crate::trees::CaseOptions;

/// A handler run on an `etree.fromstring` snippet of converted tags.
pub struct XmlCase {
    pub xml: &'static str,
    /// `handle_table`, `handle_lists`, `handle_quotes`, `handle_paragraphs`, `handle_titles`,
    /// `handle_formatting` or `handle_textelem`.
    pub handler: &'static str,
    /// Tag of the first element in document order the handler runs on; empty for the root.
    pub target: &'static str,
    /// The `potential_tags` argument of the handlers that take one.
    pub potential_tags: Option<&'static [&'static str]>,
    pub options: CaseOptions,
    /// `treedump.dump_tree` of what the handler returns; `None` when it returns `None`.
    pub dump_result: Option<&'static str>,
    /// `treedump.dump_tree` of the whole snippet after the call.
    pub dump_source: &'static str,
}

/// An HTML snippet through `tree_cleaning` and `convert_tags`, then `op`.
pub struct HtmlCase {
    pub html: &'static str,
    /// `treedump.dump_tree` of the html5lib tree.
    pub dump_in: &'static str,
    /// `extract_content`, or `comments_then_content`: `extract_comments`, then `extract_content` on the tree it
    /// leaves.
    pub op: &'static str,
    pub options: CaseOptions,
    /// `treedump.dump_tree` of the comments body; `None` for `extract_content`.
    pub dump_comments: Option<&'static str>,
    pub comments_text: Option<&'static str>,
    /// `treedump.dump_tree` of the content body.
    pub dump_body: &'static str,
    pub body_text: &'static str,
}
"""


def main() -> None:
    build()
    results = run_all()
    xml_results, html_results = results[: len(XML_CASES)], results[len(XML_CASES):]
    out = [HEADER, "#[rustfmt::skip]", "pub const XML_CASES: &[XmlCase] = &["]
    for item, (dump_result, dump_source) in zip(XML_CASES, xml_results):
        ptags = "None" if item.ptags is None else "Some(&[" + ", ".join(rust_str(tag) for tag in item.ptags) + "])"
        out += [
            "    XmlCase {",
            f"        xml: {rust_str(item.xml)},",
            f"        handler: {rust_str(item.handler)},",
            f"        target: {rust_str(item.target)},",
            f"        potential_tags: {ptags},",
            f"        options: {rust_options(item.options)},",
            f"        dump_result: {rust_opt(dump_result)},",
            f"        dump_source: {rust_str(dump_source)},",
            "    },",
        ]
    out += ["];", "", "#[rustfmt::skip]", "pub const HTML_CASES: &[HtmlCase] = &["]
    for item, (dump_in, dump_comments, comments_text, dump_body, body_text) in zip(HTML_CASES, html_results):
        out += [
            "    HtmlCase {",
            f"        html: {rust_str(item.html)},",
            f"        dump_in: {rust_str(dump_in)},",
            f"        op: {rust_str(item.op)},",
            f"        options: {rust_options(item.options)},",
            f"        dump_comments: {rust_opt(dump_comments)},",
            f"        comments_text: {rust_opt(comments_text)},",
            f"        dump_body: {rust_str(dump_body)},",
            f"        body_text: {rust_str(body_text)},",
            "    },",
        ]
    out += ["];", ""]
    sys.stdout.write("\n".join(out))
    print(f"{len(XML_CASES)} xml cases, {len(HTML_CASES)} html cases", file=sys.stderr)


def report() -> None:
    "Prints the executable lines of upstream's main_extractor.py that no case reaches."
    from trafilatura import main_extractor

    path = main_extractor.__file__
    build()
    trace = LineTrace(path)
    run_all(trace)
    with open(path, encoding="utf-8") as handle:
        source = handle.read()
    lines = source.splitlines()
    executable: set[int] = set()
    pending = [code for code in compile(source, path, "exec").co_consts if hasattr(code, "co_lines")]
    while pending:  # function bodies only: module-level lines run at import, before the trace starts
        code = pending.pop()
        executable |= {line for _, _, line in code.co_lines() if line}
        pending += [const for const in code.co_consts if hasattr(const, "co_lines")]
    # a function's own `def` line is reported by the code object but runs at import
    executable = {line for line in executable if lines[line - 1].strip() and not lines[line - 1].lstrip().startswith("def ")}
    missed = sorted(executable - trace.hit)
    print(f"{len(XML_CASES)} xml cases, {len(HTML_CASES)} html cases; {len(missed)} of {len(executable)} lines missed")
    for line in missed:
        print(f"{line:4}: {lines[line - 1].rstrip()}")


if __name__ == "__main__":
    main()
