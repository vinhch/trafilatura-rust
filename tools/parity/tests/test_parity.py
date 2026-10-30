import json
import logging
import os
import sys
import warnings
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

from parity import import_path  # noqa: E402

import_path(UPSTREAM)

from lxml.html import HtmlElement, fromstring  # noqa: E402
from parity import generate, html5tree, measure, treedump  # noqa: E402
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


def test_html5_parse_drops_leading_bom():
    root = html5tree.parse_document("\ufeff<!DOCTYPE html><html><head><title>t</title></head><body><p>x</p></body></html>")
    assert [child.tag for child in root] == ["head", "body"]
    assert root.find("body").text is None


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


def _files(root) -> dict[str, bytes]:
    return {p.relative_to(root).as_posix(): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def _failing_after(n, fn):
    calls = 0

    def wrapper(*args, **kwargs):
        nonlocal calls
        calls += 1
        if calls > n:
            raise OSError("injected")
        return fn(*args, **kwargs)

    return wrapper


def test_filtered_run_merges_instead_of_wiping(tmp_path):
    pin_dir = tmp_path / "pin"
    meta = {"pin": "p", "upstream_version": "v", "max_date": "d", "lock_sha256": "l"}
    generate.store(pin_dir, meta, [_page("tests/a.html"), _page("tests/b.html")], f1={"f1": 1})
    generate.store(pin_dir, meta, [_page("tests/b.html")], f1=None)
    assert _sources(pin_dir) == ["tests/a.html", "tests/b.html"]
    assert (pin_dir / "pages" / "tests__a.html.json").is_file()
    assert json.loads((pin_dir / "f1.json").read_text()) == {"f1": 1}
    generate.store(pin_dir, meta, [_page("tests/c.html")], f1={"f1": 2})
    assert _sources(pin_dir) == ["tests/c.html"]
    assert not (pin_dir / "pages" / "tests__a.html.json").exists()


@pytest.mark.parametrize("key", ["pin", "upstream_version", "max_date", "lock_sha256"])
def test_filtered_run_with_another_identity_drops_pages_and_f1(tmp_path, key):
    pin_dir = tmp_path / "pin"
    meta = {"pin": "p", "upstream_version": "v", "max_date": "d", "lock_sha256": "l"}
    generate.store(pin_dir, meta, [_page("tests/a.html")], f1={"f1": 1})
    generate.store(pin_dir, {**meta, key: "other"}, [_page("tests/b.html")], f1=None)
    assert _sources(pin_dir) == ["tests/b.html"]
    assert not (pin_dir / "f1.json").exists()


def test_full_run_failure_keeps_the_previous_set(tmp_path, monkeypatch):
    pin_dir = tmp_path / "pin"
    generate.store(pin_dir, {"pin": "p"}, [_page("tests/a.html"), _page("tests/b.html")], f1={"f1": 1})
    before = _files(pin_dir)
    monkeypatch.setattr(generate, "_write_json", _failing_after(1, generate._write_json))
    with pytest.raises(OSError):
        generate.store(pin_dir, {"pin": "p"}, [_page("tests/c.html"), _page("tests/d.html")], f1={"f1": 2})
    assert _files(pin_dir) == before


def test_full_run_clears_leftovers_of_a_crashed_run(tmp_path):
    pin_dir = tmp_path / "pin"
    generate.store(pin_dir, {"pin": "p"}, [_page("tests/a.html")], f1={"f1": 1})
    for leftover in ("pin.new", "pin.old"):
        (tmp_path / leftover).mkdir()
        (tmp_path / leftover / "stray.json").write_text("{}")
    generate.store(pin_dir, {"pin": "p"}, [_page("tests/c.html")], f1={"f1": 2})
    assert _sources(pin_dir) == ["tests/c.html"]
    assert not (tmp_path / "pin.new").exists() and not (tmp_path / "pin.old").exists()
    assert not list(pin_dir.rglob("stray.json"))


def test_filtered_run_failure_keeps_the_index(tmp_path, monkeypatch):
    pin_dir = tmp_path / "pin"
    generate.store(pin_dir, {"pin": "p"}, [_page("tests/a.html")], f1={"f1": 1})
    before = (pin_dir / "index.json").read_bytes()
    write_text = Path.write_text

    def torn(self, text, *args, **kwargs):
        if self.name.startswith("index.json"):
            write_text(self, text[: len(text) // 2], *args, **kwargs)
            raise OSError("injected")
        return write_text(self, text, *args, **kwargs)

    monkeypatch.setattr(Path, "write_text", torn)
    with pytest.raises(OSError):
        generate.store(pin_dir, {"pin": "p"}, [_page("tests/b.html")], f1=None)
    assert (pin_dir / "index.json").read_bytes() == before
    assert json.loads((pin_dir / "pages" / "tests__a.html.json").read_text()) == {"source": "tests/a.html"}


def test_write_json_leaves_no_temp_file_on_failure(tmp_path, monkeypatch):
    def refuse(*_):
        raise OSError("injected")

    monkeypatch.setattr(generate.os, "replace", refuse)
    with pytest.raises(OSError):
        generate._write_json(tmp_path / "x.json", {})
    assert list(tmp_path.iterdir()) == []


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


def test_import_path_supplies_version_metadata_for_a_clean_clone(tmp_path):
    import subprocess
    import sys
    import textwrap

    package = tmp_path / "clone" / "trafilatura"
    package.mkdir(parents=True)
    (package / "__init__.py").write_text('__version__ = "9.8.7"\n', encoding="utf-8")
    script = textwrap.dedent(
        f"""
        from importlib.metadata import version
        from parity import import_path
        import_path({str(tmp_path / "clone")!r})
        print(version("trafilatura"))
        """
    )
    result = subprocess.run([sys.executable, "-I", "-c", f"import sys; sys.path.insert(0, {str(Path(__file__).parents[1])!r}); exec({script!r})"], capture_output=True, text=True, check=False)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "9.8.7"


def _process_state() -> tuple:
    return logging.root.manager.disable, list(warnings.filters)


@pytest.fixture
def clean_logging():
    logging.disable(logging.NOTSET)
    yield
    logging.disable(logging.NOTSET)


def _raise_runtime_error(*_, **__):
    raise RuntimeError("boom")


def test_main_restores_process_state_on_return(tmp_path, monkeypatch, clean_logging):
    monkeypatch.setattr(generate, "check_pin", lambda root: "pin")
    monkeypatch.setattr(generate, "corpus", lambda root: [])
    monkeypatch.setattr(generate, "f1_scores", lambda root: {})
    before = _process_state()
    assert before[0] == logging.NOTSET
    assert generate.main(["--upstream", str(UPSTREAM), "--out", str(tmp_path)]) == 0
    assert _process_state() == before


def test_main_restores_process_state_on_raise(tmp_path, monkeypatch, clean_logging):
    monkeypatch.setattr(generate, "check_pin", lambda root: "pin")
    monkeypatch.setattr(generate, "corpus", lambda root: [])
    monkeypatch.setattr(generate, "f1_scores", _raise_runtime_error)
    before = _process_state()
    assert before[0] == logging.NOTSET
    with pytest.raises(RuntimeError):
        generate.main(["--upstream", str(UPSTREAM), "--out", str(tmp_path)])
    assert _process_state() == before


def test_measure_run_restores_process_state_on_return(tmp_path, monkeypatch, clean_logging):
    monkeypatch.setattr(generate, "check_pin", lambda root: "pin")
    monkeypatch.setattr(generate, "corpus", lambda root: [])
    before = _process_state()
    assert before[0] == logging.NOTSET
    measure.run(UPSTREAM, tmp_path, "")
    assert _process_state() == before


def test_measure_run_restores_process_state_on_raise(tmp_path, monkeypatch, clean_logging):
    monkeypatch.setattr(generate, "check_pin", lambda root: "pin")
    monkeypatch.setattr(generate, "corpus", _raise_runtime_error)
    before = _process_state()
    assert before[0] == logging.NOTSET
    with pytest.raises(RuntimeError):
        measure.run(UPSTREAM, tmp_path, "")
    assert _process_state() == before
