"""Generates tests/xml_output_cases/cases.rs: upstream's TXT, Markdown and HTML output on bodies and snippets.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_xml_output_cases import main; main()' > tests/xml_output_cases/cases.rs

Two kinds of case, each with three outputs: `xmltotxt(tree, False)` (TXT), `xmltotxt(tree, True)` (Markdown)
and the HTML output (`convert_to_html` on a deepcopy, then `tostring(pretty_print=True,
encoding="unicode")` stripped, which is what `build_html_output` returns without metadata). An output is
`Ok(text)`, or `Err("ValueError")` / `Err("KeyError")` for the exception upstream raises; the crate has one
error for both. Every `Extractor` is built with `fast=True` (deviation D-1).

`HtmlOutCase`: every `(html, op, options)` of `gen_main_extractor_cases.HTML_CASES` after its `build()`,
imported and not copied, plus the `summary` `rend` and `hi` `rend` cases (a `rend` Python `int` rejects, or one missing from
`HTML_TAG_MAPPING`, raises). The
snippet goes through `tree_cleaning`, `convert_tags` and `extract_comments` / `extract_content`; the outputs
are those of the content body and, for `comments_then_content`, of the comments body. The Rust test builds
the bodies with its own pipeline; `gen_main_extractor_cases` already holds the dump check of the parse.
A foreign element (`svg`, `math`) reaches the HTML output because the html5 tree keeps it. The oracle's html5
tree names it `{ns}local`, so lxml writes `ns0:title` under an `xmlns:ns0` declaration, where the crate and a
libxml2 tree write the local name (deviation D-2); the expected HTML is rewritten to local names
(`rewrite_namespaces`) and the generator prints how many cases it touched.

`XmlOutCase`: an `etree.fromstring` snippet, the root being the tree. The snippets are grouped by what they
cover (one function per group, below). `etree.fromstring` decodes `&#NN;` and normalizes whitespace in
attribute values, which the crate's test builder (`tests/trees`) does not, so `check_snippet` rejects a
snippet with `&#`, `\\r`, a tab or newline in an attribute value, or an `svg` or `math` element.
"""

import re
import sys
from copy import deepcopy
from xml.sax.saxutils import escape

from parity import gen_main_extractor_cases as main_cases
from parity.gen_main_extractor_cases import Options, page, para, prose, rust_options
from parity.gen_xpath_cases import rust_str

BS = "\\"
XML_CASES: list[str] = []
HTML_CASES: list[main_cases.HtmlCase] = []


def t(text: str) -> str:
    "Text content of a snippet."
    return escape(text)


def a(text: str) -> str:
    "Attribute value of a snippet, written in double quotes."
    return escape(text, {'"': "&quot;"})


def x(source: str) -> None:
    XML_CASES.append(source)


def html(source: str, op: str = "extract_content", **options) -> None:
    assert op in main_cases.OPS, op
    HTML_CASES.append(main_cases.HtmlCase(source, op, Options(**options)))


# ---------------------------------------------------------------- xml: inline specials, block starts


SPECIALS = "*_`~[]<"
RUNS = ("", BS, BS * 2, BS * 3)


def inline_special_cases() -> None:
    "Each inline special, with runs of backslashes before it, in every kind of text position."
    for ch in SPECIALS:
        for run in RUNS:
            text = t(run + ch)
            x(f"<doc><p>a {text} b</p></doc>")
            x(f"<doc><p>{text}</p></doc>")
            x(f"<doc><p>a<hi>b</hi>{text}</p></doc>")
        x(f"<doc><p>a{t(ch)}b{t(ch)}c</p><p>d</p></doc>")
        x(f'<doc><p>a <hi rend="#b">{t(ch)}x</hi> b</p></doc>')
        x(f'<doc><p>a <hi rend="#i">{t(BS + ch)}</hi></p></doc>')
        x(f"<doc><list><item>{t(ch)}</item></list></doc>")
        x(f"<doc><table><row><cell>{t(ch)}</cell></row></table></doc>")
        x(f'<doc><head rend="h2">{t(ch)}</head></doc>')
        x(f"<doc><p><code>a{t(ch)}b</code> tail {t(ch)}</p></doc>")
        x(f'<doc><p><hi rend="#t">{t(ch)}</hi>{t(ch)}</p></doc>')
        x(f"<doc><p><del>{t(ch)}x</del></p></doc>")
        x(f"<doc>{t(ch)}<p>b</p>{t(ch)}</doc>")
    x(f"<doc><p>{t(SPECIALS)}</p></doc>")
    x(f"<doc><p>{t(BS + '*' + BS * 2 + '_' + BS * 3 + '`' + BS + '~' + BS * 2 + '[' + BS + ']' + BS * 3 + '<')}</p></doc>")
    x(f"<doc><p>{t('**bold** _it_ ~~del~~ `code` [t](u) <b> <https://e.com>')}</p></doc>")
    x(f"<doc><p>{t('a_b_c snake_case_name *star* 2*3*4')}</p></doc>")


BLOCK_STARTS = [
    "# h", "#", "#\n", "#\nx", "###### h", "####### h", "   # h", "    # h", "#h", "#\tx", "## h\n", " #",
    "> q", ">", ">\n", ">\nx", "   > q", "    > q", " >q", "a > q",
    "- x", "-", "-\n", "-\nx", "+ x", "+", "+\n", "-x", "   - x", "    - x", "--", "- ", "-\tx", "* x",
    "1. x", "1) x", "1.", "1.\n", "1)\n", "1.\nx", "12. x", "123456789. x", "1234567890. x", "0. x", "1.x",
    "1 . x", "a. x", "1.\tx", "   1. x", "    1. x", "1.2 x",
    "\u0661. x", "\u0662\u0663. x", "\u0661\u0662\u0663\u0664\u0665\u0666\u0667\u0668\u0669. x",
    "\u0661\u0662\u0663\u0664\u0665\u0666\u0667\u0668\u0669\u0660. x", "\uff11. x", "\U0001d7cf. x",
    "\u0967. x", "\u0e51) x", "\u0661.\n", "\u0661.\nx", "\u00b2. x",
]


def block_start_cases() -> None:
    "Each block start, with `$` before a final newline, `Nd` digits, in the elements that escape it and in a cell."
    for text in BLOCK_STARTS:
        x(f"<doc><p>{t(text)}</p></doc>")
        x(f"<doc><quote>{t(text)}</quote></doc>")
        x(f"<doc><list><item>{t(text)}</item></list></doc>")
        x(f"<doc><table><row><cell>{t(text)}</cell></row></table></doc>")
        x(f"<doc><p>a</p><p>{t(text)}</p></doc>")
    for text in ("# x", "1. x", "- x", "> x", "#\n"):
        x(f"<doc><p>{t(text)}<hi rend=\"#b\">y</hi></p></doc>")
        x(f"<doc><p>   {t(text)}</p></doc>")
        x(f"<doc><p>    {t(text)}</p></doc>")
        x(f"<doc><p>a<lb/>{t(text)}</p></doc>")
        x(f'<doc><p><hi rend="#b">y</hi>{t(text)}</p></doc>')
        x(f"<doc><list><item>a</item><item>{t(text)}</item></list></doc>")
        x(f'<doc><list rend="ol"><item>{t(text)}</item></list></doc>')
        x(f'<doc><head rend="h2">{t(text)}</head></doc>')
        x(f"<doc>{t(text)}</doc>")


# ---------------------------------------------------------------- xml: code, math


CODE_TEXTS = [
    "x", "a`b", "a``b", "a```b", "`a", "a`", "`a`", "``", "`", " a ", "  ", "a\nb", "a`\nb", "```\n",
    "a\n```\nb", "a````b ```c", "``` ```", "x = `y`", "\na", "a\n", "\n", "a\n\nb", "  a\n  b", "``\n``",
]


