"""Generates src/html_processing/cases.rs: upstream's tree-mutating `htmlprocessing` functions run on HTML snippets.

Regenerate with (from the repository root, Git Bash or Linux; Docker on Windows and macOS):

    scripts/oracle.sh 'from parity.gen_html_processing_cases import main; main()' > src/html_processing/cases.rs

Each snippet is parsed with `parity.html5tree.parse_document`; `op` runs on the document root (or on the
first element named `scope`) with the options of the case, and the result is written as a
`treedump.dump_tree`, next to the dump of the input so the Rust test can tell a parser difference from a
function difference. The result is the document root, except for `tree_cleaning`, which returns the root to
continue with (the recall copy, when it is restored).

`op` is one of:

    tree_cleaning[:+tag|-tag ...]    MANUALLY_CLEANED with `tag` added (`+p`) or removed (`-form`)
    prune_html
    convert_tags
    prune_unwanted_nodes:LIST[:backup]
    delete_by_link_density:TAG:BACKTRACKING:FAVOR_PRECISION     the two flags are `true` or `false`

A case may state whether the operation must change the tree; the generator fails when it does not, so a
snippet that stops exercising its branch is not left behind silently.
"""

import sys
from dataclasses import dataclass

from parity.gen_xpath_cases import rust_str


@dataclass
class Case:
    html: str
    op: str
    changes: bool | None  # whether dump_out must differ from dump_in; None = not stated
    scope: str
    tables: bool
    images: bool
    links: bool
    formatting: bool
    focus: str


CASES: list[Case] = []


def case(html: str, op: str, *, changes: bool | None = None, scope: str = "", tables: bool = True,
         images: bool = False, links: bool = False, formatting: bool = False, focus: str = "balanced") -> None:
    CASES.append(Case(html, op, changes, scope, tables, images, links, formatting, focus))


def page(body: str, head: str = "<title>t</title>") -> str:
    return f"<html><head>{head}</head><body>{body}</body></html>"


def filler(length: int, char: str = "b") -> str:
    return char * length


def link(length: int, char: str = "a") -> str:
    return f"<ref>{char * length}</ref>"


# ---------------------------------------------------------------- tree_cleaning


