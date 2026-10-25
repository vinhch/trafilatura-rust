"""Generates tests/extraction_cases/cases.rs: upstream's `extract`, `baseline` and `html2txt` on whole pages.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_extraction_cases import main; main()' > tests/extraction_cases/cases.rs

Everything runs inside `parity.generate.html5_parsers()`, which replaces `utils.fromstring` and
`baseline.fragment_fromstring` with html5lib parses, so the expected values come from the same kind of tree
as the Rust side (D-2). Every `Extractor` is built with `fast=True` (D-1) and `dedup=False`. Every page is a
full `<html>` document, except the empty load `""`: a fragment such as `"123"` is rejected by the length
check the crate emulates (`lxml_len`) and is a unit test with the plain-lxml value.

`ExtractCase`: a page, an output format (`txt`, `markdown`, `html`), the options and what `extract` returns:
`Ok(Some(text))`, `Ok(None)` for upstream's `None`, `Err(name)` for the exception class that escapes. Pages:

    snippets   every html snippet of `gen_main_extractor_cases.HTML_CASES` after `build()` (imported, not
               copied), once per distinct (page, options); the format and options sets are `format_sets`
    targeted   pages for the branches of `trafilatura_sequence`, each checked while generating against a
               trace of the cascade (`Trace`); a page that misses its branch fails the generation
    baseline   the pages of `BaselineCase`, through `txt` and `markdown`, so the baseline rescue reads them

`BaselineCase`: a page, `baseline(page)` as `Ok((text, length))` or `Err(name)` (a Discourse preload whose
`posts` is `null`, a number or a boolean raises `TypeError` out of `baseline`), and `html2txt(page)` with
cleaning. The pages are JSON-LD and Discourse scripts that pin each divergence of Python's `json.loads` from
a stock JSON parser, the lone-surrogate rules for values and keys, the markup test of embedded values, the
`@type` needle search, the article cutoff, the paragraph and JSON dedupe, the teaser against the body dump,
and the empty load. Pages of thousands of nesting levels are unit tests, not cases.

The generator prints the count of every kind of case to stderr.
"""

import contextlib
import html as htmllib
import json
import logging
import sys
from dataclasses import dataclass, replace
from typing import Callable

from parity import gen_main_extractor_cases as main_cases
from parity.gen_main_extractor_cases import Options, prose, rust_options
from parity.gen_xml_output_cases import rewrite_namespaces
from parity.gen_xpath_cases import rust_str

# built so the page text holds a literal backslash and nothing rewrites an escape written here
BS = chr(92)
FORMATS = ("txt", "markdown", "html")


def exact(seed: int, length: int) -> str:
    "A text of exactly `length` characters that does not end in a space."
    text = prose(seed, length + 10)[:length]
    return text[:-1] + "x" if text.endswith(" ") else text


def doc(body: str, head: str = "") -> str:
    return f"<html><head><title>t</title>{head}</head><body>{body}</body></html>"


# ---------------------------------------------------------------- the cascade trace


@dataclass
class Trace:
    "What one `extract` call did in `trafilatura_sequence`, read off wrappers around its steps."

    forum: bool | None = None
    captured: bool = False
    len_first: int | None = None
    baseline_len: int | None = None
    page_len: int | None = None
    retry_len: int | None = None
    retry_error: bool = False
    final: str = "main"
    salvage: bool = False

    @property
    def baseline_ran(self) -> bool:
        return self.baseline_len is not None

    @property
    def baseline_accepted(self) -> bool:
        return self.baseline_ran and self.len_first is not None and self.baseline_len > self.len_first

    @property
    def retry_ran(self) -> bool:
        return self.retry_len is not None or self.retry_error

    @property
    def retry_accepted(self) -> bool:
        return self.final == "retry"

    def line(self) -> str:
        return (f"forum={self.forum} captured={self.captured} first={self.len_first} baseline={self.baseline_len} "
                f"page={self.page_len} retry={self.retry_len} err={self.retry_error} final={self.final} "
                f"salvage={self.salvage}")


class _Salvage(logging.Handler):
    def __init__(self, trace: Trace) -> None:
        super().__init__(logging.DEBUG)
        self.trace = trace

    def emit(self, record: logging.LogRecord) -> None:
        if "thread-forum salvage" in record.getMessage():
            self.trace.salvage = True


@contextlib.contextmanager
def tracing(trace: Trace):
    "Wrap the steps of `core.trafilatura_sequence` so they report into `trace`."
    from trafilatura import core

    names = ("_forum_thread_page", "extract_comments", "_extract_and_compare", "baseline", "html2txt",
             "_recall_retry", "trafilatura_sequence")
    saved = {name: getattr(core, name) for name in names}
    seen: dict = {}
    in_retry = [False]

    def forum_page(tree):
        trace.forum = saved["_forum_thread_page"](tree)
        return trace.forum

    def comments(tree, options):
        result = saved["extract_comments"](tree, options)
        trace.captured = bool(result[1])
        return result

    def compare(*args, **kwargs):
        result = saved["_extract_and_compare"](*args, **kwargs)
        if not in_retry[0] and trace.len_first is None:
            trace.len_first = len(result[1])
        return result

    def baseline(tree):
        result = saved["baseline"](tree)
        seen["baseline"] = result[0]
        trace.baseline_len = result[2]
        return result

    def html2txt(tree, *args, **kwargs):
        text = saved["html2txt"](tree, *args, **kwargs)
        trace.page_len = len(text)
        return text

    def retry(*args, **kwargs):
        in_retry[0] = True
        try:
            result = saved["_recall_retry"](*args, **kwargs)
        except Exception:
            trace.retry_error = True
            raise
        finally:
            in_retry[0] = False
        seen["retry"] = result[0]
        trace.retry_len = len(result[1])
        return result

    def sequence(*args, **kwargs):
        result = saved["trafilatura_sequence"](*args, **kwargs)
        if result[0] is seen.get("retry"):
            trace.final = "retry"
        elif result[0] is seen.get("baseline"):
            trace.final = "baseline"
        return result

    wrappers = {"_forum_thread_page": forum_page, "extract_comments": comments, "_extract_and_compare": compare,
                "baseline": baseline, "html2txt": html2txt, "_recall_retry": retry, "trafilatura_sequence": sequence}
    logger = logging.getLogger("trafilatura.core")
    handler, level = _Salvage(trace), logger.level
    for name, wrapper in wrappers.items():
        setattr(core, name, wrapper)
    logger.addHandler(handler)
    logger.setLevel(logging.DEBUG)
    try:
        yield
    finally:
        logger.removeHandler(handler)
        logger.setLevel(level)
        for name, original in saved.items():
            setattr(core, name, original)


# ---------------------------------------------------------------- pages


@dataclass
class Target:
    """A page for a branch of the cascade; `check` states the branch, on the trace of the base options, and `text`
    a text and whether the `txt` output holds it, so that the branch shows in what `extract` returns."""

    name: str
    html: str
    options: Options
    check: Callable[[Trace], bool] | None
    text: tuple[str, bool] | None = None


TARGETS: list[Target] = []
BASELINE_PAGES: list[str] = []

FORUM_LD = ('<script type="application/ld+json">{"@context":"https://schema.org",'
            '"@type":"DiscussionForumPosting","headline":"Thread"}</script>')


def target(name: str, html: str, check: Callable[[Trace], bool] | None = None, text: tuple[str, bool] | None = None,
           **options) -> None:
    TARGETS.append(Target(name, html, Options(**options), check, text))


def baseline_page(html: str) -> None:
    BASELINE_PAGES.append(html)


def posts(seed: int, count: int, length: int) -> str:
    return "".join(f"<p>{exact(seed + i, length)}</p>" for i in range(count))


def sidebar(seed: int, length: int) -> str:
    "Text that `html2txt` counts and every extraction prunes: it widens the page without adding content."
    return f'<div class="sidebar"><p>{exact(seed, length)}</p></div>'