def code_cases() -> None:
    "Code spans and fences with backtick runs, `lb` inside code, the `#t` highlight."
    for text in CODE_TEXTS:
        body = t(text)
        x(f"<doc><p>pre <code>{body}</code> post</p></doc>")
        x(f"<doc><code>{body}</code></doc>")
        x(f"<doc><code>{body}</code>tail</doc>")
        x(f'<doc><p>pre <hi rend="#t">{body}</hi> post</p></doc>')
        x(f'<doc><p><hi rend="#t">{body}</hi></p></doc>')
        x(f"<doc><list><item>a <code>{body}</code></item></list></doc>")
        x(f"<doc><table><row><cell><code>{body}</code></cell></row></table></doc>")
    for source in (
        "<doc><code>a<lb/>b</code></doc>",
        "<doc><code>a<lb/>b<lb/>c</code></doc>",
        "<doc><code>a<lb/></code></doc>",
        "<doc><code><lb/>b</code></doc>",
        "<doc><code><lb/></code></doc>",
        "<doc><code>a<lb/>b</code> tail</doc>",
        "<doc><p>x <code>a<lb/>b</code> y</p></doc>",
        "<doc><p><code>a<lb/>b</code></p></doc>",
        "<doc><code>a<lb/>`b`<lb/>```c</code></doc>",
        "<doc><code>a<hi>b</hi>c</code></doc>",
        '<doc><code>a<hi rend="#b">b</hi>c</code></doc>',
        '<doc><code>a<ref target="u">b</ref>c</code></doc>',
        "<doc><code>a<foo>b</foo>c</code></doc>",
        "<doc><code><hi>a</hi></code></doc>",
        "<doc><code><lb/><lb/></code>x</doc>",
        '<doc><p>x<hi rend="#t">a<lb/>b</hi>y</p></doc>',
        '<doc><p>x<hi rend="#t"><lb/></hi>y</p></doc>',
        '<doc><p>x<hi rend="#t"> `a` </hi>y</p></doc>',
        '<doc><p>x<hi rend="#t"></hi>y</p></doc>',
        '<doc><p>x<hi rend="#t"> </hi>y</p></doc>',
        '<doc><p>x<hi rend="#t">a<hi rend="#b">b</hi></hi>y</p></doc>',
        "<doc><code>  </code></doc>",
        "<doc><code></code></doc>",
    ):
        x(source)


MATH_TEXTS = [
    r"\[a\]", r"x \[a\] y", r"x\[a\]", r"\[a\]x", r" \[ a \] ", r"\[a", r"a\]", r"\[\]", r"\[ \]",
    "\\[\n a_1 \n\\]", "\\[a\n\\]", "\\[\\]", r"\(a\)", r"x\(a\)y", r"\(\)", r"\(a", "\\(a\nb\\)",
    r"\(a_1*b[x]<c`d~e\)", r"\[a_1*b[x]<c`d~e\]", r"\[ \(a\) \]", r"\(a \[b\] c\)", r"\(a\) \[b\]",
    r"\[a\] \(b\)", r"x\[a \[b\]", r"\[a \[b\] c\]", r"\(a \(b\) c\)", r"\[a\] and \[b\]", r"\(a\) and \(b\)",
    r"\\[a\\]", r"\\\[a\]", r"\\(a\\)", "a \\[b\\]\n\\[c\\]", " \\[a\\]", "\t\\[a\\]", "\u00a0\\[a\\]",
    "\u2003\\[a\\]", "\x85\\[a\\]", "\u2028\\[a\\]", "\u200b\\[a\\]", ".\\[a\\]", r"(\[a\])", r"[x] \[y\] [z]",
    r"\[x_1\] a_b", r"$a$", r"$$a$$", r"\[a\]\[b\]", r"\[a\]\(b\)", r"\(a\)\[b\]", r"\\[a\]", r"\[\[a\]\]",
    r"\(\(a\)\)", r"\[ \[a\] \]", r"\(a \[b \(c\) d\] e\)", r"a \[b\] c \(d\) e \[f", "\\(a \\[\nb\n\\] c\\)",
    r"\[a\] \(b \[c\] d\)", r"x\(a \[b\]", r"\[a\ b\]", r"\[ a * b \]", r"\[\n\]",
    r"a\)b *c*", r"x\] _y_", r"\\ a`b`", "plain words",
]


def math_cases() -> None:
    "Block and inline math, nested both ways, a lookbehind that skips an earlier candidate, in every kind of text position."
    for text in MATH_TEXTS:
        body = t(text)
        x(f"<doc><p>{body}</p></doc>")
        x(f'<doc><p>a<hi rend="#b">b</hi>{body}</p></doc>')
    for text in MATH_TEXTS[::3]:
        body = t(text)
        x(f'<doc><p>a <hi rend="#b">{body}</hi> b</p></doc>')
        x(f"<doc><head>{body}</head></doc>")
        x(f"<doc><list><item>{body}</item></list></doc>")
        x(f"<doc><table><row><cell>{body}</cell></row></table></doc>")
        x(f"<doc><quote>{body}</quote></doc>")
        x(f"<doc><p><code>{body}</code>{body}</p></doc>")
        x(f'<doc><p><hi rend="#t">{body}</hi></p></doc>')
        x(f'<doc><p>{body}<ref target="u">{body}</ref></p></doc>')
    # a block span inside an inline span keeps the inline delimiters; a lookbehind skips the first `\[`
    x(f"<doc><p>{t(chr(92) + '(a ' + chr(92) + '[b' + chr(92) + '] c' + chr(92) + ')')}</p></doc>")
    x(f"<doc><p>{t('x' + chr(92) + '[a ' + chr(92) + '[b' + chr(92) + ']')}</p></doc>")


# ---------------------------------------------------------------- xml: links and images


LINK_TARGETS = [
    "u", "http://e.com/a?b=1&c=2", "a b", "a<b", "a>b", "a(b)", "a(", "a)", "a\\b", "<a>", "a\\<b", "a\\\\b",
    "(", ")", "<", ">", " ", "a\\>b", "\\", "a b(c)<d>\\e", "/rel/path", "#frag", "mailto:a@b.c", "a\u00a0b",
    "\u00e9.html", "a&b",
]
LINK_TEXTS = [
    "link", " link ", "a[b]c", "a\\[b", "[a]", "[", "]", "x[1]", "`x[1]`", "`a]`", "a`[`b", "\\]", "a\\\\]",
    "**b**", "a_b", "a|b", "a\nb", "a<b>", "~~x~~", "`", "``a``", "\\`[a]`", "[a](b)", "![a](b)", "a\\",
    "\\[a\\]", "\\(a\\)",
]


def link_cases() -> None:
    "Link targets with and without ` <>()`, link texts with brackets and code, empty and missing targets."
    for target in LINK_TARGETS:
        x(f'<doc><p>see <ref target="{a(target)}">link</ref> end</p></doc>')
    for text in LINK_TEXTS:
        x(f'<doc><p>see <ref target="u">{t(text)}</ref> end</p></doc>')
        x(f'<doc><ref target="u">{t(text)}</ref></doc>')
        x(f'<doc><list><item><ref target="u">{t(text)}</ref></item></list></doc>')
        x(f'<doc><table><row><cell><ref target="u">{t(text)}</ref></cell></row></table></doc>')
    for source in (
        "<doc><p>a <ref>link</ref> b</p></doc>",
        '<doc><p>a <ref target="">link</ref> b</p></doc>',
        '<doc><p>a <ref target="u"></ref> b</p></doc>',
        '<doc><p>a <ref target="u"> </ref> b</p></doc>',
        '<doc><p>a <ref target="u"/> b</p></doc>',
        "<doc><p>a <ref/> b</p></doc>",
        '<doc><p>a <ref target="u"><hi rend="#b">bold</hi></ref> b</p></doc>',
        '<doc><p>a <ref target="u">x<hi rend="#b">bold</hi>y</ref> b</p></doc>',
        '<doc><p>a <hi rend="#b"><ref target="u">link</ref></hi> b</p></doc>',
        '<doc><p>a <ref target="u"><ref target="v">inner</ref></ref> b</p></doc>',
        '<doc><p>a <ref target="u">l<lb/>m</ref> b</p></doc>',
        '<doc><p>a <ref target="u"><graphic src="i.png" alt="x"/></ref> b</p></doc>',
        '<doc><p>a <ref target="u"> padded <hi rend="#i">text</hi> </ref> b</p></doc>',
        '<doc><p>a <ref target="u">link</ref>tail<ref target="v">two</ref></p></doc>',
        '<doc><p>a <ref target="u">link</ref></p></doc>',
        '<doc><p><ref target="u">link</ref></p></doc>',
        '<doc><head rend="h2">a <ref target="u">link</ref></head></doc>',
        '<doc><quote><ref target="u">link</ref></quote></doc>',
        '<doc><list><item>a <ref target="u">l</ref> b</item></list></doc>',
        '<doc><table><row><cell>a <ref target="u|v">l|m</ref> b</cell></row></table></doc>',
        '<doc><p>a <ref target="u" class="c" x="1">link</ref> b</p></doc>',
        '<doc><p>a <ref class="c" target="u" x="1">link</ref> b</p></doc>',
        '<doc><p>a <ref href="h" target="u">link</ref> b</p></doc>',
        '<doc><p>a <ref href="h">link</ref> b</p></doc>',
        '<doc><p>a <ref x="1" href="h" y="2" target="u">link</ref> b</p></doc>',
        '<doc><p>a <ref target="&quot;q&quot; &lt;&amp;&gt;">link</ref> b</p></doc>',
        '<doc><p>a <ref target="it&apos;s">link</ref> b</p></doc>',
    ):
        x(source)


