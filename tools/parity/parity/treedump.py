"""Tree dump: the text form in which Python and Rust trees are compared (spec: parity harness)."""

import json
from lxml.html import HtmlElement

SVG_NS = "http://www.w3.org/2000/svg"
MATHML_NS = "http://www.w3.org/1998/Math/MathML"
_NS_TAGS = {SVG_NS: "svg", MATHML_NS: "math"}


def _split(tag: str) -> tuple[str | None, str]:
    if tag.startswith("{"):
        ns, _, local = tag[1:].partition("}")
        return ns, local
    return None, tag


def _json(text: str) -> str:
    return json.dumps(text, ensure_ascii=False)


def dump_tree(root: HtmlElement) -> str:
    """One line per node in document order, two spaces of indent per depth.

    Element: `name[@svg|@math] key="value" ...`, attributes sorted by name, values as JSON strings.
    Text and tail: `#text "..."` at the child depth (text) or the element's own depth (tail);
    whitespace-only text is kept, empty text is omitted. Comments, PIs and the doctype are not dumped.
    The `@svg`/`@math` tag comes from the element's namespace. A tree without any namespaced tag
    (libxml2 yields plain `svg`/`math` elements) is tagged by `svg`/`math` ancestor-or-self instead.
    """
    lines: list[str] = []
    plain = not any(isinstance(el.tag, str) and el.tag.startswith("{") for el in root.iter())

    def walk(el: HtmlElement, depth: int, foreign: str | None) -> None:
        ns, local = _split(el.tag)
        if plain:
            foreign = foreign or (local if local in ("svg", "math") else None)
        else:
            foreign = _NS_TAGS.get(ns)
        pad = "  " * depth
        attrs = "".join(f" {_split(k)[1]}={_json(v)}" for k, v in sorted(el.attrib.items()))
        lines.append(f"{pad}{local}{'@' + foreign if foreign else ''}{attrs}")
        if el.text:
            lines.append(f"{pad}  #text {_json(el.text)}")
        for child in el:
            if not isinstance(child.tag, str):  # comment or PI: skipped, its tail stays
                if child.tail:
                    lines.append(f"{pad}  #text {_json(child.tail)}")
                continue
            walk(child, depth + 1, foreign)
            if child.tail:
                lines.append(f"{pad}  #text {_json(child.tail)}")

    walk(root, 0, None)
    return "\n".join(lines) + "\n"
