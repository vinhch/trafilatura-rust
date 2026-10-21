import json
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


def test_html5_copy_keeps_first_on_attribute_collision():
    root = html5tree.parse_document('<svg><use href="a" xlink:href="b"/></svg>')
    use = next(el for el in root.iter() if el.tag.endswith("use"))
    assert dict(use.attrib) == {"href": "a"}
    assert html5tree.last_parse_normalized() is True


def test_dump_tree_skips_comments_between_text():
    from lxml.etree import Comment

    root = HtmlElement()
    root.tag = "div"
    root.text = "a"
    comment = Comment("c")
    comment.tail = "b"
    root.append(comment)
    assert treedump.dump_tree(root) == 'div\n  #text "a"\n  #text "b"\n'


def _page(source: str) -> tuple[dict, dict]:
    return {"id": source.replace("/", "__"), "source": source, "valid_utf8": True}, {"source": source}


def _sources(pin_dir) -> list[str]:
    return [p["source"] for p in json.loads((pin_dir / "index.json").read_text())["pages"]]


def test_filtered_run_merges_instead_of_wiping(tmp_path):
    meta = {"pin": "p", "max_date": "d", "lock_sha256": "l"}
    generate.store(tmp_path, meta, [_page("tests/a.html"), _page("tests/b.html")], f1={"f1": 1})
    generate.store(tmp_path, meta, [_page("tests/b.html")], f1=None)
    assert _sources(tmp_path) == ["tests/a.html", "tests/b.html"]
    assert (tmp_path / "pages" / "tests__a.html.json").is_file()
    assert json.loads((tmp_path / "f1.json").read_text()) == {"f1": 1}
    generate.store(tmp_path, meta, [_page("tests/c.html")], f1={"f1": 2})
    assert _sources(tmp_path) == ["tests/c.html"]
    assert not (tmp_path / "pages" / "tests__a.html.json").exists()


def test_filtered_run_drops_pages_and_f1_from_another_lock(tmp_path):
    generate.store(tmp_path, {"lock_sha256": "old"}, [_page("tests/a.html")], f1={"f1": 1})
    generate.store(tmp_path, {"lock_sha256": "new"}, [_page("tests/b.html")], f1=None)
    assert _sources(tmp_path) == ["tests/b.html"]
    assert not (tmp_path / "f1.json").exists()


def test_failing_f1_leaves_the_previous_fixture_set(tmp_path, monkeypatch):
    out = tmp_path / "out"
    monkeypatch.setattr(generate, "check_pin", lambda root: "pin")
    monkeypatch.setattr(generate, "corpus", lambda root: [])
    monkeypatch.setattr(generate, "f1_scores", lambda root: (_ for _ in ()).throw(RuntimeError("f1 failed")))
    generate.store(out / "pin", {"lock_sha256": "old"}, [_page("tests/a.html")], f1={"f1": 1})
    before = (out / "pin" / "index.json").read_text()
    with pytest.raises(RuntimeError):
        generate.main(["--upstream", str(UPSTREAM), "--out", str(out)])
    assert (out / "pin" / "index.json").read_text() == before
    assert json.loads((out / "pin" / "f1.json").read_text()) == {"f1": 1}


def test_check_pin_names_worktrees(tmp_path):
    (tmp_path / ".git").write_text("gitdir: /elsewhere")
    with pytest.raises(SystemExit) as exc:
        generate.check_pin(tmp_path, expected="0" * 40)
    assert "worktree" in str(exc.value)


def test_variant_records_upstream_exception(monkeypatch):
    import trafilatura

    def boom(*_, **__):
        raise KeyError("uppercase")

    monkeypatch.setattr(trafilatura, "extract", boom)
    result = generate.run_variant(b"<html><body><p>x</p></body></html>", True)
    assert result["formats"]["html"] is None
    assert result["errors"]["html"] == "KeyError: 'uppercase'"