def cleaning_cases() -> None:
    from trafilatura.settings import MANUALLY_CLEANED, MANUALLY_STRIPPED

    clean = "tree_cleaning"
    # Parser-sensitive tags get their own markup; everything else sits between two paragraphs.
    frames = '<html><head><title>t</title></head><frameset><frame src="a"></frameset></html>'
    special = {
        "frame": frames,
        "frameset": frames,
        # html5ever lets a select hold a b, html5lib flattens it to text
        "select": page("<div><p>before</p><select><option>x</option></select>tail<p>after</p></div>"),
        "head": page("<p>x</p>", '<title>t</title><meta charset="utf-8"><style>p{}</style>'),
    }
    for tag in MANUALLY_CLEANED:
        if tag == "form":
            continue  # handled below, once the rest of the noise is gone
        if tag in special:
            case(special[tag], clean, changes=True)
            continue
        markup = f"<{tag}>x<b>y</b></{tag}>"
        case(page(f"<div><p>before</p>{markup}tail<p>after</p></div>"), clean)
    # a deleted element as first child gives its tail to the parent text, as a later one to its sibling
    case(page("<div><aside>x</aside>tail<p>after</p></div>"), clean, changes=True)
    case(page("<div><p>a</p><aside>x</aside><p>after</p></div>"), clean, changes=True)

    for tag in MANUALLY_STRIPPED:
        if tag in ("tbody", "thead", "tfoot"):
            case(page(f"<table><{tag}><tr><td>cell</td></tr></{tag}></table><p>after</p>"), clean, changes=True)
        else:
            case(page(f"<div><p>a<{tag}>inner<b>x</b></{tag}>b</p></div>"), clean)
    case(page("<div><p>a<font>b</font><small>c</small><mark>d</mark>e</p></div>"), clean, changes=True)

    # tables on and off, ARIA layout tables, a figure holding a table (#301)
    table = "<table><tr><th>h</th><td>d</td></tr></table>"
    case(page(f"<p>a</p>{table}<p>b</p>"), clean, tables=True, changes=True)
    case(page(f"<p>a</p>{table}<p>b</p>"), clean, tables=False, changes=True)
    for role in ("presentation", "none", "grid"):
        case(page(f'<table role="{role}"><tr><td>x</td></tr></table>'), clean, changes=True)
        case(page(f'<table role="{role}"><tr><td>x</td></tr></table>'), clean, tables=False, changes=True)
    case(page(f"<figure>{table}<figcaption>c</figcaption></figure>"), clean, changes=True)
    case(page(f"<figure>{table}<figcaption>c</figcaption></figure>"), clean, tables=False, changes=True)
    case(page("<figure><img src='a.png'><figcaption>c</figcaption></figure><p>t</p>"), clean, changes=True)
    case(page("<figure><figure>x</figure><table><tr><td>t</td></tr></table></figure>"), clean, changes=True)

    # images on: figure, picture and source are kept, img is not stripped
    figure = "<figure><img src='a.png' alt='x'><figcaption>cap</figcaption></figure>"
    picture = "<picture><source srcset='a.webp'><img src='a.png'></picture>"
    case(page(f"<p>t</p>{figure}{picture}"), clean, images=True, changes=True)
    case(page(f"<p>t</p>{figure}{picture}"), clean, images=False, changes=True)
    case(page(f"<figure>{table}<img src='a.png'></figure>"), clean, images=True, changes=True)
    case(page("<p>a<img src='a.png'>b</p>"), clean, images=True, changes=None)
    case(page("<p>a<img src='a.png'>b</p>"), clean, images=False, changes=True)
    case(page("<video><source src='a.mp4'></video><p>a</p>"), clean, images=True, changes=True)

    # prune_html at the end of tree_cleaning: an empty element goes, its tail only outside precision
    empty = "<div><p></p>tail<p>keep</p></div>"
    for focus in ("balanced", "precision", "recall"):
        case(page(empty), clean, focus=focus, changes=True)
        case(page("<div><p>keep</p><span></span>tail</div>"), clean, focus=focus, changes=True)

    # recall: the copy is made when no paragraph can survive, and restored when none did
    case(page("<aside><p>text</p></aside>"), clean, focus="recall", changes=None)
    case(page("<aside><p>text</p></aside>"), clean, focus="balanced", changes=True)
    case(page("<aside><p>text</p></aside>"), clean, focus="precision", changes=True)
    case(page("<aside><p>text</p></aside><p>more</p>"), clean, focus="recall", changes=True)
    case(page("<aside>text</aside>"), clean, focus="recall", changes=True)
    case(page("<div><nav><p>a</p></nav><footer><p>b</p></footer></div>"), clean, focus="recall")
    case(page("<form><p>a</p></form>"), clean, focus="recall")
    case(page("<div><aside><p>a</p></aside><span>x</span></div>"), clean, focus="recall")
    case(page("<aside><p></p></aside><div><p></p></div>"), clean, focus="recall")
    case(page("<aside><p>text</p></aside>"), "tree_cleaning:+p", focus="recall")
    case(page("<aside><p>text</p></aside><p>other</p>"), "tree_cleaning:+p", focus="recall")
    case(page("<div><p>a</p><aside>b</aside></div>"), "tree_cleaning:+p", focus="recall")
    case(page("<div><p>a</p><aside>b</aside></div>"), "tree_cleaning:+p", focus="balanced", changes=True)
    case(page("<div><span>x</span></div><aside>b</aside>"), "tree_cleaning:+p", focus="recall", changes=True)
    # a skipped deletion can leave a paragraph in a tree whose copy was made: no restore then
    case(page("<aside><aside>x</aside></aside><aside><p>b</p></aside>"), clean, focus="recall")
    case(page("<aside>x</aside><aside><p>b</p></aside>"), clean, focus="recall")
    # the walk of a deleted p ends there, so a later p can survive and the copy stays unused
    for focus in ("balanced", "recall"):
        case(page("<div><p>a</p><p>b</p></div>"), "tree_cleaning:+p", focus=focus)
        case(page("<p>a</p><div><p>b</p></div>"), "tree_cleaning:+p", focus=focus)

    # forms: a layout wrapper holding more than half of the text becomes a div, a widget is deleted
    case(page("<form>aaaa</form>bbbb"), clean, changes=True)
    case(page("<form>aaaaa</form>bbbb"), clean, changes=True)
    case(page("<form>aaa</form>bbbbb"), clean, changes=True)
    case(page("<form></form>"), clean, changes=True)
    case(page("<form></form>text"), clean, changes=True)
    case(page("<form><p>wrapper text that is long</p></form><p>short</p>"), clean, changes=True)
    case(page("<form id='search'><input name='q'>find</form><p>an article paragraph of text</p>"), clean, changes=True)
    case(page("<form>aa</form><p>bb</p><form>ccccccc</form>"), clean, changes=True)
    case(page("<form><aside>zzzzzzzzzz</aside>aaaa</form>bbbbb"), clean, changes=True)
    case(page("<form>aaaaa</form>bbbb<script>cccccccccccc</script>"), clean, changes=True)
    case(page("<form>aaaa</form>bbbb"), "tree_cleaning:-form", changes=True)
    case(page("<form>aaaa</form>bbbb<aside>x</aside>"), "tree_cleaning:-form", changes=True)

    # lxml's live walk skips what follows a deleted subtree
    case(page("<aside><aside>in</aside></aside><aside>after</aside><p>x</p>"), clean, changes=True)
    case(page("<div><nav><nav>1</nav></nav><nav>2</nav><nav>3</nav></div><p>x</p>"), clean, changes=True)
    case(page("<aside>a<aside>b<aside>c</aside></aside></aside><aside>d</aside><aside>e</aside>"), clean, changes=True)

    # namespaces (D-2): MathML and SVG survive, with their LaTeX unrecovered
    mixed = '<p>a <math alttext="y_1"><mi>y</mi></math> b<svg><title>t</title></svg></p>'
    case(page(mixed), clean, changes=None)
    case(page(mixed), clean, tables=False, changes=None)
    case(page('<p>a <math display="block"><semantics><mi>x</mi><annotation encoding="application/x-tex">x^2</annotation></semantics></math> b</p>'), clean, changes=None)
    case(page('<div><svg><g><path d="M0"></path></g></svg></div><svg><style>p{}</style></svg>'), clean, changes=None)
    case(page('<p>a</p><math><mi>q</mi></math><aside>x</aside>'), clean, changes=True)


# ---------------------------------------------------------------- prune_html


