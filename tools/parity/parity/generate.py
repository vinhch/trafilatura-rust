"""Run upstream trafilatura (the checkout at the pinned commit) over its test corpus and write fixtures.

    python -m parity.generate --upstream PATH --out DIR [--only SUBSTRING]

Output: DIR/<pin>/index.json, DIR/<pin>/pages/<id>.json, DIR/<pin>/f1.json. Each page is extracted
twice: `lxml` (upstream as is) and `html5` (document and fragment parsers swapped for html5lib).
"""

import argparse
import contextlib
import hashlib
import json
import logging
import sys
import tomllib
import warnings
from pathlib import Path

from . import MAX_DATE

REPO_ROOT = Path(__file__).resolve().parents[3]
LOCK_FILE = Path(__file__).resolve().parents[1] / "uv.lock"
FORMATS = ("txt", "markdown", "html", "xml", "xmltei", "json")
# Document fields that are bodies, trees or the extracted text, not metadata
_NOT_METADATA = {"body", "commentsbody", "comments", "raw_text", "text"}


def pinned_commit() -> str:
    with open(REPO_ROOT / "Cargo.toml", "rb") as f:
        return tomllib.load(f)["package"]["metadata"]["upstream"]["commit"]


def head_commit(upstream_root: Path) -> str:
    "HEAD of a git checkout, read from .git (the slim Docker image has no git binary)."
    git_dir = upstream_root / ".git"
    try:
        head = (git_dir / "HEAD").read_text().strip()
        if not head.startswith("ref: "):
            return head
        ref = head.removeprefix("ref: ")
        ref_file = git_dir / ref
        if ref_file.is_file():
            return ref_file.read_text().strip()
        for line in (git_dir / "packed-refs").read_text().splitlines():
            if line.endswith(" " + ref):
                return line.split()[0]
    except OSError:
        pass
    raise SystemExit(f"{upstream_root} is not a git checkout with a readable HEAD")


def check_pin(upstream_root: Path, expected: str | None = None) -> str:
    "Return the pin; exit when the checkout is at another commit."
    expected = expected or pinned_commit()
    actual = head_commit(upstream_root)
    if actual != expected:
        raise SystemExit(f"upstream checkout is at {actual}, Cargo.toml pins {expected}; check out the pin first")
    return expected


@contextlib.contextmanager
def html5_parsers(normalized: list[bool] | None = None):
    "Swap upstream's two parse sites for html5lib parses; `normalized` collects whether the copy rule fired."
    from . import html5tree

    # `trafilatura.baseline` the attribute is a function; the module is in sys.modules
    import trafilatura  # noqa: F401  (ensures the package is imported)

    baseline_module = sys.modules["trafilatura.baseline"]
    utils_module = sys.modules["trafilatura.utils"]

    def note() -> None:
        if normalized is not None:
            normalized.append(html5tree.last_parse_normalized())

    def fromstring(text, parser=None, **_):
        if isinstance(text, bytes):
            text = text.decode("utf-8", "replace")
        tree = html5tree.parse_document(text)
        note()
        return tree

    def fragment_fromstring(text, create_parent=None, **_):
        tree = html5tree.parse_fragment(text, create_parent or "div")
        note()
        return tree

    saved = (utils_module.fromstring, baseline_module.fragment_fromstring)
    utils_module.fromstring = fromstring
    baseline_module.fragment_fromstring = fragment_fromstring
    try:
        yield
    finally:
        utils_module.fromstring, baseline_module.fragment_fromstring = saved


def corpus(upstream_root: Path) -> list[Path]:
    tests = upstream_root / "tests"
    pages = [*tests.joinpath("eval").glob("*.html"), *tests.joinpath("cache").glob("*.html")]
    pages += [*tests.joinpath("resources").glob("*.html"), tests / "resources" / "webpage.html.gz"]
    return sorted(pages, key=lambda p: p.relative_to(upstream_root).as_posix())


def _options(output_format: str, with_metadata: bool, extensive_date_search: bool):
    from trafilatura.settings import Extractor

    return Extractor(
        output_format=output_format,
        fast=True,
        dedup=False,
        with_metadata=with_metadata,
        date_params={"original_date": True, "extensive_search": extensive_date_search, "max_date": MAX_DATE},
    )


