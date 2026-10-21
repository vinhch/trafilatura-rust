import json
import tarfile
from pathlib import Path

import pytest
from backports import zstd

from parity import pack


def _full_set(root: Path, pin: str = "pin", version: str = "1.0", pages: int = 2) -> Path:
    pin_dir = root / version
    (pin_dir / "pages").mkdir(parents=True)
    ids = [f"tests__{n}.html" for n in range(pages)]
    for page_id in ids:
        (pin_dir / "pages" / f"{page_id}.json").write_text("{}\n")
    (pin_dir / "f1.json").write_text("{}\n")
    index = {"pin": pin, "upstream_version": version, "pages": [{"id": i, "source": i} for i in ids]}
    (pin_dir / "index.json").write_text(json.dumps(index))
    return pin_dir


def test_archive_unpacks_to_the_version_directory(tmp_path):
    pack.pack(_full_set(tmp_path / "src"), tmp_path / "x.tar.zst")
    with zstd.ZstdFile(tmp_path / "x.tar.zst") as compressed, tarfile.open(fileobj=compressed) as tar:
        names = tar.getnames()
    assert "1.0/index.json" in names and "1.0/f1.json" in names and "1.0/pages/tests__0.html.json" in names
    assert all(n == "1.0" or n.startswith("1.0/") for n in names)


def test_validate_accepts_a_full_set(tmp_path):
    pack.validate(_full_set(tmp_path), "pin", "1.0")


def test_validate_refuses_a_missing_f1(tmp_path):
    pin_dir = _full_set(tmp_path)
    (pin_dir / "f1.json").unlink()
    with pytest.raises(SystemExit, match="filtered"):
        pack.validate(pin_dir, "pin", "1.0")


def test_validate_refuses_a_page_count_mismatch(tmp_path):
    pin_dir = _full_set(tmp_path)
    next((pin_dir / "pages").glob("*.json")).unlink()
    with pytest.raises(SystemExit, match="1 page files but index.json lists 2"):
        pack.validate(pin_dir, "pin", "1.0")


def test_validate_refuses_another_pin(tmp_path):
    with pytest.raises(SystemExit, match="Cargo.toml pins other"):
        pack.validate(_full_set(tmp_path), "other", "1.0")


def test_validate_refuses_another_version(tmp_path):
    with pytest.raises(SystemExit, match="upstream version 1.0, Cargo.toml has 2.0"):
        pack.validate(_full_set(tmp_path), "pin", "2.0")


def test_validate_refuses_a_missing_index(tmp_path):
    pin_dir = _full_set(tmp_path)
    (pin_dir / "index.json").unlink()
    with pytest.raises(SystemExit, match="index.json"):
        pack.validate(pin_dir, "pin", "1.0")


def test_main_keeps_an_existing_archive(tmp_path, monkeypatch, capsys):
    monkeypatch.setattr(pack, "pinned_version", lambda: "1.0")
    archive = tmp_path / pack.archive_name("1.0")
    archive.write_bytes(b"keep")
    assert pack.main(["--fixtures", str(tmp_path / "none"), "--out", str(tmp_path)]) == 0
    assert archive.read_bytes() == b"keep"
    assert "archive exists" in capsys.readouterr().out


def test_page_id_is_ascii_and_reversible_per_source():
    from parity import page_id

    assert page_id("tests/eval/a b.html") == "tests__eval__a b.html"
    assert page_id("tests/eval/jadł.html") == "tests__eval__jad%C5%82.html"
    assert page_id("tests/eval/100%.html") == "tests__eval__100%25.html"


def test_validate_refuses_non_ascii_names(tmp_path):
    pin_dir = _full_set(tmp_path)
    (pin_dir / "pages" / "jadł.json").write_text("{}\n")
    with pytest.raises(SystemExit, match="non-ASCII"):
        pack.validate(pin_dir, "pin", "1.0")


def test_pinned_version_is_read_from_the_upstream_table(tmp_path, monkeypatch):
    from parity import generate

    (tmp_path / "Cargo.toml").write_text(
        '[package]\nversion = "0.0.0"\n\n[package.metadata.upstream]\nversion = "2.3.1"\ncommit = "abc"\n'
    )
    monkeypatch.setattr(generate, "REPO_ROOT", tmp_path)
    assert generate.pinned_version() == "2.3.1"
    assert generate.pinned_commit() == "abc"