def forum_targets() -> None:
    comments = f'<div id="comments">{posts(310, 3, 90)}</div>'
    # the posts are captured, and the body (an article over the length floor) leaves them out: the salvage restores them
    target("forum salvage", doc(f"<article><p>{exact(300, 300)}</p></article>{comments}", FORUM_LD),
           lambda t: t.forum and t.captured and not t.baseline_ran and not t.retry_ran and t.salvage)
    # comments off: a forum is not pruned, nothing is captured
    target("forum, comments off", doc(f"<article><p>{exact(300, 300)}</p></article>{comments}", FORUM_LD),
           lambda t: t.forum and not t.captured and not t.salvage, comments=False)
    # precision keeps the capture and skips the baseline and the escalation
    target("forum, precision", doc(f"<article><p>{exact(300, 300)}</p></article>{comments}", FORUM_LD),
           lambda t: t.forum and t.captured and not t.baseline_ran and not t.retry_ran and t.salvage,
           focus="precision")
    # precision with comments off prunes the raw tree even on a forum
    target("forum, precision, comments off", doc(f"<article><p>{exact(300, 300)}</p></article>{comments}", FORUM_LD),
           lambda t: t.forum and not t.captured and not t.salvage, focus="precision", comments=False)
    # a short page whose baseline is accepted: the captured posts are dropped, not salvaged
    short = (f'<div><p>{exact(320, 40)}</p><p class="meta">{exact(321, 200)}</p></div>'
             f'<div id="comments">{posts(330, 3, 30)}</div>')
    target("forum, baseline accepted", doc(short, FORUM_LD),
           lambda t: t.forum and t.captured and t.baseline_accepted and not t.salvage)
    # the same page, baseline not accepted (precision skips it): the posts come back through the salvage
    target("forum, short, precision", doc(short, FORUM_LD),
           lambda t: t.forum and t.captured and not t.baseline_ran, focus="precision")
    # a forum whose escalation input is the raw tree
    wide = (f"<article><p>{exact(340, 120)}</p></article><div>{exact(341, 300)}</div>"
            f"{comments}{sidebar(342, 2500)}")
    target("forum, escalation", doc(wide, FORUM_LD), lambda t: t.forum and t.retry_ran)
    # the marker is a forum only when the key is anchored: prose and other types are not. The page keeps the
    # captured posts in the output only on a forum (comments off prunes them elsewhere), so the output tells.
    # `load_html` strips the separator control before the check, so the Python whitespace of the marker pattern is a unit test (`forum_regex_uses_python_whitespace`)
    posts_text = exact(310, 90)
    ldjson = '<script type="application/ld+json">%s</script>'
    markers = [
        ("forum marker, none", "", False),
        ("forum marker in a description",
         ldjson % '{"description":"a DiscussionForumPosting is a type","@type":"Article"}', False),
        ("forum marker in a plain script", FORUM_LD.replace("application/ld+json", "text/javascript"), False),
        ("forum marker, plain", FORUM_LD, True),
        ("forum marker in a type array", ldjson % '{"@type":["Thing","DiscussionForumPosting"]}', True),
        ("forum marker with spaces", ldjson % '{"@type"  :  "DiscussionForumPosting"}', True),
        ("forum marker with a control the load strips", ldjson % '{"@type"\x1c:\x1c"DiscussionForumPosting"}', True),
        ("forum marker in a second script", ldjson % '{"@type":"Article"}' + FORUM_LD, True),
    ]
    for name, head, is_forum in markers:
        target(name, doc(wide, head), lambda t, is_forum=is_forum: t.forum == is_forum, text=(posts_text, is_forum),
               comments=False)
    target("comments with no forum", doc(f"<article><p>{exact(300, 300)}</p></article>{comments}"),
           lambda t: not t.forum and t.captured and not t.salvage)


def baseline_targets() -> None:
    body = prose(350, 300)
    # the JSON-LD body rescues a page the extractor leaves empty
    target("baseline rescue from JSON-LD",
           doc("<div>menu</div>", f'<script type="application/ld+json">{{"articleBody":"{body}"}}</script>'),
           lambda t: t.baseline_accepted and t.final == "baseline")
    # the article tag rescues a page whose extraction drops the discard-class paragraph
    target("baseline rescue from an article",
           doc(f'<article><p>{exact(351, 100)}</p><p class="meta">{exact(352, 150)}</p></article>'),
           lambda t: t.baseline_accepted and t.final == "baseline")
    # the baseline finds the same text: not longer, not accepted
    target("baseline equal", doc(f"<article><p>{exact(353, 230)}</p></article>"),
           lambda t: t.baseline_ran and not t.baseline_accepted and t.final == "main")
    target("baseline shorter", doc(f"<div><p>{exact(354, 80)}</p></div>"),
           lambda t: t.baseline_ran and not t.baseline_accepted and t.final == "main")
    target("baseline over the floor", doc(f"<article><p>{exact(355, 300)}</p></article>"),
           lambda t: not t.baseline_ran)
    target("baseline, precision skips it",
           doc(f'<article><p>{exact(351, 100)}</p><p class="meta">{exact(352, 150)}</p></article>'),
           lambda t: not t.baseline_ran, focus="precision")
    target("nothing to extract", doc("<div></div>"), lambda t: t.baseline_ran and t.len_first == 0)


def escalation_targets() -> None:
    # recall keeps the teaser block inside the frame that balanced prunes
    def teaser(length: int) -> str:
        return (f'<article><p>{exact(360, 300)}</p><div class="teaser"><p>{exact(361, length)}</p></div></article>'
                f"{sidebar(362, 1200)}")

    target("escalation accepted, teaser", doc(teaser(400)), lambda t: t.retry_accepted and not t.baseline_ran)
    target("escalation refused by the ratio", doc(teaser(100)),
           lambda t: t.retry_ran and not t.retry_accepted and t.retry_len >= 250
           and t.retry_len <= 1.5 * t.len_first)
    # recall recovers the divs outside the frame that balanced leaves
    def outside(length: int) -> str:
        return (f"<article><p>{exact(363, 120)}</p></article><div>{exact(364, length)}</div>"
                f"{sidebar(365, 1200)}")

    target("escalation accepted, divs", doc(outside(300)), lambda t: t.retry_accepted)
    target("escalation refused by the floor", doc(outside(100)),
           lambda t: t.retry_ran and not t.retry_accepted and 1.5 * t.len_first < t.retry_len < 250)
    target("escalation page share", doc(f"<article><p>{exact(366, 300)}</p></article>{sidebar(367, 300)}"),
           lambda t: not t.retry_ran and t.page_len is not None)
    # the escalation input: comments captured, or removed without a capture
    captured = f'<div id="comments">{posts(370, 2, 40)}</div>'
    target("escalation, comments captured", doc(outside(300) + captured),
           lambda t: t.captured and t.retry_accepted)
    uncaptured = '<div id="comment1"><p>Reader says something about the page.</p></div>'
    target("escalation, comments removed", doc(outside(300) + uncaptured),
           lambda t: not t.captured and t.retry_ran)
    target("escalation, comments off", doc(outside(300) + captured), lambda t: t.retry_ran, comments=False)
    target("escalation, comment lists", doc(outside(300) + '<ul class="commentlist"><li>First comment text</li>'
                                            "<li>Second comment text</li></ul>"),
           lambda t: t.retry_ran)
    target("escalation under recall", doc(outside(300)), lambda t: not t.retry_ran, focus="recall")
    target("escalation under precision", doc(outside(300)), lambda t: not t.retry_ran, focus="precision")
    boundary_targets()