def run_variant(data: bytes, extensive_date_search: bool) -> dict:
    "One variant's result for one page: tree dump, every output format, metadata."
    from trafilatura import bare_extraction, extract
    from trafilatura.utils import load_html

    from .treedump import dump_tree

    def guarded(fn):
        try:
            return fn()
        except Exception as err:  # a crash is part of upstream's behavior; recorded as null
            print(f"  {type(err).__name__}: {err}", file=sys.stderr)
            return None

    tree = guarded(lambda: load_html(data))
    formats = {f: guarded(lambda f=f: extract(data, options=_options(f, False, extensive_date_search))) for f in FORMATS}
    doc = guarded(lambda: bare_extraction(data, options=_options("python", True, extensive_date_search)))
    metadata = None
    if doc is not None:
        metadata = {k: v for k, v in doc.as_dict().items() if k not in _NOT_METADATA}
    return {"tree": dump_tree(tree) if tree is not None else None, "formats": formats, "metadata": metadata}


def valid_utf8(data: bytes) -> bool:
    "Whether the page is valid UTF-8 after upstream's decompression."
    from trafilatura.utils import handle_compressed_file

    try:
        handle_compressed_file(data).decode("utf-8")
    except UnicodeDecodeError:
        return False
    return True


def f1_scores(upstream_root: Path) -> dict:
    "Upstream's own precision/recall/F1 over tests/evaldata.json, from its eval_common (fast runner)."
    sys.path.insert(0, str(upstream_root / "tests"))
    from eval_common import RUNNERS, ConfusionMatrix, load_evaldata, read_corpus, run_and_count

    tests = str(upstream_root / "tests")
    evaldata = load_evaldata(tests)
    docs = read_corpus(tests, evaldata)
    matrix = ConfusionMatrix()
    for url, (_, data) in docs.items():
        matrix.add(run_and_count(RUNNERS["fast"], data, evaldata[url])[1])
    precision, recall, accuracy, f1 = matrix.scores()
    return {
        "runner": "fast",
        "entries": len(docs),
        "tp": matrix.tp,
        "fp": matrix.fp,
        "fn": matrix.fn,
        "tn": matrix.tn,
        "precision": precision,
        "recall": recall,
        "accuracy": accuracy,
        "f1": f1,
    }


def _write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, ensure_ascii=True, indent=1) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--only", default="", help="keep pages whose source contains this substring (skips f1.json)")
    args = parser.parse_args(argv)

    upstream_root = args.upstream.resolve()
    pin = check_pin(upstream_root)
    sys.path.insert(0, str(upstream_root))
    logging.disable(logging.CRITICAL)
    warnings.simplefilter("ignore")

    import trafilatura
    from trafilatura.settings import DEFAULT_CONFIG

    extensive = DEFAULT_CONFIG.getboolean("DEFAULT", "EXTENSIVE_DATE_SEARCH")
    pages_dir = args.out / pin / "pages"
    pages_dir.mkdir(parents=True, exist_ok=True)
    for stale in pages_dir.glob("*.json"):
        stale.unlink()

    index_pages = []
    for path in corpus(upstream_root):
        source = path.relative_to(upstream_root).as_posix()
        if args.only not in source:
            continue
        page_id = source.replace("/", "__")
        print(source, file=sys.stderr)
        data = path.read_bytes()
        lxml_result = run_variant(data, extensive)
        normalized: list[bool] = []
        with html5_parsers(normalized):
            html5_result = run_variant(data, extensive)
        _write_json(
            pages_dir / f"{page_id}.json",
            {"source": source, "normalized": any(normalized), "lxml": lxml_result, "html5": html5_result},
        )
        index_pages.append({"id": page_id, "source": source, "valid_utf8": valid_utf8(data)})

    if not args.only:
        _write_json(args.out / pin / "f1.json", f1_scores(upstream_root))
    _write_json(
        args.out / pin / "index.json",
        {
            "pin": pin,
            "upstream_version": trafilatura.__version__,
            "max_date": MAX_DATE,
            "lock_sha256": hashlib.sha256(LOCK_FILE.read_bytes()).hexdigest(),
            "pages": index_pages,
        },
    )
    print(f"wrote {len(index_pages)} pages to {args.out / pin}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