IMAGES = [
    '<graphic src="a.png"/>', '<graphic src="a.png" alt="x"/>', '<graphic src="a.png" title="t"/>',
    '<graphic src="a.png" alt="x" title="t"/>', '<graphic src="a.png" alt="[x]*_`~&lt;"/>', '<graphic alt="x"/>',
    "<graphic/>", '<graphic src="a b.png" alt="x"/>', '<graphic src="a(1).png" alt="x"/>',
    '<graphic src="&lt;a&gt;.png" alt="x"/>', '<graphic src="a.png" alt=" x "/>', '<graphic src="a.png" alt="" title=""/>',
    '<graphic src="a\\b.png"/>', '<graphic src="a.png" title=" t " alt="x y"/>', '<graphic src="" alt="x"/>',
    '<graphic src="a.png" alt="x|y" title="p|q"/>', '<graphic src="a.png" alt="\\[x\\]"/>',
    '<graphic src="a.png" alt="a&quot;b" class="c" width="3"/>', '<graphic src="a.png" title="t"></graphic>',
    '<graphic src="a.png" alt="\u00e9\u2028"/>',
]


def image_cases() -> None:
    "Images with title and alt, in each position."
    for image in IMAGES:
        x(f"<doc><p>a {image} b</p></doc>")
        x(f"<doc><p>{image}</p></doc>")
        x(f"<doc><p>a {image}</p></doc>")
        x(f"<doc>{image}</doc>")
        x(f"<doc>{image} tail</doc>")
        x(f"<doc><p>x</p>{image}<p>y</p></doc>")
        x(f"<doc><list><item>a {image} b</item></list></doc>")
        x(f"<doc><list><item>{image}</item><item>b</item></list></doc>")
        x(f"<doc><table><row><cell>a {image} b</cell></row></table></doc>")
        x(f'<doc><p>a <hi rend="#b">{image}</hi> b</p></doc>')
    for source in (
        '<doc><p>a <graphic src="a.png"/><graphic src="b.png"/> b</p></doc>',
        '<doc><graphic src="a.png"/><graphic src="b.png"/></doc>',
        '<doc><graphic src="a.png"/>  tail  <graphic src="b.png"/></doc>',
        '<doc><head rend="h2">t <graphic src="a.png"/></head></doc>',
        '<doc><quote><graphic src="a.png" alt="x"/> q</quote></doc>',
        '<doc><graphic src="a.png"/>\n<p>x</p></doc>',
        '<doc><p>a<graphic src="a.png" alt="x"/>\n</p></doc>',
        '<doc><table><row><cell><graphic src="a.png"/> tail</cell></row></table></doc>',
        '<doc><list><item><graphic src="a.png"/> tail</item></list></doc>',
    ):
        x(source)


# ---------------------------------------------------------------- xml: lists, cells, structure


def list_cases() -> None:
    "Ordered and nested lists with tails: a list in a `p` indents its tail by the parent's marker, a bare item has no indent."
    for source in (
        "<doc><list><item>a</item><item>b</item></list></doc>",
        '<doc><list rend="ol"><item>a</item><item>b</item><item>c</item></list></doc>',
        '<doc><list rend="ul"><item>a</item></list></doc>',
        '<doc><list rend="OL"><item>a</item></list></doc>',
        '<doc><list rend="ol"><item>a<list><item>b</item><item>c</item></list></item><item>d</item></list></doc>',
        '<doc><list><item>a<list rend="ol"><item>b</item><item>c</item></list></item><item>d</item></list></doc>',
        '<doc><list rend="ol"><item>a<list rend="ol"><item>b<list rend="ol"><item>c</item></list></item></list></item></list></doc>',
        "<doc><list><item>a<list><item>b<list><item>c<list><item>d</item></list></item></list></item></list></item></list></doc>",
        '<doc><list rend="ol"><item>x</item><item>a<p>b<list><item>c</item></list> tail</p></item></list></doc>',
        "<doc><item>a</item><item>b</item></doc>",
        "<doc><list><item>a</item></list>tail</doc>",
        "<doc><list><item>a</item></list> tail <p>b</p></doc>",
        "<doc><list><item>a</item>tail<item>b</item></list></doc>",
        "<doc><list><item>a</item> <item>b</item></list></doc>",
        "<doc><list><item>a</item>\n  <item>b</item>\n</list></doc>",
        "<doc><list><item>a<list><item>b</item></list>tail</item><item>c</item></list></doc>",
        "<doc><list><item>a<list><item>b</item></list> tail</item></list></doc>",
        "<doc><list><item>a<list><item>b</item></list>tail<p>p</p></item></list></doc>",
        '<doc><list rend="ol"><item>a<list><item>b</item></list>tail</item></list></doc>',
        '<doc><list rend="ol"><item>a</item><item><list><item>b</item></list></item><item>c</item></list></doc>',
        "<doc><list><item><list><item>only nested</item></list></item></list></doc>",
        "<doc><list><item></item><item>b</item></list></doc>",
        "<doc><list><item> </item><item>b</item></list></doc>",
        '<doc><list rend="ol"><item></item><item>b</item></list></doc>',
        "<doc><list><item/></list></doc>",
        "<doc><list/></doc>",
        "<doc><list>intro<item>a</item></list></doc>",
        "<doc><list><item>a<lb/>b</item></list></doc>",
        "<doc><list><item><lb/>a</item></list></doc>",
        "<doc><list><item><lb/><lb/>a</item></list></doc>",
        "<doc><list><item>a<lb/></item><item>b</item></list></doc>",
        "<doc><list><item><lb/>a<lb/>b</item></list></doc>",
        "<doc><list><item> <lb/>a</item></list></doc>",
        "<doc><list><item><p>a</p><p>b</p></item></list></doc>",
        "<doc><list><item>a<p>b</p>c</item></list></doc>",
        "<doc><list><item><hi>a</hi> b</item></list></doc>",
        '<doc><list><item><hi rend="#b">a</hi><hi rend="#i">b</hi></item></list></doc>',
        "<doc><list><item><graphic src=\"a.png\"/></item></list></doc>",
        "<doc><list><item>a <hi><foo/></hi></item></list></doc>",
        "<doc><list><item><head>h</head>text</item></list></doc>",
        "<doc><list><item><quote>q</quote> t</item></list></doc>",
        "<doc><list><item><table><row><cell>c</cell></row></table></item></list></doc>",
        "<doc><list><item>a</item></list><list><item>b</item></list></doc>",
        '<doc><list rend="ol"><item>a</item></list><list rend="ol"><item>b</item></list></doc>',
        "<doc><p>a</p><list><item>b</item></list><p>c</p></doc>",
        "<doc><p>a<list><item>b</item></list>c</p></doc>",
        '<doc><p>a<list rend="ol"><item>b</item></list>c</p></doc>',
        "<doc><quote><list><item>a</item></list></quote></doc>",
        "<doc><head><list><item>a</item></list></head></doc>",
        "<doc><list><item><item>nested item</item></item></list></doc>",
        "<doc><list><foo><item>a</item></foo></list></doc>",
        "<doc><list><item>a</item><foo>x</foo><item>b</item></list></doc>",
        '<doc><list rend="ol"><item>a</item><foo/><item>b</item></list></doc>',
        '<doc><list rend="ol"><item>a</item><hi/><item>b</item></list></doc>',
        '<doc><list rend="ol"><item>a</item>t<item>b</item><lb/><item>c</item></list></doc>',
        '<doc><list rend="ol"><item>a</item></list><item>b</item></doc>',
        "<doc><list><item>a<list><item>b</item></list>\n</item></list></doc>",
        "<doc><list><item>a</item></list>\n<list><item>b</item></list>\n</doc>",
        # ol_twelve_items_tail_in_item_eleven: two-digit parent width for the sublist tail
        '<doc><list rend="ol">' + "".join(f"<item>i{n}</item>" for n in range(10)) + '<item>a<list><item>s</item></list> tail</item><item>z</item></list></doc>',
        '<doc><list rend="ol"><item>a</item><item><code>x<lb/>y</code></item><item>b</item></list></doc>',
        # nested ordered list in item 10
        '<doc><list rend="ol">' + "".join(f"<item>i{n}</item>" for n in range(9)) + '<item>j<list rend="ol"><item>k</item><item>l</item></list></item></list></doc>',
        '<doc><list rend="ol"><item>a</item>t<foo/><item>b</item><item>c<p>d<list><item>e</item></list> tail</p></item></list></doc>',
    ):
        x(source)
    for count in (1, 2, 3):
        items = "".join(f"<item>i{n}</item>" for n in range(count))
        for rend in ("", ' rend="ol"'):
            x(f"<doc><list{rend}>{items}</list></doc>")
            x(f"<doc><list{rend}>{items}</list>tail</doc>")
            x(f"<doc><list><item>x<list{rend}>{items}</list></item></list></doc>")
            x(f"<doc><list><item>x<list{rend}>{items}</list>tail</item></list></doc>")
            x(f"<doc><list><item>x<list{rend}>{items}</list> tail</item><item>y</item></list></doc>")