def boundary_targets() -> None:
    """The comparisons of stages 3 and 4 at their edges: the floor, the length cap, the page share, the ratio.
    Each edge has a page on each side that gains text from the stage on one side only (a discard-class paragraph
    the baseline recovers, a teaser block the recall retry keeps), so the outputs differ and a `<` for a `<=`
    in the cascade fails a case."""

    # the main text is n paragraphs, joined by one space each: the length is the sum plus n - 1
    def article(lengths: list[int], inside: str = "", extra: str = "") -> str:
        paragraphs = "".join(f"<p>{exact(400 + i, n)}</p>" for i, n in enumerate(lengths))
        return doc(f"<article>{paragraphs}{inside}</article>{extra}")

    def teaser(seed: int, length: int) -> str:
        return f'<div class="teaser"><p>{exact(seed, length)}</p></div>'

    meta = exact(440, 60)
    meta_block = f'<p class="meta">{meta}</p>'
    target("baseline floor, one under", article([249], meta_block),
           lambda t: t.len_first == 249 and t.baseline_accepted, text=(meta, True))
    target("baseline floor", article([250], meta_block),
           lambda t: t.len_first == 250 and not t.baseline_ran, text=(meta, False))
    gain = exact(441, 1600)
    wide = sidebar(450, 20000)
    nine = [300] * 9
    target("length cap, one under", article(nine + [290], teaser(441, 1600), wide),
           lambda t: t.len_first == 2999 and t.retry_accepted, text=(gain, True))
    target("length cap", article(nine + [291], teaser(441, 1600), wide),
           lambda t: t.len_first == 3000 and not t.retry_ran, text=(gain, False))
    # html2txt of the page: the article, the teaser and the sidebar paragraph, each joined by a space
    share = exact(442, 300)
    target("page share, equal", article([300], teaser(442, 300), sidebar(451, 898)),
           lambda t: t.page_len == 1500 and not t.retry_ran, text=(share, False))
    target("page share, one over", article([300], teaser(442, 300), sidebar(452, 899)),
           lambda t: t.page_len == 1501 and t.retry_accepted, text=(share, True))

    def ratio(length: int) -> str:
        block = f'<div class="teaser"><p>{exact(453, length)}</p></div>'
        return doc(f'<article><p>{exact(454, 300)}</p>{block}</article>{sidebar(455, 1200)}')

    target("ratio, equal", ratio(149),
           lambda t: t.len_first == 300 and t.retry_len == 450 and not t.retry_accepted)
    target("ratio, one over", ratio(150), lambda t: t.retry_len == 451 and t.retry_accepted)

    def outside(length: int) -> str:
        return doc(f"<article><p>{exact(456, 120)}</p></article><div>{exact(457, length)}</div>{sidebar(458, 1200)}")

    target("retry floor, below", outside(127), lambda t: t.retry_len == 248 and not t.retry_accepted)
    target("retry floor, one under", outside(128), lambda t: t.retry_len == 249 and not t.retry_accepted)
    target("retry floor, reached", outside(129), lambda t: t.retry_len == 250 and t.retry_accepted)


def targeted_pages() -> None:
    forum_targets()
    baseline_targets()
    escalation_targets()


# ---------------------------------------------------------------- baseline pages


def ld(payload: str, body: str = "", head: str = "") -> str:
    return doc(body, f'{head}<script type="application/ld+json">{payload}</script>')


def js(text: str) -> str:
    return json.dumps(text, ensure_ascii=False)


def text_item(text: str) -> str:
    return '{"articleBody": ' + js(text) + "}"


def json_grammar_pages() -> None:
    "Findings 5: where Python's `json.loads` and a stock JSON parser disagree, each deciding which scripts count."
    body, other, fall = prose(1001, 150), prose(1002, 150), prose(1003, 150)
    fallback = f"<p>{fall}</p>"

    def page(payload: str) -> None:
        baseline_page(ld(payload, fallback))

    page(text_item(body))
    # control characters: only strict=False takes them raw inside a string
    for raw in ("\n", "\t", "\r\n", "\n\n"):
        page('{"articleBody": "' + body[:60] + raw + body[60:] + '"}')
    for escape in ("u0000", "u0001", "u001f", "u007f", "n", "t", "/", "b", "f", "r"):
        page('{"articleBody": "' + body[:60] + BS + escape + body[60:] + '"}')
    # escapes the grammar does not have
    for escape in ("x41", "a", "U0041", "u00", "u00g1", "'", "0", "v"):
        page('{"articleBody": "' + body[:60] + BS + escape + body[60:] + '"}')
    # surrogate escapes in a value: lone high, lone low, reversed, a pair, a high before a non-low escape
    high, low = BS + "ud800", BS + "udc00"
    for middle in (high, low, low + high, high + low, high + high, high + BS + "u0041", BS + "ud83d" + BS + "ude00",
                   BS + "ud83d", high + " ", BS + "uD800", BS + "udbff" + BS + "udfff", BS + "uffff", BS + "ufffe"):
        page('{"articleBody": "' + body[:60] + middle + body[60:] + '"}')
    # NaN, Infinity, floats and integers past the limits
    for number in ("NaN", "Infinity", "-Infinity", "1e999", "-1e999", "1E999", "1" + "0" * 400 + ".5", "0." + "1" * 5000,
                   "1" + "0" * 5000 + ".0", "0e0", "-0", "1E5", "1e-999", "1" * 4300, "1" * 4301, "-" + "1" * 4301,
                   "1" * 4302, "1" * 4300 + "e1", "1" * 4301 + ".0", "1e4301"):
        page('{"articleBody": ' + js(body) + ', "n": ' + number + "}")
        page("[" + number + ", " + text_item(body) + "]")
    page('{"n": ' + "9" * 4301 + ', "articleBody": ' + js(body) + "}")
    page('{"n": [' + "9" * 4300 + "], " + '"articleBody": ' + js(body) + "}")
    # duplicate keys keep the first position and the last value
    page('{"articleBody": ' + js(other) + ', "x": 1, "articleBody": ' + js(body) + "}")
    page('{"articleBody": ' + js(body) + ', "articleBody": 5}')
    page('{"articleBody": 5, "articleBody": ' + js(body) + "}")
    page('{"a": 1, "articleBody": ' + js(other) + ', "a": 2, "articleBody": ' + js(body) + ', "a": 3}')
    page('{"articleBody": ' + js(body) + ', "articleBody": null}')
    page('[{"articleBody": ' + js(other) + '}, {"articleBody": ' + js(body) + ', "articleBody": ' + js(other) + "}]")
    # whitespace: only space, tab, newline and carriage return
    for lead in (" ", "\t", "\n", "\r\n", " \t\n ", "\N{ZERO WIDTH NO-BREAK SPACE}", "\N{NO-BREAK SPACE}",
                 "\N{LINE SEPARATOR}", "\N{IDEOGRAPHIC SPACE}", "\N{EM SPACE}"):
        page(lead + text_item(body))
        page(text_item(body) + lead)
        page('{"articleBody":' + lead + js(body) + "}")
        page('{' + lead + '"articleBody":' + js(body) + "}")
    # syntax errors, each ending the script's contribution
    for broken in (
        text_item(body) + " x", text_item(body) + text_item(other), text_item(body) + ",",
        '{"articleBody": ' + js(body) + ",}", "[" + text_item(body) + ",]", "[," + text_item(body) + "]",
        '{"n": 01, "articleBody": ' + js(body) + "}", '{"n": -01, "articleBody": ' + js(body) + "}",
        '{"n": 1., "articleBody": ' + js(body) + "}", '{"n": .5, "articleBody": ' + js(body) + "}",
        '{"n": +1, "articleBody": ' + js(body) + "}", '{"n": -, "articleBody": ' + js(body) + "}",
        '{"n": tru, "articleBody": ' + js(body) + "}", '{"n": True, "articleBody": ' + js(body) + "}",
        '{"n": -NaN, "articleBody": ' + js(body) + "}", '{"n": +Infinity, "articleBody": ' + js(body) + "}",
        '{"n": Infinit, "articleBody": ' + js(body) + "}", '{"n": nan, "articleBody": ' + js(body) + "}",
        "{'articleBody': " + js(body) + "}", '{"articleBody": \'' + body + "'}",
        '{articleBody: ' + js(body) + "}", '{"articleBody" ' + js(body) + "}", '{"articleBody": ' + js(body),
        '{"articleBody": "' + body + '}', '{"articleBody": ' + js(body) + "} // c", "/* c */ " + text_item(body),
        '{"articleBody": ' + js(body) + ', "n": 1 2}', '{"articleBody": ' + js(body) + ' "n": 1}',
        '{"articleBody": ' + js(body) + ', 5: 1}', '{"articleBody": ' + js(body) + ', "n": [1 2]}',
        '{"articleBody": ' + js(body) + ', "n": {"a" 1}}', "", "   ", "{}", "[]", "null", '"articleBody"',
        '{"articleBody": ' + js(body) + ', "n": 1e}', '{"articleBody": ' + js(body) + ', "n": 1e+}',
        '{"articleBody": ' + js(body) + ', "n": 0x10}', '{"articleBody": ' + js(body) + ', "n": 1_0}',
    ):
        page(broken)
    # valid forms that look odd
    for valid in (
        "[" + text_item(body) + "]", "[ " + text_item(body) + " , " + text_item(other) + " ]",
        '{"articleBody": ' + js(body) + ', "n": -0.0e-0, "m": [], "o": {}, "p": [[[]]], "q": null, "r": true, "s": false}',
        '{"": 1, "articleBody": ' + js(body) + "}", '{"articleBody": ' + js(body) + ', "n": 1E+5, "m": 0.5e1}',
        "[1, null, true, " + text_item(body) + ', "x", [' + text_item(other) + "]]",
    ):
        page(valid)
    # nesting well under the cap: only the top level, `@graph` and `mainEntity` are walked
    nest = 40
    page("[" * nest + text_item(body) + "]" * nest)
    page('{"x": ' + "[" * nest + text_item(body) + "]" * nest + "}")
    graph = text_item(body)
    for _ in range(nest):
        graph = '{"@graph": ' + graph + "}"
    page(graph)
    chain = text_item(body)
    for level in range(nest):
        chain = '{"mainEntity": ' + chain + ', "articleBody": ' + js(exact(1100 + level, 110)) + "}"
    page(chain)


