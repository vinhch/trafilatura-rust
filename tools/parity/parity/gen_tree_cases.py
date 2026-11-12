"""Generates src/dom/parse/tree_cases.rs: the tree `lxml.html.document_fromstring` builds on fixed snippets, as
libxml2 2.14.6 parses them with trafilatura's HTML_PARSER, read by the libxml2 tree builder's rule tests.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_tree_cases import main; main()' > src/dom/parse/tree_cases.rs

Each case holds the treedump (`parity.treedump.dump_tree`) of the root, the treedump of every other element on
the document node (libxml2 hangs a second implied `html` there after the first one closes), and `"!empty"` where
lxml raises "Document is empty".
"""

import sys

import lxml.etree
from lxml.html import document_fromstring

from parity.gen_xpath_cases import rust_str
from parity.treedump import dump_tree

LIBXML2_VERSION = (2, 14, 6)

# The 38 snippets of .work/spike-parsers/cases/, names kept.
SPIKE_CASES = [
    ("01-p-closed-by-li", "<ul><li><p>a<li>b</ul>"),
    ("02-p-closed-by-dd-dt", "<dl><dt><p>t<dd><p>d<dt>t2</dl>"),
    ("03-p-closed-by-td-th", "<table><tr><td><p>a<td><p>b<th><p>c</table>"),
    ("04-p-closed-by-tr-table", "<table><tr><td><p>a<tr><td>b</table><p>x<table><tr><td>y</table>"),
    ("05-p-closed-by-ul-div-h1", "<p>a<ul><li>x</ul><p>b<div>c</div><p>d<h1>e</h1>"),
    ("06-p-not-closed-by-section-article-nav", "<p>a<section>b</section><p>c<article>d</article><p>e<nav>f</nav>"),
    ("07-div-end-while-td-open", "<table><tr><td><div>a</td><td>b</table>"),
    ("08-div-end-with-td-open-no-div", "<table><tr><td>a</div>b</td></tr></table>"),
    ("09-p-end-without-open-p", "<div>a</p>b</div>"),
    ("10-li-li", "<ul><li>a<li>b<li>c</ul>"),
    ("11-dt-dd", "<dl><dt>a<dd>b<dt>c<dd>d</dl>"),
    ("12-tr-td-th", "<table><tr><td>a<th>b<tr><th>c<td>d</table>"),
    ("13-thead-tbody-tfoot", "<table><thead><tr><td>h<tbody><tr><td>b<tfoot><tr><td>f</table>"),
    ("14-table-no-tbody", "<table><tr><td>a</td></tr></table>"),
    ("15-misnested-b-i", "<p><b>x<i>y</b>z</i>w</p>"),
    ("16-script-raw-text", '<script>var a = "<b>x</b></div>"; if (1<2) {}</script><p>after'),
    ("17-style-raw-text", '<style>a > b { content: "</div><b>"; }</style><p>after'),
    ("18-textarea-title-rcdata", "<title>a <b>b</b> &amp; c</title><textarea><b>x</b> &lt; y</textarea>"),
    ("19-named-refs-no-semicolon-text", "<p>&copy 2024 &lang &amp &notit &ampx &unknown; &nbsp x</p>"),
    ("20-named-refs-attrs", '<a href="?a=1&copy=2&lang=en&amp=3&unknown=4">x</a>'),
    ("21-numeric-refs", "<p>&#128; &#x80; &#0; &#xD800; &#x10FFFF; &#65 &#x41</p>"),
    ("22-bogus-comment-decl", "<p>a<!foo>b</p><p>c</ 3>d</p>"),
    ("23-leading-text-implied", "hello <b>world</b>"),
    ("24-title-meta-after-body", "<p>text</p><title>t</title><meta name=\"a\" content=\"b\"><p>more"),
    ("25-frameset", '<html><head><title>t</title></head><frameset cols="50%,50%"><frame src="a"><frame src="b"></frameset></html>'),
    ("26-unknown-tags", '<foo-bar><baz>x</baz></foo-bar><custom:tag a="1">y</custom:tag>'),
    ("27-uppercase", '<DIV CLASS="A" ID=B><P>Text</P></DIV>'),
    ("28-duplicate-attrs", '<div a="1" a="2" b=x B=y>t</div>'),
    ("29-attr-without-value", "<input disabled checked value><div hidden data-x>t</div>"),
    ("30-unclosed-final", "<div><p>a<span>b"),
    ("31-nested-forms", "<form id=a><form id=b><input></form></form>"),
    ("32-select-option", "<select><option>a<option selected>b<optgroup label=g><option>c</select>"),
    ("33-nested-a", "<a href=1>x<a href=2>y</a>z</a>"),
    ("34-svg-math", '<svg viewBox="0 0 1 1"><path d="M0"/></svg><math><mi>x</mi></math>'),
    ("35-crlf", "<p>a\r\nb\rc</p>"),
    ("36-script-lt-letter", '<script>for(var t=0;t<e.length;t++){a=1}</script><p>after'),
    ("37-noscript-img", "<head><noscript><img src=a></noscript></head><body><noscript><p>x</p></noscript></body>"),
    ("38-html-duplicate-lang", '<html lang="de-DE" lang="de"><body><p>a</p></body></html>'),
]