CELL_BODIES = [
    "a|b", "a\nb", "a | b", "|", "x<hi>y</hi>", '<head rend="h2">t</head>', "<head>t</head>",
    "<list><item>i</item><item>j</item></list>", '<list rend="ol"><item>i</item><item>j</item></list>',
    "<p>a</p><p>b</p>", "a<lb/>b", '<ref target="u|v">r|s</ref>', '<graphic src="a|b.png" alt="x|y"/>',
    "<quote>q</quote>", "<code>a|b</code>", "<code>a\nb</code>", '<hi rend="#b">a|b</hi>', " ", "",
    "a<p>b</p>tail", "<list><item>a<list><item>b</item></list></item></list>",
    "<table><row><cell>n</cell></row></table>", "<del>a</del>", "a<lb/>", "<lb/>a", "  a  b  ", "a\n\nb",
    "<p>a|b</p>", '<head rend="h3">a|b</head>x', "<head>a</head><head>b</head>", "<item>a</item><item>b</item>",
    "<list><item>a</item></list>tail", "t<list><item>a</item></list>", "<foo>x</foo>", "<hi><foo/></hi>",
    '<hi rend="#t">a|`b</hi>', "<code>a`b|c</code>", "a\\|b", "\\", "a<hi>b</hi> <hi>c</hi>",
]


def cell_cases() -> None:
    "Cells with pipes, newlines, lists, heads; the head separator row."
    for body in CELL_BODIES:
        x(f"<doc><table><row><cell>{body}</cell></row></table></doc>")
        x(f"<doc><table><row><cell>{body}</cell><cell>z</cell></row></table></doc>")
        x(f"<doc><table><row><cell>a</cell><cell>{body}</cell></row></table></doc>")
        x(f"<doc><table><row><cell>{body}</cell>tail<cell>z</cell></row>tail2</table></doc>")
    for source in (
        '<doc><table><row><cell role="head">h1</cell><cell role="head">h2</cell></row><row><cell>a</cell><cell>b</cell></row></table></doc>',
        '<doc><table><row><cell role="head">h1</cell><cell>h2</cell><cell role="head">h3</cell></row><row><cell>a</cell><cell>b</cell><cell>c</cell></row></table></doc>',
        '<doc><table><row><cell>a</cell><cell>b</cell></row><row><cell role="head">h</cell><cell role="head">i</cell></row></table></doc>',
        '<doc><table><row><cell role="head">h</cell></row><row><cell role="head">i</cell></row></table></doc>',
        '<doc><table><row><cell role="head"></cell><cell role="head"/></row></table></doc>',
        '<doc><table><row><cell role="head">a</cell></row></table></doc>',
        '<doc><table><row><cell role="x">a</cell><cell role="">b</cell></row></table></doc>',
        '<doc><table><row><cell role="head">a</cell><foo><cell role="head">b</cell></foo></row></table></doc>',
        '<doc><table><row><foo><cell role="head">b</cell></foo></row></table></doc>',
        '<doc><table><row><cell role="head">a</cell><cell>b</cell></row>tail<row><cell>c</cell></row></table>after</doc>',
        '<doc><row><cell role="head">a</cell><cell>b</cell></row></doc>',
        '<doc><row><cell>a</cell><cell>b</cell></row><row><cell>c</cell><cell>d</cell></row></doc>',
        "<doc><table><row><cell>a</cell><cell>b</cell></row><row><cell>c</cell><cell>d</cell></row></table></doc>",
        "<doc><table><row><cell>a</cell><cell>b</cell></row>\n<row><cell>c</cell><cell>d</cell></row>\n</table></doc>",
        "<doc><table>\n<row>\n<cell>a</cell>\n<cell>b</cell>\n</row>\n</table></doc>",
        "<doc><table><row><cell>a</cell></row></table>tail<p>b</p></doc>",
        "<doc><p>a</p><table><row><cell>a</cell></row></table><p>b</p></doc>",
        "<doc>x<table><row><cell>a</cell></row></table>y</doc>",
        "<doc><table/></doc>",
        "<doc><table><row/></table></doc>",
        "<doc><table><row/><row><cell>a</cell></row></table></doc>",
        "<doc><table><row><cell/><cell/></row></table></doc>",
        "<doc><table><cell>a</cell><cell>b</cell></table></doc>",
        "<doc><cell>a</cell><cell>b</cell></doc>",
        "<doc><p><cell>a</cell></p></doc>",
        '<doc><table><row><cell colspan="2" class="c">a</cell><cell rowspan="2">b</cell></row></table></doc>',
        "<doc><table><row><cell>a<table><row><cell>n1</cell><cell>n2</cell></row></table>b</cell></row></table></doc>",
        "<doc><table><row><cell><row><cell>x</cell></row></cell></row></table></doc>",
        "<doc><table><row><cell>a <hi>b</hi> c</cell><cell>d</cell></row></table></doc>",
        '<doc><table><row><cell><p>a</p><p>b</p></cell><cell><list><item>x</item><item>y</item></list></cell></row></table></doc>',
    ):
        x(source)


# ---------------------------------------------------------------- xml: empty pieces, emphasis, structure


def empty_piece_cases() -> None:
    "An empty piece reads as an empty last character, which separates like a newline."
    for source in (
        "<doc>text<hi><foo/></hi><p>b</p></doc>",
        "<doc>text<p>b</p></doc>",
        "<doc>text<hi></hi><p>b</p></doc>",
        '<doc>text<hi rend="#b"><foo/></hi><p>b</p></doc>',
        "<doc>a<hi><foo/></hi> <p>b</p></doc>",
        "<doc><hi><foo/></hi>text<hi><foo/></hi> more</doc>",
        "<doc><list><item><hi><foo/></hi>x</item></list></doc>",
        "<doc>text<ref><foo/></ref><p>b</p></doc>",
        "<doc>text<del><foo/></del><p>b</p></doc>",
        "<doc>text<head><foo/></head>x</doc>",
        "<doc>text<code><foo/></code><p>b</p></doc>",
        "<doc>text<hi><foo/></hi><head>b</head></doc>",
        "<doc>text<hi><foo/></hi><quote>b</quote></doc>",
        "<doc>text<hi><foo/></hi><list><item>b</item></list></doc>",
        "<doc>text<hi><foo/></hi><table><row><cell>b</cell></row></table></doc>",
        "<doc>text<hi><foo/></hi><lb/>b</doc>",
        '<doc>text<hi><foo/></hi><graphic src="a.png"/>b</doc>',
        "<doc>text <hi><foo/></hi><p>b</p></doc>",
        "<doc>text<hi><foo/></hi> <p>b</p></doc>",
        "<doc>text<hi><foo/></hi>\n<p>b</p></doc>",
        "<doc>text<hi><foo/></hi>|<p>b</p></doc>",
        "<doc><p>a</p><hi><foo/></hi><p>b</p></doc>",
        "<doc><p>a</p><hi><foo/></hi><hi><foo/></hi><p>b</p></doc>",
        "<doc><p>text<hi><foo/></hi></p><p>b</p></doc>",
        "<doc>x<foo/><p>b</p></doc>",
        "<doc>x<foo></foo><p>b</p></doc>",
        "<doc><p>a</p><foo/><p>b</p></doc>",
        "<doc><p>a</p><foo/>t<p>b</p></doc>",
    ):
        x(source)