def prune_html_cases() -> None:
    from trafilatura.settings import CUT_EMPTY_ELEMS

    for tag in sorted(CUT_EMPTY_ELEMS):
        case(page(f"<div><p>keep</p><{tag}></{tag}>tail<span>x</span></div>"), "prune_html", changes=True)
        if tag in ("div", "li", "strong"):
            case(page(f"<div><{tag}></{tag}>tail<p>keep</p></div>"), "prune_html", changes=True, focus="precision")
    for focus in ("balanced", "precision", "recall"):
        case(page("<div><p></p>tail</div>"), "prune_html", focus=focus, changes=True)
        case(page("<div>text<p></p>tail<p></p>more</div>"), "prune_html", focus=focus, changes=True)
        case(page("<div><p></p></div>"), "prune_html", focus=focus, changes=True)
        case(page("<p>a</p><div></div>"), "prune_html", focus=focus, changes=True)
    # kept: text (even blank), children, an element outside the list
    case(page("<div><p> </p><p>x</p><p><b>y</b></p><a></a><td></td><em></em>t</div>"), "prune_html", changes=True)
    case(page("<div><p> </p><p>x</p><p><b>y</b></p><a></a>t</div>"), "prune_html", changes=False)
    case(page("<div><div></div></div>"), "prune_html", changes=True)
    case(page("<div><div><div></div></div></div>tail"), "prune_html", changes=True)
    case(page("<ul><li></li><li>a</li><li></li></ul>"), "prune_html", changes=True)
    case(page("<div id='r'></div>"), "prune_html", scope="div", changes=False)
    case(page("<div id='r'><p></p>x</div>"), "prune_html", scope="div", changes=True)
    case(page("<div><span>a</span><svg><title></title></svg><math><mi></mi></math></div>"), "prune_html", changes=False)


# ---------------------------------------------------------------- prune_unwanted_nodes


def pruning_cases() -> None:
    def prune(lst: str, *, backup: bool = False) -> str:
        return f"prune_unwanted_nodes:{lst}" + (":backup" if backup else "")

    overall = "OVERALL_DISCARD_XPATH"
    # each list with one matching element, with and without the guard; the tail text stays
    samples = {
        overall: '<div role="navigation">nav</div>',
        "RAW_TREE_PRUNE_XPATH": '<div class="infinite-scroll"><p>more</p></div>',
        "TEASER_DISCARD_XPATH": '<div class="teaser">t</div>',
        "PRECISION_DISCARD_XPATH": '<header>h</header>',
        "DISCARD_IMAGE_ELEMENTS": '<div class="caption">c</div>',
        "REMOVE_COMMENTS_XPATH": '<div class="comments">c</div>',
        "REMOVE_COMMENTS_AND_LISTS_XPATH": '<ul class="comments"><li>c</li></ul>',
        "COMMENTS_DISCARD_XPATH": '<div id="respond">r</div>',
    }
    for lst, sample in samples.items():
        body = f"<p>keep this paragraph of text here</p>{sample}tail<p>and this one too</p>"
        case(page(body), prune(lst), changes=True)
        case(page(body), prune(lst, backup=True), changes=True)
        case(page(body), prune(lst), scope="body", changes=True)
        case(page("<p>nothing to prune</p>"), prune(lst), changes=False)
    # first child gives its tail to the parent, a later one to its previous sibling
    case(page('<div><div role="navigation">n</div>tail<p>a</p></div>'), prune(overall), changes=True)
    case(page('<div><p>a</p><div role="navigation">n</div>tail</div>'), prune(overall), changes=True)
    # a list holding two selectors: the second sees the tree the first left
    case(page('<div aria-hidden="true"><div role="navigation">n</div></div><p>a</p>'), prune(overall), changes=True)
    case(page('<div role="navigation"><div aria-hidden="true">n</div></div><p>a</p>'), prune(overall), changes=True)
    case(page('<div role="navigation">a<div role="navigation">b</div></div><p>c</p>'), prune(overall), changes=True)
    case(page('<div class="teaser">a<div class="teaser">b</div></div><p>c</p>'), prune("TEASER_DISCARD_XPATH"), changes=True)
    case(page('<div class="x"><p>a</p></div><header>h</header>'), prune("PRECISION_DISCARD_XPATH"), changes=True)

    # the guard: nothing is pruned unless more than a seventh of the text stays; the text is measured on
    # the body here so the lengths below are the whole text
    def widget(length: int, char: str = "w") -> str:
        return f'<div aria-hidden="true">{char * length}</div>'

    for kept, removed in [(10, 60), (11, 59), (9, 61), (10, 61), (11, 60), (12, 59), (1, 5), (1, 6), (0, 10), (5, 0)]:
        body = f"<p>{filler(kept, 'k')}</p>{widget(removed)}"
        case(page(body), prune(overall, backup=True), scope="body")
        case(page(body), prune(overall), scope="body")
    # an element matched by two selectors counts once (dict.fromkeys), a nested match with its outer one
    both = '<div role="navigation" aria-hidden="true">' + filler(60, "w") + "</div>"
    case(page(f"<p>{filler(11, 'k')}</p>{both}"), prune(overall, backup=True), scope="body", changes=True)
    case(page(f"<p>{filler(10, 'k')}</p>{both}"), prune(overall, backup=True), scope="body", changes=False)
    nested = f'<div aria-hidden="true">{filler(30, "o")}<div aria-hidden="true">{filler(29, "n")}</div></div>'
    case(page(f"<p>{filler(11, 'k')}</p>{nested}"), prune(overall, backup=True), scope="body", changes=True)
    nested = f'<div aria-hidden="true">{filler(30, "o")}<div aria-hidden="true">{filler(30, "n")}</div></div>'
    case(page(f"<p>{filler(10, 'k')}</p>{nested}"), prune(overall, backup=True), scope="body", changes=False)
    case(page(f'<p>{filler(20, "k")}</p><div role="navigation">{filler(5)}</div>{widget(5)}'), prune(overall, backup=True), scope="body", changes=True)
    # two separate matches add up
    case(page(f'<p>{filler(12, "k")}</p>{widget(30)}{widget(30)}'), prune(overall, backup=True), scope="body", changes=True)
    case(page(f'<p>{filler(10, "k")}</p>{widget(30)}{widget(31)}'), prune(overall, backup=True), scope="body", changes=False)
    # tails and nesting under the guard
    case(page(f'<div><p>{filler(30, "k")}</p>{widget(5)}tail</div>'), prune(overall, backup=True), changes=True)
    case(page('<div id="r"><div role="navigation">n</div><p>keep</p></div>'), prune(overall), scope="div", changes=True)
    case(page('<div id="r" role="navigation"><p>keep</p></div>'), prune(overall), scope="div", changes=False)
    case(page('<div id="r" role="navigation"><p>keep</p></div>'), prune(overall, backup=True), scope="div", changes=False)


