"""HTML5 parse (html5lib) copied into lxml.html elements, the tree the `html5` fixture is built on."""

import re

import html5lib
from lxml.html import HtmlElement, html_parser

# upstream utils.INVALID_XML_CHARS: what lxml refuses in text and attribute values
_INVALID_CHARS = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f￾￿]")
_NAME_START = (
    r"A-Za-z_À-ÖØ-öø-˿Ͱ-ͽͿ-῿‌‍⁰-↏"
    r"Ⰰ-⿯、-퟿豈-﷏ﷰ-�\U00010000-\U000EFFFF"
)
_NAME_CHAR = _NAME_START + r"\-.0-9·̀-ͯ‿-⁀"
# XML Name for elements (libxml2 keeps `o:p`), NCName for attributes (`x-on:click` and `@click` are dropped)
_ELEMENT_NAME = re.compile(rf"[{_NAME_START}:][{_NAME_CHAR}:]*")
_ATTRIBUTE_NAME = re.compile(rf"[{_NAME_START}][{_NAME_CHAR}]*")

_normalized = False


def last_parse_normalized() -> bool:
    "Whether the copy rule changed anything in the most recent parse call."
    return _normalized


def _strip_invalid(text: str) -> str:
    global _normalized
    cleaned = _INVALID_CHARS.sub("", text)
    _normalized |= cleaned != text
    return cleaned


class _Cursor:
    "Append position inside a target element: text goes to the target until a child exists, then to its tail."

    def __init__(self, target: HtmlElement) -> None:
        self.target = target
        self.last: HtmlElement | None = None

    def add_text(self, text: str | None) -> None:
        text = _strip_invalid(text) if text else ""
        if not text:
            return
        if self.last is None:
            self.target.text = (self.target.text or "") + text
        else:
            self.last.tail = (self.last.tail or "") + text


def _copy_attributes(source) -> dict[str, str]:
    "Valid attributes of source, namespaces reduced to the local name; invalid names count as normalized."
    global _normalized
    attrib = {}
    for key, value in source.attrib.items():
        name = key.rpartition("}")[2]
        if not _ATTRIBUTE_NAME.fullmatch(name) or name in attrib:
            # invalid name, or `href` vs `xlink:href` collision: the first one wins
            _normalized = True
        else:
            attrib[name] = _strip_invalid(value)
    return attrib


def _copy_children(source, cursor: _Cursor) -> None:
    global _normalized
    for child in source:
        # comments and PIs have a function as tag in html5lib's etree builder: dropped, tail kept
        if isinstance(child.tag, str):
            namespace, _, local = child.tag[1:].rpartition("}") if child.tag[0] == "{" else ("", "", child.tag)
            if _ELEMENT_NAME.fullmatch(local):
                element = html_parser.makeelement(f"{{{namespace}}}{local}" if namespace else local, _copy_attributes(child))
                cursor.target.append(element)
                cursor.last = element
                inner = _Cursor(element)
                inner.add_text(child.text)
                _copy_children(child, inner)
            else:
                # not representable as an lxml tag: its content replaces it
                _normalized = True
                cursor.add_text(child.text)
                _copy_children(child, cursor)
        cursor.add_text(child.tail)


def _copy_tree(source, tag: str) -> HtmlElement:
    root = html_parser.makeelement(tag, _copy_attributes(source))
    cursor = _Cursor(root)
    cursor.add_text(source.text)
    _copy_children(source, cursor)
    return root


def parse_document(text: str) -> HtmlElement:
    """Parse a full document with the HTML5 algorithm; returns the `html` element.

    A leading U+FEFF is dropped first: WHATWG's decode step discards a BOM, but html5lib keeps it
    when given a str and then starts the body at it (html5ever and libxml2 both skip it).
    """
    global _normalized
    _normalized = False
    tree = html5lib.parse(text.removeprefix("\ufeff"), treebuilder="etree", namespaceHTMLElements=False)
    return _copy_tree(tree, tree.tag)


def parse_fragment(text: str, create_parent: str) -> HtmlElement:
    "Parse a fragment in the context of `create_parent`; returns that parent element holding the result."
    global _normalized
    _normalized = False
    nodes = html5lib.parseFragment(text, container=create_parent, treebuilder="etree", namespaceHTMLElements=False)
    return _copy_tree(nodes, create_parent)
