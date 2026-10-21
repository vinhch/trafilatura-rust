import os
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[3]


def _upstream_root() -> Path:
    path = Path(os.environ.get("TRAFILATURA_UPSTREAM") or REPO_ROOT / ".." / "trafilatura").resolve()
    if not (path / "trafilatura" / "utils.py").is_file():
        pytest.exit(
            f"upstream trafilatura checkout not found at {path}; set TRAFILATURA_UPSTREAM or clone it at ../trafilatura",
            returncode=2,
        )
    return path


UPSTREAM = _upstream_root()
sys.path.insert(0, str(UPSTREAM))

from lxml.html import HtmlElement, fromstring  # noqa: E402
from parity import generate, html5tree, treedump  # noqa: E402
from trafilatura import bare_extraction  # noqa: E402
from trafilatura.utils import HTML_PARSER  # noqa: E402


def test_dump_tree_format():
    html = '<html><body><div b="2" a="1">x<!--c--><p> </p>tail</div><svg><path d="M0"/></svg></body></html>'
    root = fromstring(html, parser=HTML_PARSER)
    assert treedump.dump_tree(root) == (
        "html\n"
        "  body\n"
        '    div a="1" b="2"\n'
        '      #text "x"\n'
        "      p\n"
        '        #text " "\n'
        '      #text "tail"\n'
        "    svg@svg\n"
        '      path@svg d="M0"\n'
    )


def test_html5_copy_drops_invalid_names():
    root = html5tree.parse_document('<div @click="x" :href="y" id="z">a&#1;b</div>')
    assert isinstance(root, HtmlElement)
    div = root.find(".//div")
    assert dict(div.attrib) == {"id": "z"}
    assert div.text == "ab"
    assert html5tree.last_parse_normalized() is True


def test_html5_tree_accepted_by_bare_extraction():
    sentence = "The quick brown fox jumps over the lazy dog while the committee reviews the long report. "
    html = (
        "<html><head><title>T</title></head><body><article>"
        f"<table><tr><td>{sentence * 3}</td></tr></table>"
        f"<p>{sentence * 4}<b>bold <i>both</b> italic</i> {sentence * 3}</p>"
        f"<p>{sentence * 4}</p>"
        "</article></body></html>"
    )
    with generate.html5_parsers():
        doc = bare_extraction(html, fast=True)
    assert doc is not None and doc.text.strip()
    assert html5tree.last_parse_normalized() is False


def test_refuses_wrong_commit():
    with pytest.raises(SystemExit) as exc:
        generate.check_pin(UPSTREAM, expected="0" * 40)
    assert "0" * 40 in str(exc.value)
    assert generate.head_commit(UPSTREAM) in str(exc.value)


def test_html5_copy_keeps_root_attributes():
    root = html5tree.parse_document('<html lang="en" class="a"><body><p>x</p></body></html>')
    assert dict(root.attrib) == {"lang": "en", "class": "a"}