# ---------------------------------------------------------------- delete_by_link_density


def density_cases() -> None:
    def delete(tag: str, *, backtracking: bool = False, precision: bool = False) -> str:
        return f"delete_by_link_density:{tag}:{str(backtracking).lower()}:{str(precision).lower()}"

    def element(tag: str, content: str, nxt: bool, tail: str = "") -> str:
        return f"<{tag}>{content}</{tag}>{tail}" + ("<div>keep</div>" if nxt else "")

    def two_links(total: int) -> str:
        first = total // 2
        return link(first) + link(total - first)

    # limitlen of each (tag, next sibling) combination, one text length below and at it; the other
    # configurations' limits sit between, so swapping two limits flips a case
    limits = {("p", False): 60, ("p", True): 30, ("div", False): 300, ("div", True): 100}
    for (tag, nxt), limit in limits.items():
        for length in (limit - 1, limit):
            case(page(element(tag, two_links(length), nxt)), delete(tag), changes=length < limit)
    for tag in ("p", "div"):
        for nxt in (False, True):
            for length in (45, 70, 150, 200):
                case(page(element(tag, two_links(length), nxt)), delete(tag))
    case(page(element("p", two_links(100), False)), delete("p"), changes=False)
    case(page(element("p", two_links(60), True)), delete("p"), changes=False)

    # the single-link shortcut: a link of more than 100 characters (10 when favoring precision) that is
    # more than 0.9 of the text, in an element over its limitlen
    case(page(element("p", link(101) + filler(4), True)), delete("p"), changes=True)
    case(page(element("p", link(100) + filler(5), True)), delete("p"), changes=False)
    case(page(element("p", link(180) + filler(20), True)), delete("p"), changes=False)
    case(page(element("p", link(181) + filler(19), True)), delete("p"), changes=True)
    case(page(element("p", link(91) + link(90) + filler(19), True)), delete("p"), changes=False)
    case(page(element("p", link(150) + link(10), True)), delete("p"), changes=False)
    for precision in (False, True):
        case(page(element("p", link(31) + filler(2), True)), delete("p", precision=precision))
        case(page(element("p", link(29) + filler(4), True)), delete("p", precision=precision))
        case(page(element("p", link(40) + filler(10), True)), delete("p", precision=precision))
        case(page(element("p", link(33) + filler(1), True)), delete("p", precision=precision))
        case(page(element("div", link(60) + filler(4), True)), delete("div", precision=precision))
    case(page(element("div", link(200) + filler(10), False)), delete("div"), changes=True)

    # in the size gate: linklen above 0.8 of the text, or more than one link of which over 0.8 are short
    case(page(element("div", link(80) + filler(20), False)), delete("div"), changes=False)
    case(page(element("div", link(81) + filler(19), False)), delete("div"), changes=True)
    case(page(element("div", link(3) + filler(197), False)), delete("div"), changes=False)
    case(page(element("div", "".join(link(9) for _ in range(5)) + filler(155), False)), delete("div"), changes=True)
    case(page(element("div", "".join(link(10) for _ in range(5)) + filler(150), False)), delete("div"), changes=False)
    case(page(element("div", "".join(link(9) for _ in range(4)) + link(10) + filler(154), False)), delete("div"), changes=False)
    case(page(element("div", "".join(link(9) for _ in range(5)) + link(10) + filler(145), False)), delete("div"), changes=True)
    case(page(element("div", link(9) + link(9) + filler(182), False)), delete("div"), changes=True)
    case(page(element("div", link(9) + filler(191), False)), delete("div"), changes=False)
    # links without text: elemnum 0 is a hit, a graphic inside keeps the element
    case(page(element("div", "<ref></ref>" + filler(50), False)), delete("div"), changes=True)
    case(page(element("div", "<ref> </ref>" + filler(50), False)), delete("div"), changes=True)
    case(page(element("div", "<ref></ref><ref></ref>" + filler(400), False)), delete("div"), changes=False)
    case(page(element("div", "<ref>" + filler(50, "a") + "<graphic src='a'></graphic></ref>", False)), delete("div"), changes=False)
    case(page(element("div", "<graphic src='a'></graphic>" + link(50), False)), delete("div"), changes=False)
    case(page(element("div", "<ref><graphic src='a'></graphic></ref>" + filler(20), False)), delete("div"), changes=False)
    case(page(element("div", "no links at all here", False)), delete("div"), changes=False)

    # the farm test, over the limitlen: more than four links, linklen above 0.9 of the text and under 100
    # per link, and not a listing of one link per paragraph
    def farm(count: int, each: int, text: int, nxt: bool = True, tag: str = "div") -> str:
        return page(element(tag, link(each) * count + filler(text - count * each), nxt))

    case(farm(5, 30, 150), delete("div"), changes=True)
    case(farm(4, 38, 152), delete("div"), changes=False)
    case(farm(5, 36, 200), delete("div"), changes=False)
    case(farm(5, 37, 200), delete("div"), changes=True)
    case(farm(5, 100, 500), delete("div"), changes=False)
    case(farm(5, 99, 495), delete("div"), changes=True)
    case(farm(6, 99, 594), delete("div"), changes=True)
    case(farm(10, 90, 990), delete("div"), changes=True)
    case(farm(10, 90, 1000), delete("div"), changes=False)
    case(farm(6, 100, 600), delete("div"), changes=False)
    case(farm(5, 30, 150, nxt=False), delete("div"), changes=True)
    case(farm(5, 70, 350, nxt=False), delete("div"), changes=True)
    case(farm(5, 100, 500, nxt=False), delete("div"), changes=False)
    case(farm(5, 14, 70, tag="p"), delete("p"), changes=True)
    case(farm(5, 9, 100, tag="p"), delete("p"), changes=False)
    case(farm(5, 20, 100, tag="div"), delete("div"), changes=True)

    def listing(items: list[str]) -> str:
        return page(element("div", "".join(items), True))

    one = [f"<p>{link(30)}</p>" for _ in range(5)]
    case(listing(one), delete("div"), changes=False)
    case(listing(one[:4] + [f"<p>{link(15)}{link(15)}</p>"]), delete("div"), changes=True)
    case(listing(one[:4] + [link(30)]), delete("div"), changes=True)
    case(listing(one[:4] + [f"<span>{link(30)}</span>"]), delete("div"), changes=True)
    case(listing(one[:4] + [f"<p>{link(30)}tail</p>"]), delete("div"), changes=False)
    case(listing([f"<p>{link(30)}</p>" for _ in range(6)]), delete("div"), changes=False)
    case(listing([f"<p>{link(30)}</p>" for _ in range(4)]), delete("div"), changes=False)

    # backtracking: a short element with links that is not dense goes when it has enough children
    # (3, or 1 when favoring precision) and little text (under 100, or 200)
    def short(children: int, text: int) -> str:
        body = link(5) + "<span>x</span>" * (children - 1)
        return page(element("div", body + filler(text - 5 - (children - 1)), False))

    for children in (1, 2, 3):
        for text in (99, 100, 150, 199, 200):
            for precision in (False, True):
                case(short(children, text), delete("div", backtracking=True, precision=precision))
    case(short(3, 99), delete("div", backtracking=True), changes=True)
    case(short(3, 99), delete("div", backtracking=False), changes=False)
    case(short(2, 99), delete("div", backtracking=True), changes=False)
    case(short(3, 100), delete("div", backtracking=True), changes=False)
    case(short(1, 150), delete("div", backtracking=True, precision=True), changes=True)
    case(short(1, 150), delete("div", backtracking=True, precision=False), changes=False)
    case(short(1, 199), delete("div", backtracking=True, precision=True), changes=True)
    case(short(1, 200), delete("div", backtracking=True, precision=True), changes=False)
    case(page(element("div", link(5) + "<span>x</span>" * 3 + filler(10), True)), delete("div", backtracking=True), changes=True)
    # a non-dense element without links is never short_with_links
    case(page(element("div", "<span>x</span>" * 3 + filler(10), False)), delete("div", backtracking=True), changes=False)

    # other tags, a p under item, td or th is skipped, a div there is not
    case(page(f"<list><item>{link(20)}</item></list><p>keep</p>"), delete("item"), changes=True)
    case(page(f"<list><item>{link(20)}</item></list><p>keep</p>"), delete("list"), changes=True)
    case(page(f"<list><item>{link(20)}</item></list>"), delete("list", backtracking=True), changes=True)
    case(page(f"<quote>{two_links(20)}</quote>tail<p>keep</p>"), delete("quote", precision=True), changes=True)
    case(page(f"<quote>{link(20)}</quote>"), delete("quote", precision=True), changes=True)
    case(page(f"<item><p>{link(20)}</p></item>"), delete("p"), changes=False)
    case(page(f"<table><tr><td><p>{link(20)}</p></td></tr></table>"), delete("p"), changes=False)
    case(page(f"<table><tr><th><p>{link(20)}</p></th></tr></table>"), delete("p"), changes=False)
    case(page(f"<div><p>{link(20)}</p></div>"), delete("p"), changes=True)
    case(page(f"<item><p>{link(20)}</p>tail</item>"), delete("p"), changes=False)
    case(page(f"<item><div>{link(20)}</div></item>"), delete("div"), changes=True)
    case(page(f"<table><tr><td><div>{link(20)}</div></td></tr></table>"), delete("div"), changes=True)
    case(page(f"<item><p>{link(20)}</p><p>{link(20)}</p></item>"), delete("p"), changes=False)
    case(page(f"<div><item><p>{link(20)}</p></item><p>{link(20)}</p></div>"), delete("p"), changes=True)
    case(page(f"<item><p>{link(20)}</p></item>"), delete("item"), changes=True)
    # tails of deleted elements, nested hits
    case(page(f"<div>a<div>{link(30)}</div>tail<div>{link(30)}</div>t2</div><p>x</p>"), delete("div"), changes=True)
    case(page(f"<div><div>{link(30)}</div></div>"), delete("div"), changes=True)
    case(page(f"<p>x</p><div id='r'>{link(30)}</div>"), delete("div"), scope="div", changes=True)
    case(page(f"<div id='r'><p>{link(30)}</p>keep</div>"), delete("p"), scope="div", changes=True)