def surrogate_pages() -> None:
    "Findings 6: a lone surrogate never reaches the output, and object keys keep the code units they were written with."
    fall = prose(1010, 150)
    fallback = f"<p>{fall}</p>"
    high, low, ffff = BS + "ud800", BS + "ud801", BS + "uffff"
    pad = prose(1011, 120)
    for value in (f"word \u2026 {high} end {pad}", f"&am{high}p; {pad}", f"x{high}&amp;{pad}", f"{ffff} {pad}",
                  f"{high}{high}{high} {pad}", f"{low}{high} {pad} {BS}ud83d{BS}ude00 pair",
                  f"{pad} &#1;{high}&#x7f;&#0;", f"&{high}amp; {pad}", f"&lt;p&gt;{high}x&lt;/p&gt; {pad}"):
        baseline_page(ld('{"articleBody": "' + value.replace("\u2026", "\N{HORIZONTAL ELLIPSIS}") + '"}', fallback))
    # keys: the three stay distinct, a repeated one keeps its first position
    topics = [
        [("topic_1", "A")],
        [("topic_" + high, "A"), ("topic_" + low, "B")],
        [("topic_" + high, "A"), ("topic_" + ffff, "B")],
        [("topic_" + low, "A"), ("topic_" + ffff, "B")],
        [("topic_" + high, "A"), ("topic_" + low, "B"), ("topic_" + ffff, "C")],
        [("topic_" + high, "A"), ("topic_" + high, "B")],
        [("topic_" + high, "A"), ("topic_" + BS + "uD800", "B")],
        [("topic_" + high, "A"), ("topic_" + ffff, "B"), ("topic_" + high, "C")],
        [(BS + "ud800" + "topic_", "A")],
        [("topic_" + BS + "ud83d" + BS + "ude00", "A"), ("topic_" + BS + "ud83d", "B")],
    ]
    for topic in topics:
        baseline_page(discourse([(key, [f"<p>{exact(1020 + ord(label), 70)}</p>"]) for key, label in topic]))
    # an ld script holding a key with a surrogate: the keys stay apart, the text counts
    baseline_page(ld('{"k' + high + '": 1, "k' + low + '": 2, "articleBody": ' + js(prose(1030, 150)) + "}", fallback))
    baseline_page(ld('{"articleBody": ' + js(prose(1031, 150)) + ', "k' + high + '": 1, "k' + high + '": 2}', fallback))


def discourse(topics: list[tuple[str, list[str]]], posts_value: str | None = None) -> str:
    "A page whose posts sit in the `data-preloaded` attribute of a Discourse forum."
    entries = []
    for key, cooked in topics:
        inner = json.dumps({"post_stream": {"posts": [{"cooked": text} for text in cooked]}})
        entries.append(f'"{key}": {json.dumps(inner)}')
    return preload("{" + ", ".join(entries) + "}")


def preload(value: str, body: str = "") -> str:
    return doc(f'<div id="data-preloaded" data-preloaded="{htmllib.escape(value)}"></div>{body}')


def discourse_pages() -> None:
    "Findings 8: `posts` of another type than a list; the shapes of the preload."
    cooked = [f"<p>{exact(1040, 80)}</p>", f"<p>{exact(1041, 80)}</p>"]
    fall = f"<p>{prose(1042, 150)}</p>"

    def with_posts(value: str, key: str = "topic_1") -> str:
        inner = '{"post_stream": {"posts": ' + value + "}}"
        return preload("{" + f'"{key}": {json.dumps(inner)}' + "}", fall)

    for value in ("null", "5", "0", "1.5", "true", "false", '"abc"', "{}", '{"a": 1}', "[]", "[1, 2]", "[null]",
                  '[{"cooked": 5}, {"cooked": null}, {"cooked": "' + exact(1043, 120) + '"}]',
                  '[{"cooked": ""}]', '[{"other": "x"}, "string", ' + json.dumps({"cooked": exact(1044, 120)}) + "]"):
        baseline_page(with_posts(value))
    baseline_page(with_posts(json.dumps([{"cooked": text} for text in cooked])))
    baseline_page(with_posts(json.dumps([{"cooked": text} for text in cooked]), key="other_1"))
    baseline_page(with_posts(json.dumps([{"cooked": f"<p>{exact(1045, 120)}</p>"}]), key="topic_"))
    # the preload itself
    for value in ("[]", "[1]", '"x"', "5", "null", "true", "", "{", "{}", '{"topic_1": 5}', '{"topic_1": null}',
                  '{"topic_1": "not json"}', '{"topic_1": "[]"}', '{"topic_1": "{}"}',
                  '{"topic_1": "{\\"post_stream\\": 5}"}', '{"topic_1": "{\\"post_stream\\": {}}"}'):
        baseline_page(preload(value, fall))
    baseline_page(discourse([("topic_1", cooked), ("topic_2", [f"<p>{exact(1046, 90)}</p>"])]))
    baseline_page(discourse([("other", cooked), ("topic_2", [f"<p>{exact(1046, 90)}</p>"]), ("topic_1", cooked[:1])]))
    baseline_page(discourse([("topic_1", cooked)]).replace('id="data-preloaded"', 'id="data-preloaded2"'))
    baseline_page(doc(f'<div id="data-preloaded"></div>{fall}'))
    # a null topic first and a good one after: the first raises out of baseline, whatever follows
    first = json.dumps('{"post_stream": {"posts": null}}')
    second = json.dumps(json.dumps({"post_stream": {"posts": [{"cooked": exact(1047, 120)}]}}))
    baseline_page(preload("{" + f'"topic_1": {first}, "topic_2": {second}' + "}", fall))
    baseline_page(preload("{" + f'"topic_2": {second}, "topic_1": {first}' + "}", fall))
    # a Discourse page with a JSON-LD body: the ld text comes first in the paragraphs
    baseline_page(doc(f'<div id="data-preloaded" data-preloaded="'
                      + htmllib.escape('{"topic_1": ' + json.dumps(json.dumps({"post_stream": {"posts": [{"cooked": exact(1048, 120)}]}})) + "}")
                      + '"></div>', f'<script type="application/ld+json">{text_item(prose(1049, 130))}</script>'))