COLLAPSE = [
    '<hi rend="#b"><hi rend="#b">x</hi></hi>',
    '<hi rend="#b"><hi rend="#i"><hi rend="#b">x</hi></hi></hi>',
    '<hi rend="#i"><hi rend="#b"><hi rend="#i">x</hi></hi></hi>',
    '<hi rend="#b"> <hi rend="#b">x</hi> </hi>',
    '<hi rend="#b">t<hi rend="#b">x</hi></hi>',
    '<hi rend="#b"><hi rend="#b">x</hi>t</hi>',
    '<hi rend="#b"><hi rend="#b">x</hi> </hi>',
    '<hi rend="#b"><hi rend="#b">x</hi><hi rend="#b">y</hi></hi>',
    '<hi rend="#b"><hi rend="#t">x</hi></hi>',
    '<hi rend="#t"><hi rend="#t">x</hi></hi>',
    '<hi rend="#u"><hi rend="#u">x</hi></hi>',
    "<hi><hi>x</hi></hi>",
    '<hi rend="#b"><hi rend="#b"><hi rend="#b">x<lb/>y</hi></hi></hi>',
    '<hi rend="#b"><hi rend="#b">a<ref target="u">r</ref></hi></hi>',
    '<hi rend="#b"><hi rend="#i"><hi rend="#b"><hi rend="#i">x</hi></hi></hi></hi>',
    '<hi rend="#b"><hi rend="#b"></hi></hi>',
    '<hi rend="#b"><hi rend="#b"/></hi>',
    '<hi rend="#b"><hi rend="#i"><hi rend="#i">x</hi></hi></hi>',
    '<hi rend="#b"><del><hi rend="#b">x</hi></del></hi>',
    '<hi rend="#sup"><hi rend="#sup">x</hi></hi>',
    '<hi rend="#b"><hi rend="#b">x</hi>\n</hi>',
    '<hi rend="#b"><hi rend="#b" class="c">x</hi></hi>',
    '<hi rend="#b">a<hi rend="#i">b<hi rend="#b">c</hi></hi></hi>',
    '<hi rend="#b"><hi rend="#i">a</hi><hi rend="#i">b</hi></hi>',
]
MERGE = [
    '<hi rend="#b">x</hi><hi rend="#b">y</hi>',
    '<hi rend="#b">x</hi><hi rend="#b">y</hi><hi rend="#b">z</hi>',
    '<hi rend="#i">x</hi><hi rend="#i">y</hi>',
    '<hi rend="#t">x</hi><hi rend="#t">y</hi>',
    '<hi rend="#u">x</hi><hi rend="#u">y</hi>',
    '<hi rend="#sub">x</hi><hi rend="#sub">y</hi>',
    '<hi rend="#sup">x</hi><hi rend="#sup">y</hi>',
    '<hi rend="#b">x</hi><hi rend="#i">y</hi>',
    '<hi rend="#b">x</hi> <hi rend="#b">y</hi>',
    '<hi rend="#b">x</hi>t<hi rend="#b">y</hi>',
    '<hi rend="#b">x</hi>\n<hi rend="#b">y</hi>',
    '<hi rend="#b">x<lb/>w</hi><hi rend="#b">y</hi>',
    '<hi rend="#b">x</hi><hi rend="#b">y<lb/>w</hi>',
    "<hi>x</hi><hi>y</hi>",
    '<hi rend="#b">x</hi><hi>y</hi>',
    '<hi rend="#b">x</hi><hi rend="#b">y</hi>tail',
    '<hi rend="#b">x</hi> <hi rend="#b">y</hi> tail',
    '<hi rend="#b"></hi><hi rend="#b">y</hi>',
    '<hi rend="#b">x</hi><hi rend="#b"></hi>',
    '<hi rend="#b">x</hi><hi rend="#b">y</hi><hi rend="#i">z</hi><hi rend="#i">w</hi>',
    '<hi rend="#b">x</hi> <hi rend="#b">y</hi> <hi rend="#b">z</hi>',
    '<hi rend="#b">x</hi><ref target="u">r</ref><hi rend="#b">y</hi>',
    '<hi rend="#t">a`</hi><hi rend="#t">`b</hi>',
    '<hi rend="#b">x</hi><hi rend="#b" class="c">y</hi>',
    '<hi rend="#b">x</hi><hi rend="#b">y</hi>t<hi rend="#b">z</hi><hi rend="#b">w</hi>',
    '<hi rend="#b">x</hi><hi rend="#b">y<lb/></hi><hi rend="#b">z</hi><hi rend="#b">w</hi>',
    '<hi rend="#b"/><hi rend="#b"/><hi rend="#b"/>',
    '<hi rend="#b">x</hi> <hi rend="#b"></hi> <hi rend="#b">z</hi> t',
    '<hi rend="#b">x</hi><hi rend="#b">y</hi><hi rend="#b">z</hi><hi rend="#i">w</hi><hi rend="#i">v</hi><hi rend="#i">u</hi>',
    '<hi rend="#u">a</hi>\n<hi rend="#u">b</hi>\n<hi rend="#u">c</hi>\n',
    '<hi rend="#t">a</hi><hi rend="#t">b</hi><hi rend="#t">c</hi><hi rend="#t">d</hi>',
]


def emphasis_cases() -> None:
    "Emphasis collapse and adjacent `hi` merge."
    for body in COLLAPSE + MERGE:
        x(f"<doc><p>{body}</p></doc>")
        x(f"<doc><p>a {body} b</p></doc>")
        x(f"<doc><list><item>{body}</item></list></doc>")
        x(f"<doc>{body}</doc>")
        x(f"<doc><head rend=\"h2\">{body}</head></doc>")
    # a `hi` with a rend that has a formatting marker, around other content
    for rend, marker in (("#b", "**"), ("#i", "*"), ("#t", "`"), ("#u", ""), ("#sup", ""), ("#sub", ""), ("", ""), ("zz", "")):
        for body in ("x", " x ", "  x", "x  ", "", " ", "a b", "x<lb/>y", "<foo>x</foo>", "x\ny", "*x*", "`x`", "~x~"):
            x(f'<doc><p>a <hi rend="{rend}">{body}</hi> b</p></doc>')
    for source in (
        '<doc><p><del>x</del> <del> y </del><del></del><del> </del></p></doc>',
        '<doc><p>a <del>b<hi rend="#b">c</hi></del> d</p></doc>',
        '<doc><p>~~x~~ <del>~y~</del></p></doc>',
    ):
        x(source)


ELEMENTS = [
    "<p>a</p>", '<p>a <hi rend="#b">b</hi> c</p>', '<head rend="h2">h</head>', "<quote>q</quote>",
    "<list><item>i</item><item>j</item></list>", '<list rend="ol"><item>i</item></list>',
    "<table><row><cell>c</cell></row></table>", "<lb/>", '<hi rend="#b">b</hi>', '<ref target="u">r</ref>',
    '<graphic src="a.png"/>', "<code>c</code>", "<del>d</del>", "<foo>f</foo>",
]
TAILS = ["", "tail", " tail ", " ", "\n", "  \n  x"]
CONTEXTS = [
    "<doc>{}</doc>",
    "<doc><list><item>{}</item></list></doc>",
    "<doc><table><row><cell>{}</cell></row></table></doc>",
    "<doc><p>{}</p></doc>",
    "<doc><quote>{}</quote></doc>",
    "<doc><head>{}</head></doc>",
]