# ---------------------------------------------------------------- convert_tags


def conversion_cases() -> None:
    conv = "convert_tags"
    from trafilatura.htmlprocessing import CONVERSIONS, REND_TAG_MAPPING

    for tag in CONVERSIONS:
        if tag in ("blockquote", "pre", "q"):
            continue  # below
        if tag in ("dl", "ol", "ul"):
            inner = "<dt>t</dt><dd>d</dd><dt>t2</dt><dd>d2</dd>" if tag == "dl" else "<li>a</li><li>b<b>c</b></li>"
            case(page(f'<{tag} class="k">{inner}</{tag}>tail'), conv, changes=True)
        elif tag == "details":
            case(page("<details><summary>s</summary>body<details><summary>t</summary>x</details></details>"), conv, changes=True)
        elif tag in ("br", "hr"):
            case(page(f"<p>a<{tag} class='k'>b</p>"), conv, changes=True)
        else:
            case(page(f"<div><{tag} class='k' id='i'>text<b>x</b></{tag}>tail</div>"), conv, changes=True)
    case(page("<ul><li>a<ul><li>b</li></ul></li></ul><ol><li>c</li></ol>"), conv, changes=True)
    case(page("<dl><dt>a</dt><dt>b</dt><dd>c</dd><dd>d</dd><dt>e</dt></dl>"), conv, changes=True)
    case(page("<ul><li><ol><li>x</li></ol></li></ul>"), conv, changes=True)
    case(page("<del>a</del><s class='x'>b</s><strike>c</strike>"), conv, changes=True)

    # quotes: a pre is code when it wraps a single span, holds hljs spans or code-like text
    pre = lambda inner, attrs="": f"<div><pre{attrs}>{inner}</pre></div>"
    for markup in (
        pre("<span>x</span>"),
        pre("<span>x</span><span>y</span>"),
        pre("<span>x</span> tail"),
        pre('<span class="hljs-keyword">a</span> <span class="hljs-title" id="z">b</span>'),
        pre('<span class="x hljs">a</span>b<span>c</span>'),
        pre('<b><span class="hljs-a" data-x="y">a</span></b>text'),
        pre("if (x) { y }"),
        pre('call("a")'),
        pre("call('a')"),
        pre("line\n    indented"),
        pre("a\n  two spaces"),
        pre("plain text"),
        pre(""),
        pre("<b>x</b>"),
        pre("a{b"),
        pre("a(b"),
        pre("a\"b"),
        pre("x", " class='k'"),
        "<blockquote cite='u'>quote<b>x</b></blockquote>",
        "<blockquote><pre>{</pre></blockquote>",
        "<q class='k'>short</q>",
        "<p>a <q>b</q> c</p>",
    ):
        case(page(markup), conv, changes=True)

    # links: kept as ref/target, or dropped; only links inside a container stay as ref
    links = '<a href="http://x.org/a" class="k" title="t">link</a>'
    case(page(f"<p>{links}</p>"), conv, links=True, changes=True)
    case(page(f"<p>{links}</p>"), conv, links=False, changes=True)
    for container in ("div", "li", "p", "table"):
        inner = f"<tr><td>{links}</td></tr>" if container == "table" else links
        case(page(f"<{container}>{inner}</{container}>"), conv, links=False, tables=True, changes=True)
        case(page(f"<{container}>{inner}</{container}>"), conv, links=False, tables=False, changes=True)
    case(page(f"<span>{links}</span>"), conv, links=False, changes=True)
    case(page(links), conv, links=False, changes=True)
    case(page(f"<section><article>{links}</article></section>"), conv, links=False, changes=True)
    case(page(f"<section><div><span>{links}</span></div></section>"), conv, links=False, changes=True)
    case(page(f"<table><tr><td>{links}</td></tr></table>"), conv, links=False, tables=False, changes=True)
    case(page(f"<div><table><tr><td>{links}</td></tr></table></div>"), conv, links=False, tables=False, changes=True)
    case(page(f"<ul><li>{links}</li></ul>"), conv, links=False, changes=True)
    case(page(f"<p>a {links} b <a>no href</a> c <a href=''>empty</a></p>"), conv, links=True, changes=True)
    case(page(f"<p>a {links} b <a>no href</a> c <a href=''>empty</a></p>"), conv, links=False, changes=True)
    case(page("<p><a href='/rel'>r</a> <a href='#f'>f</a> <a href='../up'>u</a></p>"), conv, links=True, changes=True)
    case(page("<p><ref href='x' class='k'>kept</ref><a href='y'>a</a></p>"), conv, links=True, changes=True)
    case(page("<p><ref href='x' class='k'>kept</ref><a href='y'>a</a></p>"), conv, links=False, changes=True)
    # the tree root is no container, and only the nearest container counts
    case(page(f'<div id="r">{links}</div>'), conv, links=False, scope="div", changes=True)
    case(page(f'<div id="o"><div id="r">{links}</div></div>'), conv, links=False, scope="div", changes=True)
    case(page(f'<div id="o"><p id="r"><b>{links}</b></p></div>'), conv, links=False, scope="p", changes=True)
    case(page(f'<div id="o"><span id="r">{links}</span></div>'), conv, links=False, scope="span", changes=True)
    case(page(f'<div id="o"><span id="r"><p>{links}</p></span></div>'), conv, links=False, scope="span", changes=True)
    case(page(f'<div id="r">{links}<a>x</a></div>'), conv, links=True, scope="div", changes=True)

    # Yoast FAQ questions, with formatting both ways
    for formatting in (False, True):
        case(page("<p><strong class='a schema-faq-question b' id='q'>Q</strong> text <strong>plain</strong></p>"), conv, formatting=formatting, changes=True)
        case(page("<strong class='schema-faq-answer'>A</strong>"), conv, formatting=formatting, changes=True)
        case(page("<strong class='schema-faq-questions'>A</strong>"), conv, formatting=formatting, changes=True)

    # sub and sup: an empty one goes with its tail kept, one with text or children stays
    case(page("<p>a<sub></sub>tail</p>"), conv, changes=True)
    case(page("<p>a<sup></sup>tail<sup></sup>more</p>"), conv, changes=True)
    case(page("<p><sub></sub>tail</p>"), conv, changes=True)
    case(page("<p>a<sub> </sub>tail</p>"), conv, changes=True)
    case(page("<p>a<sub>x</sub>tail</p>"), conv, changes=True)
    case(page("<p>a<sub><b>x</b></sub>tail</p>"), conv, changes=True)
    case(page("<p>a<sup><i></i></sup>tail</p>"), conv, changes=True)
    for formatting in (False, True):
        case(page("<p>a<sub></sub>tail <sup>2</sup> <sub>3</sub></p>"), conv, formatting=formatting, changes=True)

    # formatting on turns each rend tag into hi, off strips it with its text and tail kept
    for formatting in (False, True):
        for tag in REND_TAG_MAPPING:
            case(page(f"<p>a<{tag} class='k' id='i'>b<br>c</{tag}>d</p>"), conv, formatting=formatting, changes=True)
        case(page("<p><b>a<i>b</i><u>c</u></b><em><strong>d</strong></em></p>"), conv, formatting=formatting, changes=True)

    # images: img becomes graphic; with links on they leave the link, in reverse order of the loop
    for links_on in (False, True):
        for images in (False, True):
            case(page("<p>a<img src='a.png' alt='x'>b</p>"), conv, images=images, links=links_on)
            case(page("<p><a href='u'><img src='a.png'></a></p>"), conv, images=images, links=links_on)
            case(page("<p><a href='u'>text <img src='a.png'><img src='b.png'> more</a> tail</p>"), conv, images=images, links=links_on)
            case(page("<p><a href='u'><img src='a.png'></a>tail</p>"), conv, images=images, links=links_on)
            case(page("<p><a href='u'> <img src='a.png'> </a></p>"), conv, images=images, links=links_on)
            case(page("<p><a href='u'><picture><source srcset='s'><img src='a.png'></picture></a></p>"), conv, images=images, links=links_on)
            case(page("<p><a href='u'><img src='a.png'>t</a><a href='v'><img src='b.png'></a><a href='w'>w</a></p>"), conv, images=images, links=links_on)
            case(page("<div><a href='u'>x</a><a href='v'><img src='c.png'><img src='d.png'><img src='e.png'></a></div>"), conv, images=images, links=links_on)
    case(page("<p><a href='u'></a>x</p>"), conv, images=True, links=True, changes=True)
    case(page("<p><a href='u'> </a>x<a href='v'><img src='a.png'></a></p>"), conv, images=True, links=True, changes=True)
    case(page("<p><ref><img src='a.png'></ref>x</p>"), conv, images=True, links=True, changes=True)
    case(page("<p><ref><img src='a.png'></ref>x</p>"), conv, images=True, links=False, changes=True)

    # namespaces (D-2): elements of an svg or math subtree keep their name, a plain tag test skips them
    case(page('<p>a <svg><a href="u">l</a><strong class="schema-faq-question">q</strong></svg></p>'), conv, links=True)
    case(page('<p>a <svg><a href="u">l</a></svg><a href="v">m</a></p>'), conv, links=False, changes=True)
    case(page('<p>a <math><mi>x</mi><sub></sub><i>y</i></math></p>'), conv, formatting=True, changes=None)
    case(page('<p>a <svg><image href="a.png"></image><img src="b.png"></svg></p>'), conv, images=True, changes=None)
    case(page('<svg><br><h1>x</h1><ul><li>a</li></ul></svg><p>t</p>'), conv, changes=None)
    case(page('<math><mrow><mi>x</mi></mrow></math><p>t</p>'), conv, changes=False)