def structure_pages() -> None:
    "The walk of `_walk_json`: text keys, recipe steps, answers, teasers, `@graph` and `mainEntity`, and their order."
    fall = f"<p>{prose(1050, 150)}</p>"

    def page(payload: str, body: str = fall) -> None:
        baseline_page(ld(payload, body))

    a, b, c, d, e = (prose(1051 + i, 120) for i in range(5))
    page('{"articleBody": ' + js(a) + ', "reviewBody": ' + js(b) + "}")
    page('{"reviewBody": ' + js(b) + ', "articleBody": ' + js(a) + "}")
    page('{"articleBody": "", "reviewBody": ' + js(b) + "}")
    page('{"articleBody": ' + js(a) + ', "reviewBody": ["x"]}')
    page('{"articleBody": ["' + a + '"]}')
    page('{"articleBody": 5, "reviewBody": null}')
    # recipe and how-to instructions
    page('{"recipeInstructions": ' + js(a + " " + b) + "}")
    page('{"recipeInstructions": [' + js(a) + ", " + js(b) + "]}")
    page('{"recipeInstructions": [{"text": ' + js(a) + '}, {"text": ' + js(b) + '}, {"text": 5}, {"name": "x"}]}')
    page('{"recipeInstructions": {"text": ' + js(a + " " + b) + "}}")
    page('{"recipeInstructions": [{"@type": "HowToSection", "itemListElement": [{"text": ' + js(a) + '}, {"text": '
         + js(b) + '}]}]}')
    page('{"recipeInstructions": [{"itemListElement": {"text": ' + js(a + " " + b) + '}}]}')
    page('{"recipeInstructions": [{"itemListElement": [{"itemListElement": [{"text": ' + js(a + " " + b) + "}]}]}]}")
    page('{"@type": "HowTo", "step": [{"text": ' + js(a) + '}, ' + js(b) + "]}")
    page('{"step": ' + js(a + " " + b) + ', "recipeInstructions": ' + js(c) + "}")
    page('{"@type": "HowTo", "name": "x"}')
    page('{"@type": ["HowTo"], "step": [{"text": ' + js(a + " " + b) + "}]}")
    # FAQ answers: a dict only
    page('{"acceptedAnswer": {"text": ' + js(a + " " + b) + "}}")
    page('{"acceptedAnswer": [{"text": ' + js(a + " " + b) + "}]}")
    page('{"acceptedAnswer": {"text": 5}, "articleBody": ' + js(a) + "}")
    page('{"@type": "Question", "acceptedAnswer": {"@type": "Answer", "text": ' + js(a + " " + b) + "}}")
    page('{"mainEntity": [{"@type": "Question", "acceptedAnswer": {"text": ' + js(a) + '}}, {"@type": "Question", '
         '"acceptedAnswer": {"text": ' + js(b) + "}}]}")
    # one item carrying every source: the order of the paragraphs follows the walk
    page('{"mainEntity": {"articleBody": ' + js(e) + '}, "@graph": [{"articleBody": ' + js(d) + '}], '
         '"acceptedAnswer": {"text": ' + js(c) + '}, "recipeInstructions": ' + js(b) + ', "articleBody": ' + js(a) + "}")
    page('[{"@graph": [{"articleBody": ' + js(a) + '}, {"mainEntity": {"articleBody": ' + js(b) + '}}]}, '
         '{"articleBody": ' + js(c) + "}]")
    page('{"@graph": {"articleBody": ' + js(a + " " + b) + '}, "mainEntity": [[{"articleBody": ' + js(c) + "}]]}")
    page('{"@graph": [1, "x", null, {"articleBody": ' + js(a + " " + b) + "}]}")
    page('{"@graph": "x", "mainEntity": 5, "articleBody": ' + js(a + " " + b) + "}")
    # two scripts, the same body twice (theme and SEO plugin), and different ones
    baseline_page(doc(fall, f'<script type="application/ld+json">{text_item(a)}</script>'
                      f'<script type="application/ld+json">{text_item(a)}</script>'))
    baseline_page(doc(fall, f'<script type="application/ld+json">{text_item(a)}</script>'
                      f'<script type="application/ld+json">{text_item(b)}</script>'))
    baseline_page(doc(fall, f'<script type="application/ld+json">{text_item(a)}</script>'
                      f'<script type="application/ld+json">{{broken</script>'
                      f'<script type="application/ld+json">{text_item(b)}</script>'))
    baseline_page(doc(fall, f'<script type="application/ld+json">{text_item(a)}</script>'
                      f'<script type="application/json">{text_item(b)}</script>'
                      f'<script>{text_item(c)}</script>'))
    # the dedupe of JSON texts: a long repeat is dropped, a short one stays
    long, short = exact(1060, 80), exact(1061, 50)
    for texts in ([long, long], [long, long, long], [short, short, exact(1062, 110)], [exact(1063, 51)] * 2 + [a],
                  [long, "x " + long + " y", a], [long, long[:60], a], [a, long[:40], long[:60], b],
                  [exact(1064, 50)] * 3 + [exact(1065, 200)]):
        page("[" + ", ".join(text_item(text) for text in texts) + "]")
    # the length gate: more than 100 characters in all
    page(text_item(exact(1066, 100)))
    page(text_item(exact(1067, 101)))
    page("[" + text_item(exact(1068, 50)) + ", " + text_item(exact(1069, 50)) + "]")
    page("[" + text_item(exact(1068, 50)) + ", " + text_item(exact(1069, 49)) + "]")
    page("[" + text_item(exact(1068, 50)) + ", " + text_item(exact(1069, 50)) + ", " + text_item(exact(1070, 1)) + "]")
    # hooks: the script is parsed only when it holds one
    page('{"description": ' + js(a) + ', "@type": "Article"}')
    page('{"step": ' + js(a + b) + "}")
    page('{"@type": "HowTo", "text": ' + js(a + b) + "}")


def markup_pages() -> None:
    "Findings 9: what counts as markup in an embedded value, and what the fragment parse then does with it."
    fall = f"<p>{prose(1080, 150)}</p>"
    pad = prose(1081, 100)
    values = [
        f"<p>{pad}</p>", f"&lt;p&gt;{pad}&lt;/p&gt;", f"&amp;lt;p&amp;gt;{pad}&amp;lt;/p&amp;gt;", f"{pad} <br/> more",
        f"{pad} <br> more", f"{pad} </b> more", f"i<b and c>d {pad}", f"a < b and c > d {pad}",
        f'{pad} <span class="x">tail</span>', f"{pad} <span foo>tail</span>", f'{pad} <a href="x">link</a>',
        f"{pad} <DIV>upper</DIV>", f"{pad} <Div>mixed</Div>", f"{pad} <unknown>word</unknown>",
        f"{pad} <script>x</script> end", f"{pad} <p>one<p>two", f"{pad} <ul><li>a<li>b</ul>",
        f"{pad} <table><tr><td>a</td></tr></table> end", f"{pad} <b>bold</b><i>it</i> end",
        f"<h1>head</h1>{pad}<h2>sub</h2>", f"{pad} <pre>  keep  </pre> end", f"{pad} &nbsp;&amp;&lt;&gt;&quot;&#39; end",
        f"{pad} &#1;&#2;&#x7f;&#x80;&#0; end", f"{pad} &amp;amp; &amp;#65;", f"{pad}   spaced   out   ", f"\n{pad}\n",
        f"{pad} <!-- c --> end", f"{pad} <?php x ?> end", f"{pad} <![CDATA[ x ]]> end", f"{pad} <img src=x> end",
        f"{pad} <br/ > end", f"{pad} <br\t/> end", f'{pad} <p class="a"\nid="b">x</p>',
        f"<dIv>{pad}</dIv>", "<dİv>x</dİv> " + pad, "<dıv>x</dıv> " + pad, "<ſpan>x</ſpan> " + pad,
        "<Kbd>x</Kbd> " + pad, "<kbd>x</kbd> " + pad, "<dİv class=\"a\">x " + pad, "<dıv id=b>x " + pad,
        "x<ſpan class='a'> " + pad, "x<Kbd id=b> " + pad, "<p İ>x</p> " + pad, "<İ>x</İ> " + pad,
        f"{pad} <di\N{LATIN SMALL LETTER DOTLESS I}v>w</di\N{LATIN SMALL LETTER DOTLESS I}v>",
        f"{pad} <\N{LATIN SMALL LETTER LONG S}trong>w</\N{LATIN SMALL LETTER LONG S}trong>",
        f"{pad} <\N{KELVIN SIGN}bd>w</\N{KELVIN SIGN}bd>", f"{pad} <b\N{KELVIN SIGN}>w",
        f"{pad} <div\x1cclass=\"a\">w", f"{pad} <div\x1fid=\"a\">w", f"{pad} <div\x85id=\"a\">w",
        f"{pad} <div\N{NO-BREAK SPACE}id=\"a\">w", f"{pad} <div\u2028id=\"a\">w".replace("\u2028", "\N{LINE SEPARATOR}"),
        f"{pad} <div\x0bid=\"a\">w", f"{pad} <div\x0cid=\"a\">w", f"{pad} <div\x1c/>w",
    ]
    for value in values:
        # the value as a JSON string: control characters as escapes, so the page keeps them
        quoted = json.dumps(value, ensure_ascii=False)
        baseline_page(ld('{"articleBody": ' + quoted + "}", fall))
    # the same values as a teaser and in a discourse post
    for value in values[::7]:
        quoted = json.dumps(value, ensure_ascii=False)
        baseline_page(ld('{"@type": "Product", "description": ' + quoted + "}"))
        baseline_page(discourse([("topic_1", [value])]))


