# Fixed "now" for date extraction (htmldate max_date); written into index.json, passed to Rust the same way.
MAX_DATE = "2026-10-01"


def page_id(source: str) -> str:
    """File-name id of a corpus page: the source path with `/` as `__`, non-ASCII bytes as `%XX`.

    ASCII only because bsdtar on Windows writes names through the ANSI code page and turns `ł`
    into `l`, which would detach the unpacked page files from index.json.
    """
    return "".join(c if c.isascii() and c != "%" else "".join(f"%{b:02X}" for b in c.encode()) for c in source.replace("/", "__"))


def import_path(upstream_root):
    """Put the upstream checkout on sys.path and make `importlib.metadata.version("trafilatura")` work.

    Upstream's xml.py reads its own version from installed metadata at import; a fresh clone has none,
    so a throwaway dist-info with the version from `trafilatura/__init__.py` stands in for it.
    """
    import re
    import sys
    import tempfile
    from importlib.metadata import PackageNotFoundError, version
    from pathlib import Path

    root = Path(upstream_root)
    sys.path.insert(0, str(root))
    try:
        version("trafilatura")
        return
    except PackageNotFoundError:
        pass
    found = re.search(r'^__version__\s*=\s*"([^"]+)"', (root / "trafilatura" / "__init__.py").read_text(encoding="utf-8"), re.M)
    if found is None:
        raise SystemExit(f"no __version__ in {root / 'trafilatura' / '__init__.py'}")
    dist_info = Path(tempfile.mkdtemp(prefix="parity-meta-")) / "trafilatura.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text(f"Metadata-Version: 2.1\nName: trafilatura\nVersion: {found.group(1)}\n", encoding="utf-8")
    sys.path.insert(0, str(dist_info.parent))