# ---------------------------------------------------------------- output


def run(item: Case):
    "(dump_in, dump_out) of one case, run on upstream's code."
    from parity import html5tree, treedump
    from trafilatura import htmlprocessing as hp
    from trafilatura import settings, xpaths
    from trafilatura.settings import Extractor

    document = html5tree.parse_document(item.html)
    assert not html5tree.last_parse_normalized(), f"snippet needs the copy rule: {item.html!r}"
    dump_in = treedump.dump_tree(document)
    target = document
    if item.scope:
        target = document.find(f".//{item.scope}")
        assert target is not None, f"no <{item.scope}> in {item.html!r}"
    options = Extractor(
        tables=item.tables, images=item.images, links=item.links, formatting=item.formatting,
        precision=item.focus == "precision", recall=item.focus == "recall",
    )
    assert options.focus == item.focus
    name, *rest = item.op.split(":")
    result = document
    if name == "tree_cleaning":
        assert not item.scope
        edits = rest[0].split(",") if rest else []
        cleaned = settings.MANUALLY_CLEANED
        saved = list(cleaned)  # the list upstream's tree_cleaning reads is mutated in place, as its users do
        try:
            for edit in edits:
                assert edit[0] in "+-", item.op
                cleaned.append(edit[1:]) if edit[0] == "+" else cleaned.remove(edit[1:])
            result = hp.tree_cleaning(document, options)
        finally:
            cleaned[:] = saved
    elif name == "prune_html":
        hp.prune_html(target, item.focus)
    elif name == "convert_tags":
        hp.convert_tags(target, options)
    elif name == "prune_unwanted_nodes":
        hp.prune_unwanted_nodes(target, getattr(xpaths, rest[0]), with_backup=rest[1:] == ["backup"])
    elif name == "delete_by_link_density":
        tag, backtracking, precision = rest
        assert backtracking in ("true", "false") and precision in ("true", "false"), item.op
        hp.delete_by_link_density(target, tag, backtracking == "true", precision == "true")
    else:
        raise AssertionError(item.op)
    return dump_in, treedump.dump_tree(result)