# Cases for the rules the spike does not reach, in the order of the plan.
EXTRA_CASES = [
    ("ws-before-head", "<html>\n<head><title>t</title></head><body><p>a</p></body></html>"),
    ("ws-in-head", "<html><head>\n<title>t</title>\n<meta charset=utf-8>\n</head><body><p>a</p></body></html>"),
    ("ws-after-head", "<html><head><title>t</title></head>\n<body><p>a</p></body></html>"),
    ("ws-after-body", "<html><head></head><body><p>a</p></body>\n</html>"),
    ("ws-after-html", "<html><head></head><body><p>a</p></body></html>\n"),
    ("element-after-body", "<p>a</p></body><p>b</p>"),
    ("element-after-html", "<p>a</p></html><p>b</p>"),
    ("body-after-body", "<body><p>a</p></body><body><p>b</p></body>"),
    ("body-end-in-div", "<body><div>a</body>b"),
    ("self-closing-div-text", '<div class="x"/>text'),
    ("misplaced-body-self-closing", '<p><body a="1"/>x'),
    ("misplaced-html-attributes", '<html lang="de"><body><p>a</p><html a=1><p>b</p></html>'),
    ("misplaced-head", "<body><p>a</p><head><title>t</title></head>"),
    ("misplaced-body-attributes", '<html><head></head><body b=2><p>a</p><body b=3 c=4>x</body>'),
    ("cut-start-tag-attribute", '<html><head><title>x</title></head><div class="a'),
    ("cut-start-tag-name", "<html><head><title>x</title></head><div"),
    ("text-after-html-end", "<p>a</p></html>tail text"),
    ("html-tag-after-html-end", '<p>a</p></html><html lang="de"><p>b</p>'),
    ("long-tag-and-attribute-names", "<" + "t" * 120 + " " + "b" * 120 + '="v">x</' + "t" * 120 + ">"),
    ("end-tag-not-open", "<div></span>x</div>"),
    ("end-p-not-open", "<p>a</p></p>b"),
    ("end-br", "<p>a</br>b</p>"),
    ("void-img", "<p>a<img src=a>b</p>"),
    ("source-track-not-void", "<video><source src=a><p>x</video>"),
    ("svg-path", '<svg viewBox="0 0 1 1"><path d="M0"/></svg>'),
    ("math-mi", "<math><mi>x</mi></math>"),
    ("control-characters", "<p>a\x01b\x00c￿d</p>"),
    ("attribute-name-not-xml", '<p a"b=1>x</p>'),
    ("table-unclosed-td", "<table><tr><td>a</table>"),
    ("nested-tables", "<table><tr><td><table><tr><td>b</table>c</td></tr></table>"),
    ("table-caption-colgroup", "<table><caption>c</caption><colgroup><col></colgroup><tr><td>x</table>"),
    ("head-noscript-img", "<head><noscript><img src=a></noscript></head>"),
    ("frameset-frames", "<frameset><frame><frame></frameset>"),
    ("leading-text", "Hello <p>a</p>"),
    ("leading-bom", "﻿<p>a</p>"),
    ("xml-declaration-prefix", '<?xml version="1.0"?><p>a</p>'),
    ("doctype-leading-whitespace", " \n<!doctype html><p>a</p>"),
    ("doctype-alone", "<!DOCTYPE html>"),
    ("comment-alone", "<!--c-->"),
]


def dump_case(html: str, parser) -> tuple[str, list[str]]:
    "The treedump of the root and of the other document-level elements, or `!empty`."
    try:
        root = document_fromstring(html, parser=parser)
    except lxml.etree.ParserError as err:
        if str(err) != "Document is empty":
            raise
        return "!empty", []
    others = [dump_tree(el) for el in root.itersiblings() if isinstance(el.tag, str)]
    return dump_tree(root), others


def main() -> None:
    from trafilatura.utils import HTML_PARSER
    from importlib.metadata import version as package_version

    if lxml.etree.LIBXML_VERSION != LIBXML2_VERSION:
        raise RuntimeError(f"libxml2 {lxml.etree.LIBXML_VERSION}, expected {LIBXML2_VERSION}")
    rows = []
    for name, html in SPIKE_CASES + EXTRA_CASES:
        document, others = dump_case(html, HTML_PARSER)
        rows.append(
            f"    TreeCase {{ name: {rust_str(name)}, html: {rust_str(html)}, "
            f"document: {rust_str(document)}, others: &[{', '.join(rust_str(o) for o in others)}] }},"
        )
    print(
        f"// @generated by tools/parity/parity/gen_tree_cases.py with Python {sys.version.split()[0]}, "
        f"lxml {package_version('lxml')}, libxml2 {'.'.join(map(str, LIBXML2_VERSION))}."
    )
    print("// Do not edit. Regenerate from the repository root with:")
    print("//   scripts/oracle.sh 'from parity.gen_tree_cases import main; main()' > src/dom/parse/tree_cases.rs")
    print("// Each document is the treedump of lxml.html.document_fromstring(html, parser=HTML_PARSER).")
    print()
    print("/// One snippet and the tree lxml builds from it.")
    print("pub(super) struct TreeCase {")
    print("    pub(super) name: &'static str,")
    print("    pub(super) html: &'static str,")
    print("    /// Treedump of the root, or `!empty` where lxml raises \"Document is empty\".")
    print("    pub(super) document: &'static str,")
    print("    /// Treedumps of the other elements on the document node, in order.")
    print("    pub(super) others: &'static [&'static str],")
    print("}")
    print()
    print("#[rustfmt::skip]")
    print("pub(super) const TREE_CASES: &[TreeCase] = &[")
    for row in rows:
        print(row)
    print("];")


if __name__ == "__main__":
    main()