def type_pages() -> None:
    "Findings 10: the teaser test is a substring search in `str` of the `@type` value."
    teaser = prose(1090, 140)
    fall = f"<p>{prose(1091, 40)}</p>"

    def page(kind: str, hook: str = '"reviewBody": "",') -> None:
        baseline_page(ld('{' + hook + ' "@type": ' + kind + ', "description": ' + js(teaser) + "}", fall))

    for kind in ('"Product"', '"ProductGroup"', '"VideoObject"', '"Video"', '"product"', '"Article"', '["Product"]',
                 '["Thing", "VideoObject"]', '[["Product"]]', '[[["x", ["VideoObject"]]]]', '{"a": "Product"}',
                 '{"Product": 1}', '{"a": ["x", {"b": "VideoObject"}]}', "5", "true", "false", "null", "[]", "{}",
                 "[5, true, null]", '[1, "Pro" , "duct"]', '"Pro" ', '["Pro", "duct"]', '"x\\nProduct"', '"VideoObject\\u0000"',
                 '["x\\u0000Product"]', '{"k\\u0000": "Product"}', '"Product\\ud800"', '"Pro\\ud800duct"',
                 '"\\u0050roduct"', '"\\u00cdProduct"', '[{"a": [{"b": [{"c": "Product"}]}]}]'):
        page(kind)
    # with only the quoted hook: the script is parsed because of the type itself
    for kind in ('"Product"', '["Product"]', '"ProductX"', '"VideoObject"', '["VideoObject", "Thing"]'):
        baseline_page(ld('{"@type": ' + kind + ', "description": ' + js(teaser) + "}", fall))
    # the description must be a string; a body wins over a teaser; a list of items
    baseline_page(ld('{"@type": "Product", "description": 5, "reviewBody": ""}', fall))
    baseline_page(ld('{"@type": "Product", "description": null, "reviewBody": ""}', fall))
    baseline_page(ld('{"@type": "Product", "description": ["' + teaser + '"], "reviewBody": ""}', fall))
    baseline_page(ld('{"@type": "Product", "description": ' + js(teaser) + ', "articleBody": ' + js(prose(1092, 130)) + "}", fall))
    baseline_page(ld('[{"@type": "Product", "description": ' + js(teaser) + '}, {"@type": "VideoObject", "description": '
                     + js(prose(1093, 140)) + "}]", fall))
    baseline_page(ld('{"@type": "Product", "description": ' + js(teaser) + ', "@graph": [{"@type": "VideoObject", '
                     '"description": ' + js(prose(1094, 140)) + "}]}", fall))
    baseline_page(ld('{"@type": "Product", "description": ' + js(exact(1095, 100)) + "}", fall))
    baseline_page(ld('{"@type": "Product", "description": ' + js(exact(1096, 101)) + "}", fall))
    baseline_page(ld('[{"@type": "Product", "description": ' + js(exact(1097, 60)) + '}, {"@type": "Product", '
                     '"description": ' + js(exact(1098, 60)) + "}]", fall))


def teaser_pages() -> None:
    "The teaser against the body dump: it wins only when the dump is shorter."
    for dump, teaser in ((50, 120), (119, 120), (120, 120), (121, 120), (300, 120), (0, 120), (101, 101), (100, 101),
                         (102, 101)):
        json_part = '{"@type": "Product", "description": ' + js(exact(1200 + teaser, teaser)) + "}"
        body = f"<div>{exact(1300 + dump, dump)}</div>" if dump else ""
        baseline_page(ld(json_part, body))
    # the dump joins the texts of the body with newlines, the teaser is rendered like any value
    baseline_page(ld('{"@type": "Product", "description": ' + js(exact(1210, 140)) + "}",
                     f"<div>{exact(1211, 40)}</div><div>{exact(1212, 40)}</div><div>{exact(1213, 40)}</div>"))
    baseline_page(ld('{"@type": "Product", "description": ' + js(exact(1210, 140)) + "}",
                     f"<div>{exact(1211, 40)}<b>x</b>{exact(1212, 40)}</div>"))
    baseline_page(ld('{"@type": "Product", "description": ' + js("<p>" + exact(1214, 140) + "</p>") + "}",
                     f"<div>{exact(1215, 50)}</div>"))
    # a teaser alone, a dump alone, neither
    baseline_page(ld('{"@type": "Product", "description": ' + js(exact(1216, 140)) + "}"))
    baseline_page(doc(f"<div>{exact(1217, 140)}</div>"))
    baseline_page(doc("<div></div>"))
    baseline_page(doc(""))
    baseline_page("<html></html>")
    baseline_page("<html><head><title>t</title></head></html>")
    baseline_page(doc("<script>var x = 1;</script><style>p {}</style>"))


def article_pages() -> None:
    "The article tier: the cutoff is a fifth of the longest article, nested ones are counted in their ancestor."
    fall = f"<p>{prose(1220, 150)}</p>"

    def art(seed: int, length: int, tag: str = "p") -> str:
        return f"<article><{tag}>{exact(seed, length)}</{tag}></article>"

    for lengths in ((1010, 202), (1010, 201), (1010, 203), (1000, 200), (1000, 199), (500, 100), (500, 101), (500, 99),
                    (101, 101), (100, 100), (101, 20), (150, 150), (1000, 400, 150), (1000, 200, 199, 201)):
        baseline_page(doc("".join(art(1230 + i, n) for i, n in enumerate(lengths)) + fall))
    # nested articles are not separate, the text of the outer one counts once
    baseline_page(doc(f"<article><p>{exact(1240, 150)}</p><article><p>{exact(1241, 150)}</p></article></article>{fall}"))
    baseline_page(doc(f"<article><article><p>{exact(1242, 400)}</p></article></article>{art(1243, 120)}{fall}"))
    # the dominant article drops small siblings; similar sizes all stay; an article under the gate falls through
    baseline_page(doc(art(1244, 600) + art(1245, 110) + art(1246, 130) + fall))
    baseline_page(doc(art(1247, 150) + art(1248, 160) + art(1249, 170)))
    baseline_page(doc(art(1250, 90) + fall))
    baseline_page(doc(art(1251, 90) + art(1252, 90) + fall))
    # block boundaries become spaces, and the cleaning removes what it targets before the scan
    baseline_page(doc("<article><div>" + exact(1253, 60) + "</div><div>" + exact(1254, 60) + "</div></article>" + fall))
    baseline_page(doc("<article>" + exact(1255, 60) + "<br>" + exact(1256, 60) + "</article>"))
    baseline_page(doc("<article><span>" + exact(1257, 60) + "</span><span>" + exact(1258, 60) + "</span></article>"))
    baseline_page(doc("<article><p>" + exact(1259, 150) + '</p><div class="footer"><p>' + exact(1260, 300)
                      + "</p></div></article>"))
    baseline_page(doc("<article><p>" + exact(1261, 150) + "</p><aside><p>" + exact(1262, 300) + "</p></aside></article>"))
    baseline_page(doc('<article><p>' + exact(1263, 150) + '</p><div class="cookie-banner"><p>' + exact(1264, 300)
                      + "</p></div><script>x</script><style>y</style></article>"))
    baseline_page(doc('<article><p>' + exact(1265, 150) + '</p><div id="cookiebot"><p>' + exact(1266, 300)
                      + '</p></div><div class="cookies-not-set"><p>' + exact(1267, 50) + "</p></div></article>"))
    baseline_page(doc("<article><p>" + exact(1268, 80) + "</p><svg><text>" + exact(1269, 90) + "</text></svg></article>"))
    baseline_page(doc("<article><p>" + exact(1270, 80) + "</p><template><p>" + exact(1271, 90) + "</p></template></article>"))
    baseline_page(doc('<article><p>' + exact(1272, 150) + '</p></article><div id="main-footer"><p>' + exact(1273, 300)
                      + "</p></div>"))