def rust_bool(value: bool) -> str:
    return "true" if value else "false"


def main() -> None:
    for build in (cleaning_cases, prune_html_cases, pruning_cases, density_cases, conversion_cases):
        build()
    out = [
        "// @generated by tools/parity/parity/gen_html_processing_cases.py. Do not edit. Regenerate from the repository root with:",
        "//   scripts/oracle.sh 'from parity.gen_html_processing_cases import main; main()' > src/html_processing/cases.rs",
        "// Upstream's tree-mutating htmlprocessing functions run on HTML snippets: the tree before and after.",
        "",
        "/// The `Extractor` fields a case sets; the others keep their defaults.",
        "pub(super) struct CaseOptions {",
        "    pub tables: bool,",
        "    pub images: bool,",
        "    pub links: bool,",
        "    pub formatting: bool,",
        "    /// `balanced`, `precision` or `recall`.",
        "    pub focus: &'static str,",
        "}",
        "",
        "/// One snippet, one operation and the tree upstream leaves.",
        "pub(super) struct Case {",
        "    pub html: &'static str,",
        "    /// `treedump.dump_tree` of the html5lib tree.",
        "    pub dump_in: &'static str,",
        "    /// Tag of the first element the operation runs on; empty for the document root.",
        "    pub scope: &'static str,",
        "    /// `tree_cleaning[:+tag|-tag]`, `prune_html`, `convert_tags`, `prune_unwanted_nodes:LIST[:backup]` or",
        "    /// `delete_by_link_density:TAG:BACKTRACKING:FAVOR_PRECISION`.",
        "    pub op: &'static str,",
        "    pub options: CaseOptions,",
        "    /// `treedump.dump_tree` of what the operation returns: the document root, or for `tree_cleaning` the",
        "    /// root it returns (the restored copy under recall).",
        "    pub dump_out: &'static str,",
        "}",
        "",
        "#[rustfmt::skip]",
        "pub(super) const CASES: &[Case] = &[",
    ]
    problems: list[str] = []
    seen: set[tuple] = set()
    for item in CASES:
        key = (item.html, item.op, item.scope, item.tables, item.images, item.links, item.formatting, item.focus)
        if key in seen:
            continue
        seen.add(key)
        dump_in, dump_out = run(item)
        if item.changes is not None and (dump_in != dump_out) != item.changes:
            problems.append(f"{item.op} must {'change' if item.changes else 'keep'} {item.html!r}")
        out += [
            "    Case {",
            f"        html: {rust_str(item.html)},",
            f"        dump_in: {rust_str(dump_in)},",
            f"        scope: {rust_str(item.scope)},",
            f"        op: {rust_str(item.op)},",
            "        options: CaseOptions {"
            f" tables: {rust_bool(item.tables)}, images: {rust_bool(item.images)}, links: {rust_bool(item.links)},"
            f" formatting: {rust_bool(item.formatting)}, focus: {rust_str(item.focus)} }},",
            f"        dump_out: {rust_str(dump_out)},",
            "    },",
        ]
    assert not problems, chr(10).join(problems)
    out += ["];", ""]
    sys.stdout.write("\n".join(out))


if __name__ == "__main__":
    main()