def structure_cases() -> None:
    "Every pair of elements with a tail between them, at the top, in an item, a cell and a p; each element alone in all six."
    for i, first in enumerate(ELEMENTS):
        for j, second in enumerate(ELEMENTS):
            tail = TAILS[(i * 3 + j) % len(TAILS)]
            for context in CONTEXTS[:4]:
                x(context.format(f"{first}{t(tail)}{second}"))
    for first in ELEMENTS:
        for context in CONTEXTS:
            x(context.format(f"lead {first}"))
            x(context.format(f"{first}tail"))
            x(context.format(f"{first} "))
    for source in (
        "<doc>\n  <p>a</p>\n  <p>b</p>\n</doc>",
        "<doc>\n  <p>a</p>\n  <list>\n    <item>b</item>\n  </list>\n</doc>",
        "<doc><p>a</p><p>b</p></doc>",
        "<doc><p>a</p> <p>b</p></doc>",
        "<doc><p>a</p><p>b</p>tail</doc>",
        "<doc>lead<p>a</p></doc>",
        "<doc><p></p></doc>",
        "<doc><p/>x</doc>",
        "<doc> </doc>",
        "<doc>a<lb/>b<lb/>c</doc>",
        "<doc><p>a<lb/>b<lb/>c</p></doc>",
        "<doc><p>a<lb/> b</p></doc>",
        "<doc><p>a <lb/>b</p></doc>",
        "<doc><p><lb/>b</p></doc>",
        "<doc><p>a<lb/></p></doc>",
        "<doc><p>a<lb/><lb/>b</p></doc>",
        "<doc><lb/><p>b</p></doc>",
        "<doc><quote>a<lb/>b</quote></doc>",
        "<doc><quote><quote>nested</quote></quote></doc>",
        "<doc><quote><p>a</p><p>b</p></quote></doc>",
        "<doc><p>a</p><quote>b</quote>tail</doc>",
        "<doc><p><p>nested</p></p></doc>",
        "<doc><p>a<p>b</p>c</p></doc>",
        "<doc><head>a<lb/>b</head></doc>",
        '<doc><head rend="h1">a</head><head rend="h3">b</head><p>c</p></doc>',
        '<doc><head rend="h2">a <hi rend="#b">b</hi> c</head></doc>',
        '<doc><head rend="h2"><hi rend="#b">b</hi></head></doc>',
        '<doc><head rend="h2"><hi rend="#b">b</hi> tail</head></doc>',
        '<doc><head rend="h2"> <hi rend="#b">b</hi></head></doc>',
        '<doc><head rend="h2"><lb/>b</head></doc>',
        '<doc><head rend="h2"><graphic src="a.png"/></head></doc>',
        '<doc><head rend="h2"></head></doc>',
        '<doc><head rend="h2"> </head></doc>',
        '<doc><head rend="h2"><foo/></head></doc>',
        "<doc><head>a</head><head>b</head></doc>",
        '<doc><head rend="h2">a</head>tail</doc>',
        '<doc>x<head rend="h2">a</head>y</doc>',
        "<doc>x<foo>y</foo>z</doc>",
        "<doc>x<foo><p>y</p></foo>z</doc>",
        "<doc><foo>x</foo><foo>y</foo></doc>",
        "<doc><foo><foo>x</foo></foo>t</doc>",
        "<doc><done>x</done></doc>",
        "<doc><div><p>a</p></div></doc>",
        "<doc><p>a</p><div/>b</doc>",
        "<doc><p>  a  </p><p>  b  </p></doc>",
        "<doc><p> a <hi> b </hi> c </p></doc>",
        '<doc><p>a  <hi rend="#b">  b  </hi>  c</p></doc>',
        "<doc><p>a\nb</p></doc>",
        "<doc><p>a\n\n\nb</p></doc>",
        "<doc><p>\n\na\n\n</p></doc>",
        "<doc><p>a\tb</p></doc>",
        "<doc><p>a   b</p></doc>",
        "<doc><p>a \u00a0 b</p></doc>",
        "<doc><p>a\u2028b\u2029c</p></doc>",
        "<doc><p>a\u2424b</p></doc>",
        "<doc><p>\u2424</p></doc>",
        "<doc><p>a</p>\u2424<p>b</p></doc>",
        "<doc><p>x\u00e9\u4e2d\U0001f600y</p></doc>",
        "<doc><p>a\x85b</p></doc>",
        "<doc><p>a\u200bb\ufeffc</p></doc>",
    ):
        x(source)


# ---------------------------------------------------------------- xml: HTML conversions and pretty printing


HEAD_RENDS = [
    "h1", "h2", "h3", "h4", "h5", "h6", "h0", "h7", "h9", "h12", "h 2", "h-1", "h+2", "h-0", "h٣", "h1_0",
    "h٣٤", "h\u00851\u3000", "h 1 ", "h\u20031\u2003", "h\u00a01", "H2", "#3", "x3", "hx", "h", "", "x", "2",
    "h1 2", "h1__0", "h_1", "h1_", "h+", "h-", "h٣_٤", "h１", "h1.0", "h0x1", "h007", "h" + "0" * 20 + "1",
    "h\u00b21", "h\u00b2",
]
HI_RENDS = ["#i", "#b", "#u", "#t", "#sub", "#sup", "", "zz", "#x", "#I", "h2", "#b ", "#sup#sub", " #b"]


def conversion_cases() -> None:
    "Each `HTML_CONVERSIONS` entry, the attribute rules of `convert_to_html`, the heading levels."
    for rend in HEAD_RENDS:
        x(f'<doc><head rend="{a(rend)}">h</head></doc>')
        x(f'<doc><head rend="{a(rend)}">h<hi rend="#b">b</hi></head><p>x</p></doc>')
    for rend in HI_RENDS:
        x(f'<doc><p>a <hi rend="{a(rend)}">b</hi> c</p></doc>')
        x(f'<doc><hi rend="{a(rend)}">b</hi></doc>')
    for source in (
        "<doc><head>h</head></doc>",
        '<doc><head class="c" id="i">h</head></doc>',
        '<doc><head rend="h2" class="c">h</head></doc>',
        "<doc><p>a <hi>b</hi></p></doc>",
        '<doc><p>a <hi class="c" rend="#b" id="i">b</hi></p></doc>',
        "<doc><list><item>a</item></list></doc>",
        '<doc><list rend="ol" id="x"><item rend="y" class="c">a</item></list></doc>',
        '<doc><code lang="py" class="c">a</code></doc>',
        '<doc><quote lang="x" class="c">q</quote></doc>',
        "<doc>a<lb/>b</doc>",
        '<doc>a<lb class="c"/>b</doc>',
        '<doc><graphic src="a.png" alt="x" title="t" class="c" width="3"/></doc>',
        '<doc><graphic data-src="a.png"/></doc>',
        "<doc><graphic/></doc>",
        '<doc><ref target="u">r</ref></doc>',
        '<doc><ref target="u" class="c" x="1">r</ref></doc>',
        '<doc><ref class="c" target="u" x="1">r</ref></doc>',
        '<doc><ref href="h" target="u">r</ref></doc>',
        '<doc><ref href="h">r</ref></doc>',
        '<doc><ref x="1" href="h" y="2" target="u">r</ref></doc>',
        '<doc><ref class="c">r</ref></doc>',
        "<doc><ref>r</ref></doc>",
        '<doc><ref target="">r</ref></doc>',
        '<doc><table><row><cell>a</cell><cell role="head">b</cell></row></table></doc>',
        '<doc><table><row><cell role="other">a</cell><cell role="">b</cell><cell class="c">c</cell></row></table></doc>',
        '<doc><table class="t" id="i"><row class="r" id="j"><cell colspan="2" role="head" class="c">a</cell></row></table></doc>',
        '<doc><p class="c" id="i">x</p><foo x="1"><bar/></foo><div>y</div><del rend="x">d</del><table a="1"><row/></table></doc>',
        '<doc x="1" class="root"><p>a</p></doc>',
        '<doc><item>a</item><cell>b</cell><row>c</row><head>d</head></doc>',
        '<body><p>a</p></body>',
        '<head rend="h2">a</head>',
        '<list><item>a</item></list>',
        '<hi rend="#b">a</hi>',
        '<hi rend="zz">a</hi>',
        '<ref target="u">a</ref>',
        '<p>a</p>',
        '<graphic src="a.png"/>',
        '<lb/>',
        '<cell>a</cell>',
        '<doc><p>a</p><list><item><hi rend="#u">u</hi><hi rend="#t">t</hi><hi rend="#sub">s</hi><hi rend="#sup">p</hi></item></list></doc>',
        '<doc><hi rend="#b"><hi rend="zz">y</hi></hi></doc>',
        '<doc><head rend="hx"><hi rend="zz">y</hi></head></doc>',
        '<doc><head rend="h2"><hi rend="zz">y</hi></head></doc>',
        '<doc><head rend="hx"/><hi rend="zz"/></doc>',
        '<doc><hi rend="zz"/><head rend="hx"/></doc>',
    ):
        x(source)