def paragraph_pages() -> None:
    "The paragraph tier: repeats longer than `MIN_DUPLICATE_LENGTH` (50) are dropped, nested elements are skipped."
    a, b = exact(1280, 80), exact(1281, 80)
    for paragraphs in ([a, a, b], [a, b, a], [a, a, a, b], [exact(1282, 50), exact(1282, 50), a], [exact(1283, 51), exact(1283, 51), a],
                       [exact(1284, 49), exact(1284, 49), exact(1284, 49), a], [a, a[10:70], b], [a, a[10:50], b],
                       [a, a[10:61], b], [a, a[10:60], b], [a, "x" + a, b], [a[:60], a, b], [a, a + " tail", b],
                       [a + " " + b, a, b], [a, " " + a + " ", b], [a, a.upper(), b], [exact(1285, 30), exact(1286, 30),
                                                                                       exact(1287, 30), exact(1288, 30)]):
        baseline_page(doc("".join(f"<p>{text}</p>" for text in paragraphs)))
    # block elements: blockquote and its paragraph, code in a paragraph, pre, q, quote
    baseline_page(doc(f"<blockquote><p>{a}</p><p>{b}</p></blockquote>"))
    baseline_page(doc(f"<blockquote>{a}<p>{b}</p></blockquote>"))
    baseline_page(doc(f"<p>{a} <code>x = 1</code> {b}</p>"))
    baseline_page(doc(f"<pre><code>{a}\n{b}</code></pre>"))
    baseline_page(doc(f"<pre>{a}</pre><p>{b}</p>"))
    baseline_page(doc(f"<q>{a}</q><q>{b}</q>"))
    baseline_page(doc(f"<quote>{a}</quote><quote>{b}</quote>"))
    baseline_page(doc(f"<div><p>{a}</p><div><p>{b}</p></div></div>"))
    baseline_page(doc(f"<ul><li><p>{a}</p></li><li><p>{b}</p></li></ul>"))
    baseline_page(doc(f"<p>{a}<p>{b}"))
    baseline_page(doc(f"<p>{a}<br>{b}</p>"))
    baseline_page(doc(f"<p>{a}</p><p></p><p> </p><p>{b}</p>"))
    baseline_page(doc(f"<p>short</p><p>{a}</p>"))
    baseline_page(doc(f"<p>{exact(1289, 30)}</p><p>{exact(1290, 30)}</p>"))
    baseline_page(doc(f"<p>{exact(1289, 30)}</p><p>{exact(1290, 30)}</p><p>{exact(1291, 30)}</p><p>{exact(1292, 30)}</p>"))
    baseline_page(doc(f'<div class="footer"><p>{a}</p></div><p>{b}</p>'))
    baseline_page(doc(f'<p>{a}</p><footer><p>{b}</p></footer>'))
    baseline_page(doc(f"<p>{a}</p><p>x<b>bold</b><i>it</i>y {b}</p>"))
    baseline_page(doc(f"<p>{a}</p><p>{b}</p><table><tr><td>{exact(1293, 90)}</td></tr></table>"))
    baseline_page(doc("<p>" + (a + " ") * 3 + "</p>"))
    baseline_page(doc(f"<p>{a.replace(' ', chr(10), 3)}</p><p>{b}</p>"))
    baseline_page(doc(f"<p>{a.replace(' ', chr(9), 2)}</p><p>{b.replace(' ', '&nbsp;', 2)}</p>"))
    baseline_page(doc(f"<p>{a}\N{NO-BREAK SPACE}{b}</p>"))
    baseline_page(doc(f"<p>{a}\N{IDEOGRAPHIC SPACE}{b}</p>"))
    baseline_page(doc(f"<p>{a}\N{ZERO WIDTH SPACE}{b}</p>"))


def dump_pages() -> None:
    "The last tiers: the text of the whole body, and `html2txt` on markup that carries no whitespace."
    for body in (
        "<div>" + exact(1300, 60) + "</div><div>" + exact(1301, 60) + "</div>",
        "<span>" + exact(1302, 60) + "</span><span>" + exact(1303, 60) + "</span>",
        "<b>" + exact(1304, 60) + "</b><i>" + exact(1305, 60) + "</i>",
        exact(1306, 60) + "<br>" + exact(1307, 60), exact(1308, 60) + "<hr>" + exact(1309, 60),
        "<h1>" + exact(1310, 60) + "</h1><h2>" + exact(1311, 60) + "</h2>",
        "<ul><li>" + exact(1312, 40) + "</li><li>" + exact(1313, 40) + "</li></ul>",
        "<table><tr><td>" + exact(1314, 40) + "</td><td>" + exact(1315, 40) + "</td></tr></table>",
        "<dl><dt>" + exact(1316, 40) + "</dt><dd>" + exact(1317, 40) + "</dd></dl>",
        "<figure><figcaption>" + exact(1318, 70) + "</figcaption></figure>" + exact(1319, 60),
        "<details><summary>" + exact(1320, 70) + "</summary>" + exact(1321, 60) + "</details>",
        "<nav>" + exact(1322, 80) + "</nav><main>" + exact(1323, 80) + "</main><header>x</header>",
        "<aside>" + exact(1324, 80) + "</aside>" + exact(1325, 80),
        "<footer>" + exact(1326, 80) + "</footer>" + exact(1327, 80),
        '<div class="site-footer">' + exact(1328, 80) + "</div>" + exact(1329, 80),
        '<div id="footer">' + exact(1330, 80) + "</div>" + exact(1331, 80),
        '<div class="Footer">' + exact(1332, 80) + "</div>" + exact(1333, 80),
        '<div class="cookie-banner">' + exact(1334, 80) + "</div>" + exact(1335, 80),
        '<div class="CookieConsent">' + exact(1336, 80) + "</div>" + exact(1337, 80),
        '<div id="onetrust-banner-sdk">' + exact(1338, 80) + "</div>" + exact(1339, 80),
        '<div class="cmplz-cookiebanner">' + exact(1340, 80) + "</div>" + exact(1341, 80),
        '<div class="borlabs-cookie">' + exact(1342, 80) + "</div>" + exact(1343, 80),
        '<div class="cookies-not-set">' + exact(1344, 80) + "</div>" + exact(1345, 80),
        '<div class="consent-manager">' + exact(1346, 80) + "</div>" + exact(1347, 80),
        '<div class="notice-cookie">' + exact(1348, 80) + "</div>" + exact(1349, 80),
        '<div class="moove-gdpr">' + exact(1350, 80) + "</div>" + exact(1351, 80),
        '<p class="cookie_policy">' + exact(1352, 80) + "</p>" + exact(1353, 80),
        "<svg><text>" + exact(1354, 80) + "</text></svg>" + exact(1355, 80),
        "<template><p>" + exact(1356, 80) + "</p></template>" + exact(1357, 80),
        "<fencedframe>" + exact(1358, 80) + "</fencedframe>" + exact(1359, 80),
        "<style>p {}</style><script>var a;</script>" + exact(1360, 80),
        "<pre>  " + exact(1361, 60) + "  \n  " + exact(1362, 60) + "</pre>",
        "<textarea>" + exact(1363, 60) + "</textarea>" + exact(1364, 60),
        "<select><option>" + exact(1365, 30) + "</option><option>" + exact(1366, 30) + "</option></select>",
        "<noscript>" + exact(1367, 60) + "</noscript>" + exact(1368, 60),
        "<p>" + exact(1373, 30) + "\N{NO-BREAK SPACE}\N{IDEOGRAPHIC SPACE}\N{LINE SEPARATOR}"
        + exact(1374, 30) + "</p>",
        "<p>" + exact(1375, 30) + "\x85" + exact(1376, 30) + "</p>",
        "   \n  <div>   " + exact(1377, 60) + "   </div>  \n ",
    ):
        baseline_page(doc(body))
    # a document without a body, and the empty load
    baseline_page("")
    baseline_page("<html><head><title>t</title></head></html>")
    baseline_page("<html><body></body></html>")
    baseline_page("<html><body> </body></html>")
    baseline_page("<!DOCTYPE html><html><body><p>" + exact(1381, 150) + "</p></body></html>")
    baseline_page("<HTML><BODY><P>" + exact(1382, 150) + "</P></BODY></HTML>")


def baseline_pages() -> None:
    for builder in (json_grammar_pages, surrogate_pages, discourse_pages, structure_pages, markup_pages, type_pages,
                    teaser_pages, article_pages, paragraph_pages, dump_pages):
        builder()


# ---------------------------------------------------------------- running upstream


def make_extractor(output_format: str, options: Options):
    from trafilatura.settings import Extractor

    extractor = Extractor(
        output_format=output_format, fast=True, dedup=False, tables=options.tables, images=options.images,
        links=options.links, formatting=options.formatting, precision=options.focus == "precision",
        recall=options.focus == "recall", comments=options.comments, url=options.url,
    )
    assert extractor.focus == options.focus and extractor.fast and not extractor.dedup
    return extractor


def run_extract(html: str, output_format: str, options: Options, trace: Trace | None = None):
    "('Ok', text or None), or ('Err', exception class)."
    from trafilatura import extract

    extractor = make_extractor(output_format, options)
    try:
        if trace is None:
            text = extract(html, options=extractor)
        else:
            with tracing(trace):
                text = extract(html, options=extractor)
    except Exception as error:
        return "Err", type(error).__name__
    if text is not None and output_format == "html":
        text = rewrite_namespaces(text)
    return "Ok", text


def run_baseline(html: str):
    "(('Ok', (text, length)) or ('Err', name), html2txt with cleaning)."
    from trafilatura.baseline import baseline, html2txt

    try:
        _, text, length = baseline(html)
        found = ("Ok", (text, length))
    except Exception as error:
        found = ("Err", type(error).__name__)
    return found, html2txt(html)


# ---------------------------------------------------------------- the case sets


def snippet_pages() -> list[tuple[str, Options]]:
    "Every distinct (page, options) of the main-extractor html cases; they are all full documents."
    main_cases.build()
    seen: set = set()
    pages = []
    for item in main_cases.HTML_CASES:
        key = (item.html, item.options)
        if key in seen:
            continue
        seen.add(key)
        assert item.html.startswith("<html"), item.html[:80]
        pages.append((item.html, item.options))
    return pages


def format_sets(options: Options) -> list[tuple[str, Options, tuple[str, Options] | None]]:
    """The runs of one page with its own options: the three formats; then, in `txt`, `precision`, `recall` and
    comments off, which change the cascade and not the output; then Markdown with formatting on, its default.
    The third item is the run Markdown with formatting on is dropped for when it expects the same."""
    runs = [(output_format, options, None) for output_format in FORMATS]
    for focus in ("precision", "recall"):
        if options.focus != focus:
            runs.append(("txt", replace(options, focus=focus), None))
    if options.comments:
        runs.append(("txt", replace(options, comments=False), None))
    if not options.formatting:
        runs.append(("markdown", replace(options, formatting=True), ("markdown", options)))
    return runs


@dataclass
class ExtractCase:
    html: str
    output_format: str
    options: Options
    expected: tuple[str, str | None]
    kind: str


def rust_expected(expected: tuple[str, str | None]) -> str:
    kind, value = expected
    if kind == "Err":
        return f"Err({rust_str(value)})"
    return "Ok(None)" if value is None else f"Ok(Some({rust_str(value)}))"


def rust_baseline(found) -> str:
    kind, value = found
    if kind == "Err":
        return f"Err({rust_str(value)})"
    text, length = value
    return f"Ok(({rust_str(text)}, {length}))"


HEADER = """\
// @generated by tools/parity/parity/gen_extraction_cases.py. Do not edit. Regenerate from the repository root with:
//   scripts/oracle.sh 'from parity.gen_extraction_cases import main; main()' > tests/extraction_cases/cases.rs
// Upstream's extract, baseline and html2txt on whole pages, with fast=True and dedup=False.

use crate::trees::CaseOptions;

/// What `extract` returns: the text, `None` for upstream's `None`, or the exception class that escapes.
pub type Expected = Result<Option<&'static str>, &'static str>;

/// A page through `extract` in `format` (`txt`, `markdown` or `html`) with `options`.
pub struct ExtractCase {
    pub html: &'static str,
    pub format: &'static str,
    pub options: CaseOptions,
    pub expected: Expected,
}

/// A page through `baseline` and `html2txt` (with cleaning).
pub struct BaselineCase {
    pub html: &'static str,
    /// `baseline(html)`: the text and its length, or the exception class that escapes.
    pub baseline: Result<(&'static str, usize), &'static str>,
    pub html2txt: &'static str,
}
"""


def main() -> None:
    from parity.generate import html5_parsers

    targeted_pages()
    baseline_pages()
    snippets = snippet_pages()
    cases: list[ExtractCase] = []
    results: dict = {}
    failures: list[str] = []

    def add(kind: str, html: str, output_format: str, options: Options, trace: Trace | None = None,
            unless_same_as: tuple[str, Options] | None = None) -> None:
        """Runs upstream and keeps the case unless the same page, format and options are already there, or, with
        `unless_same_as` (a format and options of this page), unless it expects what that run does."""
        key = (html, output_format, options)
        if key in results:
            return
        results[key] = run_extract(html, output_format, options, trace)
        if unless_same_as is not None and results.get((html, *unless_same_as)) == results[key]:
            return
        cases.append(ExtractCase(html, output_format, options, results[key], kind))

    with html5_parsers():
        for html, options in snippets:
            for output_format, run_options, same_as in format_sets(options):
                add("snippet", html, output_format, run_options, unless_same_as=same_as)
        # the traced run of each page first: another page's variants must not take its place
        for item in TARGETS:
            trace = Trace()
            add("targeted", item.html, "txt", item.options, trace)
            print(f"{item.name}: {trace.line()}", file=sys.stderr)
            if item.check is not None and not item.check(trace):
                failures.append(item.name)
            if item.text is not None:
                marker, present = item.text
                output = results[(item.html, "txt", item.options)][1]
                if output is None or (marker in output) != present:
                    failures.append(f"{item.name}: output {'lacks' if present else 'holds'} {marker[:30]!r}")
        for item in TARGETS:
            for output_format, run_options, same_as in format_sets(item.options)[1:]:
                add("targeted", item.html, output_format, run_options, unless_same_as=same_as)
        baseline_cases = []
        for html in dict.fromkeys(BASELINE_PAGES):
            found, text = run_baseline(html)
            baseline_cases.append((html, found, text))
            add("baseline", html, "txt", Options())
            # Markdown differs from TXT only where it escapes or formats
            add("baseline", html, "markdown", Options(formatting=True), unless_same_as=("txt", Options()))
    if failures:
        raise AssertionError(f"targeted pages that miss their branch or their output: {failures}")
    for case in cases:
        assert case.html == "" or case.html[:9].lower().startswith(("<html", "<!doctype")), case.html[:80]

    page_names: dict[str, str] = {}
    option_names: dict[Options, str] = {}
    for case in cases:
        page_names.setdefault(case.html, f"P{len(page_names)}")
        option_names.setdefault(case.options, f"O{len(option_names)}")
    for html, _, _ in baseline_cases:
        page_names.setdefault(html, f"P{len(page_names)}")

    out = [HEADER, "#[rustfmt::skip]", "mod pages {"]
    out += [f"    pub const {name}: &str = {rust_str(html)};" for html, name in page_names.items()]
    out += ["}", "", "use pages::*;", ""]
    out += [f"#[rustfmt::skip]\nconst {name}: CaseOptions = {rust_options(options)};"
            for options, name in option_names.items()]
    out += ["", "#[rustfmt::skip]", "pub const EXTRACT_CASES: &[ExtractCase] = &["]
    for case in cases:
        out += [
            "    ExtractCase {",
            f"        html: {page_names[case.html]},",
            f"        format: {rust_str(case.output_format)},",
            f"        options: {option_names[case.options]},",
            f"        expected: {rust_expected(case.expected)},",
            "    },",
        ]
    out += ["];", "", "#[rustfmt::skip]", "pub const BASELINE_CASES: &[BaselineCase] = &["]
    for html, found, text in baseline_cases:
        out += [
            "    BaselineCase {",
            f"        html: {page_names[html]},",
            f"        baseline: {rust_baseline(found)},",
            f"        html2txt: {rust_str(text)},",
            "    },",
        ]
    out += ["];", ""]
    sys.stdout.write("\n".join(out))

    kinds: dict[str, int] = {}
    outcomes: dict[str, int] = {}
    by_format: dict[str, int] = {}
    for case in cases:
        kinds[case.kind] = kinds.get(case.kind, 0) + 1
        outcome = case.expected[0] if case.expected[0] == "Err" or case.expected[1] is not None else "None"
        outcomes[outcome] = outcomes.get(outcome, 0) + 1
        by_format[case.output_format] = by_format.get(case.output_format, 0) + 1
    errors = sum(found[0] == "Err" for _, found, _ in baseline_cases)
    print(f"{len(cases)} extract cases {kinds}, formats {by_format}, outcomes {outcomes}; {len(snippets)} snippet pages, "
          f"{len(TARGETS)} targeted pages; {len(baseline_cases)} baseline cases ({errors} raising); "
          f"{len(page_names)} distinct pages, {len(option_names)} option sets", file=sys.stderr)


if __name__ == "__main__":
    main()