def tostring_cases() -> None:
    "The pretty-print shapes (text nodes turn indentation off, the 60-space cap, `<x/>` against `<x></x>`) and the escapes."
    deep = "<div>" * 35 + "<p>x</p>" + "</div>" * 35
    deeper = "<div>" * 70 + "<p>x</p>" + "</div>" * 70
    for source in (
        "<body><div> <p>x</p></div></body>",
        "<doc><div>a<p>b</p></div><div><p>c</p></div></doc>",
        "<doc><div><p>x</p> </div></doc>",
        "<doc><div><p>x</p>t</div></doc>",
        "<doc><div> <p>x</p><p>y</p></div></doc>",
        "<doc><div><p>x</p><p>y</p></div> <div><p>z</p></div></doc>",
        "<doc><p/></doc>",
        "<doc><p></p></doc>",
        "<doc><p> </p></doc>",
        "<doc/>",
        "<doc></doc>",
        "<doc> </doc>",
        "<doc>t</doc>",
        "<doc><div/><div/></doc>",
        "<doc><div><div/></div></doc>",
        "<doc><div><div>x</div></div></doc>",
        "<doc><div><div>x</div> </div></doc>",
        "<doc><div><div><div>x</div></div>t</div></doc>",
        f"<doc>{deep}</doc>",
        f"<doc>{deeper}</doc>",
        f"<doc>t{deep}</doc>",
        f"<doc>{deep}t</doc>",
        f"<doc>{'<div>' * 33}t<div><p>x</p></div>{'</div>' * 33}</doc>",
        f"<doc>{'<div>' * 32}<div> <p>x</p></div>{'</div>' * 32}</doc>",
        f"<doc>{'<div>' * 31}<p>x</p><p>y</p>{'</div>' * 31}</doc>",
        "<doc>" + "".join(f"<div>{n}</div>" for n in range(12)) + "</doc>",
        "<doc>" + "".join(f"<p>{n}</p>" for n in range(12)) + "</doc>",
        "<doc><p>a&amp;b&lt;c&gt;d\"e'f</p></doc>",
        '<doc><p x="a&amp;b&lt;c&gt;d&quot;e\'f">y</p></doc>',
        '<doc><foo x="a&amp;b&lt;c&gt;d&quot;e&apos;f"/></doc>',
        "<doc><foo>&lt;/foo&gt;</foo></doc>",
        '<doc><foo x="&lt;&gt;"/><foo x="&amp;amp;"/><foo x="&amp;lt;"/></doc>',
        "<doc><p>&amp;amp;&amp;lt;&amp;#65;</p></doc>",
        "<doc><p>\u00e9\u4e2d\U0001f600\u00a0\u2028\x85</p></doc>",
        '<doc><foo x="\u00e9\u4e2d\U0001f600\u00a0\u2028\x85"/></doc>',
        '<doc><foo a="1" b="2" c="3"/></doc>',
        '<doc><foo b="2" a="1"/></doc>',
        '<doc><foo a=\'1"2\'/></doc>',
        '<doc><foo a="" b=" "/></doc>',
        "<doc><foo>a</foo><bar>b</bar></doc>",
        "<doc><foo><bar/></foo></doc>",
        "<doc><foo><bar>x</bar> <bar>y</bar></foo></doc>",
        "<doc>x<foo/>y<bar/>z</doc>",
        "<doc><foo>\n<bar>x</bar>\n</foo></doc>",
        "<doc>\n<foo>\n<bar>x</bar>\n</foo>\n</doc>",
        "<doc>\n  <foo>\n    <bar>x</bar>\n  </foo>\n</doc>",
        "<doc><p>a<foo/>b<foo/></p><foo/></doc>",
        "<doc><br/><br/>x</doc>",
        "<doc><lb/><lb/>x</doc>",
        "<doc><lb/><lb/></doc>",
        "<doc><lb/>x</doc>",
        "<doc><lb>x</lb></doc>",
        '<doc><graphic src="a.png"/><graphic src="b.png"/></doc>',
        '<doc><graphic src="a.png"/>t</doc>',
        "<doc><list><item/></list></doc>",
        "<doc><list><item>a</item><item/></list></doc>",
        "<doc><list><item><list><item><list><item>x</item></list></item></list></item></list></doc>",
        "<doc><table><row><cell/></row></table></doc>",
    ):
        x(source)


# ---------------------------------------------------------------- xml: html.unescape


UNESCAPE_TEXTS = [
    "&ampx &notit; &notin &amp &AMP; &#128; &#0; &#xD800; &#1; &#x110000;",
    "a &lt;b&gt; &amp; x\u2424y", "&", "&;", "&#;", "&#x;", "&#X41;", "&#65", "&#65;", "&#065;", "&#x41", "&#0000065;",
    "&hearts;", "&hearts", "&Hearts;", "&notit", "&noti", "&not;", "&not", "&nbsp;", "&nbsp", "&copy", "&copyx",
    "&amp;amp;", "&unknown;", "&unknown", "& amp;", "&#128512;", "&#x1F600;", "&#129;", "&#x81;", "&#150;", "&#x9F;",
    "&#xA0;", "&#13;", "&#10;", "&#9;", "&#9252;", "&#8232;", "&#x7F;", "&#xFFFE;", "&#xFFFF;", "&#x1FFFF;",
    "&#xFDD0;", "&#xFDEF;", "&#8;", "&#x0B;", "&#99999999999999999999;", "&#4294967361;", "&#x" + "0" * 5000 + "41;",
    "&#" + "1" * 4300 + ";", "&#" + "0" * 4298 + "65;", "&#x" + "F" * 40 + ";", "&#xD7FF;&#xE000;&#xDFFF;", "&#0x41;",
    "&lt", "&gt", "&quot", "&apos;", "&AMP", "&Amp;", "&amp&amp;", "&ampamp", "&amp;;", "&&amp;", "&#&#65;",
    "&#6&#5;", "&#65&#66;", "&#65 ;", "&# 65;", "&#-65;", "&#+65;", "&eacute;&Eacute;&eacutex", "&ETH;&ETHx",
    "&thorn&THORN", "&fjlig;", "&NotEqualTilde;", "&ThickSpace;", "&nbsp;&nbsp;", "a&amp;b&amp;c", "&it;",
    "&" + "a" * 40 + ";", "&" + "a" * 40, "&ampxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx;", "&notinx;",
    "&#x10FFFF;&#x110000;", "&#1114111;&#1114112;", "&#x1;&#x2;&#x8;&#xB;&#xE;&#x1F;&#x7F;&#x80;&#x9F;",
    "&#14;&#31;&#127;&#159;", "&#x0D;&#x0A;", "&#xD;", "&#x85;", "&#133;", "&#x2028;&#x2029;&#x2424;",
]


def unescape_cases() -> None:
    "Each `unescape` form short of the raise, and references that decode to separators."
    for text in UNESCAPE_TEXTS:
        x(f"<doc><p>{t(text)}</p></doc>")
    for text in UNESCAPE_TEXTS[:12]:
        x(f"<doc>{t(text)}</doc>")
        x(f'<doc><p>a <hi rend="#b">{t(text)}</hi> b</p></doc>')
        x(f"<doc><table><row><cell>{t(text)}</cell></row></table></doc>")
        x(f"<doc><list><item>{t(text)}</item></list></doc>")
    for source in (
        "<doc><p>a&amp;#13;b&amp;#10;c&amp;#9252;d</p></doc>",
        "<doc><p>a&amp;#8232;b</p></doc>",
        "<doc><p>a &amp;lt;b&amp;gt; &amp;amp; x\u2424y</p></doc>",
        "<doc><p>a\u2424b</p><p>c&amp;#9252;d</p></doc>",
        '<doc><ref target="a&amp;amp;b">l&amp;amp;m</ref></doc>',
        '<doc><graphic src="a&amp;amp;b.png" alt="&amp;amp;"/></doc>',
        '<doc><p>a <ref target="a&amp;amp;b">l&amp;amp;m</ref></p></doc>',
        '<doc><p>a <graphic src="a&amp;copy.png" alt="&amp;copy"/></p></doc>',
        "<doc><p><code>a&amp;lt;b</code></p></doc>",
        "<doc><p>&amp;#1" + "1" * 4300 + ";</p></doc>",
        "<doc><p>&amp;#" + "1" * 4301 + ";</p></doc>",
        "<doc><p>&amp;#" + "0" * 4300 + "1;</p></doc>",
    ):
        x(source)


def build_xml_cases() -> None:
    for builder in (inline_special_cases, block_start_cases, code_cases, math_cases, link_cases, image_cases,
                    list_cases, cell_cases, empty_piece_cases, emphasis_cases, structure_cases, conversion_cases,
                    tostring_cases, unescape_cases):
        builder()


# ---------------------------------------------------------------- html cases


def summary_page(rend: str) -> str:
    return page(f'<article>{para(300)}<details><summary rend="{rend}">Summary title</summary>'
                f"<p>{prose(301, 80)}</p></details>{para(302)}</article>")


def hi_page(inner: str) -> str:
    return page(f"<article>{para(310)}<p>{prose(311, 60)} {inner} end</p>{para(312)}</article>")


def build_html_cases() -> None:
    main_cases.build()
    HTML_CASES[:] = list(main_cases.HTML_CASES)
    for rend in ("hx", "h9", "h 2", ""):
        html(summary_page(rend))
        html(summary_page(rend), formatting=True)
    html(summary_page("h2"))
    html(page(f"<article>{para(300)}<details><summary>Summary title</summary><p>{prose(301, 80)}</p></details>"
              f"{para(302)}</article>"))
    for inner in ('<hi rend="zz">y</hi>', '<hi rend="#b">y</hi>', "<hi>y</hi>", '<hi rend="#sup">y</hi>'):
        html(hi_page(inner))
        html(hi_page(inner), formatting=True)
    html(hi_page('<hi rend="zz">y</hi>'), op="comments_then_content")
    html(page(f'{para(313)}<div id="comments"><p>a <hi rend="zz">y</hi> b</p><p>second comment</p></div>'),
         op="comments_then_content")
    html(page(f'{para(313)}<div id="comments"><details><summary rend="hx">t</summary><p>second comment</p></details></div>'),
         op="comments_then_content")


# ---------------------------------------------------------------- running upstream


NAMESPACE_DECLARATION = re.compile(r' xmlns:ns\d+="[^"]*"')
NAMESPACE_PREFIX = re.compile(r"<(/?)ns\d+:")
rewritten = 0


def rewrite_namespaces(output: str) -> str:
    "lxml's `ns0:` prefixes and `xmlns:ns0` declarations removed: the crate writes local names (D-2)."
    global rewritten
    if "xmlns:" not in output:
        return output
    rewritten += 1
    output = NAMESPACE_PREFIX.sub(r"<\1", NAMESPACE_DECLARATION.sub("", output))
    assert "xmlns" not in output and not re.search(r"</?ns\d+:", output), output
    return output


def attempt(compute) -> tuple[str, str]:
    "('Ok', value), or ('Err', name) for the exception upstream raises."
    try:
        return "Ok", compute()
    except ValueError:
        return "Err", "ValueError"
    except KeyError:
        return "Err", "KeyError"


def outputs(element) -> tuple[tuple[str, str], tuple[str, str], tuple[str, str]]:
    "TXT, Markdown and HTML output of one tree, as `build_html_output` and `xmltotxt` give them."
    from lxml import etree
    from trafilatura import htmlprocessing
    from trafilatura.xml import xmltotxt

    def html_output() -> str:
        converted = htmlprocessing.convert_to_html(deepcopy(element))
        return rewrite_namespaces(etree.tostring(converted, pretty_print=True, encoding="unicode").strip())

    return attempt(lambda: xmltotxt(element, False)), attempt(lambda: xmltotxt(element, True)), attempt(html_output)


def check_snippet(source: str) -> None:
    "The rules of the module docstring."
    from lxml import etree

    assert "&#" not in source and "\r" not in source, repr(source[:200])
    for tag in re.findall(r"<[A-Za-z][^<>]*>", source):
        for double, single in re.findall(r"""=\s*(?:"([^"]*)"|'([^']*)')""", tag):
            assert not re.search(r"[\t\n\r]", double + single), repr(tag)
    root = etree.fromstring(source)
    assert not any(el.tag in ("svg", "math") for el in root.iter()), repr(source[:200])


def run_xml(source: str):
    from lxml import etree

    return outputs(etree.fromstring(source))


def run_html(item: main_cases.HtmlCase):
    "Outputs of the comments body (None for `extract_content`) and of the content body."
    from parity import html5tree
    from trafilatura import htmlprocessing, main_extractor

    document = html5tree.parse_document(item.html)
    assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {item.html!r}"
    options = main_cases.make_options(item.options)
    tree = htmlprocessing.convert_tags(htmlprocessing.tree_cleaning(document, options), options)
    comments = None
    if item.op == "comments_then_content":
        body, _, tree = main_extractor.extract_comments(tree, options)
        comments = outputs(body)
    body, _ = main_extractor.extract_content(tree, options)
    return comments, outputs(body)


def unique(items: list) -> list:
    seen: set = set()
    kept = []
    for item in items:
        key = repr(item)
        if key not in seen:
            seen.add(key)
            kept.append(item)
    return kept


# ---------------------------------------------------------------- output


def rust_result(result: tuple[str, str]) -> str:
    kind, value = result
    return f"{kind}({rust_str(value)})"


def rust_triple(triple) -> str:
    return "(" + ", ".join(rust_result(result) for result in triple) + ")"


HEADER = """\
// @generated by tools/parity/parity/gen_xml_output_cases.py. Do not edit. Regenerate from the repository root with:
//   scripts/oracle.sh 'from parity.gen_xml_output_cases import main; main()' > tests/xml_output_cases/cases.rs
// Upstream's TXT, Markdown and HTML output on extracted bodies and on etree.fromstring snippets.

use crate::trees::CaseOptions;

/// An expected output: the text, or the exception upstream raises (`ValueError` or `KeyError`).
pub type Output = Result<&'static str, &'static str>;

/// An HTML snippet through `tree_cleaning`, `convert_tags` and `op`, then the three outputs of the bodies.
pub struct HtmlOutCase {
    pub html: &'static str,
    /// `extract_content`, or `comments_then_content`: `extract_comments`, then `extract_content` on the tree it
    /// leaves.
    pub op: &'static str,
    pub options: CaseOptions,
    /// `xmltotxt(body, False)` of the content body.
    pub body_txt: Output,
    /// `xmltotxt(body, True)`.
    pub body_md: Output,
    /// `convert_to_html` on a copy, `tostring(pretty_print=True)` stripped; lxml's `ns0:` prefixes and
    /// `xmlns:ns0` declarations removed (foreign elements are written under their local name, D-2).
    pub body_html: Output,
    /// The same three outputs of the comments body; `None` for `extract_content`.
    pub comments: Option<(Output, Output, Output)>,
}

/// An `etree.fromstring` snippet, the root being the tree, and its three outputs.
pub struct XmlOutCase {
    pub xml: &'static str,
    pub txt: Output,
    pub md: Output,
    pub html: Output,
}
"""


def main() -> None:
    build_html_cases()
    build_xml_cases()
    for source in XML_CASES:
        check_snippet(source)
    html_cases = unique(HTML_CASES)
    xml_cases = unique(XML_CASES)
    html_results = []
    html_rewritten = 0
    for item in html_cases:
        before = rewritten
        try:
            html_results.append(run_html(item))
        except Exception as error:
            raise AssertionError(f"html case raised {error!r}: {item!r}") from error
        html_rewritten += rewritten > before
    xml_results = []
    for source in xml_cases:
        try:
            xml_results.append(run_xml(source))
        except Exception as error:
            raise AssertionError(f"xml case raised {error!r}: {source!r}") from error
    check_invalid_rend_cases(html_cases, html_results)

    out = [HEADER, "#[rustfmt::skip]", "pub const HTML_CASES: &[HtmlOutCase] = &["]
    for item, (comments, body) in zip(html_cases, html_results):
        txt, md, html_ = body
        out += [
            "    HtmlOutCase {",
            f"        html: {rust_str(item.html)},",
            f"        op: {rust_str(item.op)},",
            f"        options: {rust_options(item.options)},",
            f"        body_txt: {rust_result(txt)},",
            f"        body_md: {rust_result(md)},",
            f"        body_html: {rust_result(html_)},",
            f"        comments: {'None' if comments is None else 'Some(' + rust_triple(comments) + ')'},",
            "    },",
        ]
    out += ["];", "", "#[rustfmt::skip]", "pub const XML_CASES: &[XmlOutCase] = &["]
    for source, (txt, md, html_) in zip(xml_cases, xml_results):
        out += [
            "    XmlOutCase {",
            f"        xml: {rust_str(source)},",
            f"        txt: {rust_result(txt)},",
            f"        md: {rust_result(md)},",
            f"        html: {rust_result(html_)},",
            "    },",
        ]
    out += ["];", ""]
    sys.stdout.write("\n".join(out))
    errors: dict[str, int] = {}
    triples = [body for _, body in html_results] + [c for c, _ in html_results if c is not None] + xml_results
    for triple in triples:
        for kind, value in triple:
            if kind == "Err":
                errors[value] = errors.get(value, 0) + 1
    print(f"{len(html_cases)} html cases, {len(xml_cases)} xml cases; {html_rewritten} html cases with xmlns "
          f"rewriting ({rewritten} outputs rewritten in all); errors {errors}", file=sys.stderr)


def check_invalid_rend_cases(cases: list, results: list) -> None:
    "The `summary` and `hi` `rend` cases give the output or exception they pin; a snippet that does not is a generator bug."
    reached = {"hx": False, "h9": False, "h 2": False, "": False, "zz": False}
    for item, (comments, body) in zip(cases, results):
        for rend in reached:
            if f'<summary rend="{rend}">' in item.html and item.op == "extract_content":
                html_ = body[2]
                if rend in ("hx", ""):
                    assert html_ == ("Err", "ValueError"), (rend, html_)
                elif rend == "h9":
                    assert html_[0] == "Ok" and "<h9>" in html_[1], html_
                else:
                    assert html_[0] == "Ok" and "<h2>" in html_[1] and "h 2" not in html_[1], html_
                reached[rend] = True
        if '<hi rend="zz">' in item.html and item.op == "extract_content":
            assert body[2] == ("Err", "KeyError"), body
            reached["zz"] = True
    assert all(reached.values()), reached


if __name__ == "__main__":
    main()
