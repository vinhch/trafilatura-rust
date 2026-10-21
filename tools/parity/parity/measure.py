"""Time and heap of upstream's decode + parse, and the merged Rust/Python comparison.

    python -m parity.measure --upstream PATH --out DIR [--only SUBSTRING]
    python -m parity.measure --merge --out DIR [--rows N]

The first form times `trafilatura.utils.load_html(bytes)` (5-run median) and measures its heap with
tracemalloc, per page of the fixture corpus, into DIR/<pin>-python.json. The second form reads that
file and the crate's DIR/<pin>-rust.json (tests/measure.rs), writes DIR/<pin>.json and prints a
Markdown table. tracemalloc has no running total of allocations: Python reports the heap still held
once the tree is built (`retained_*`), the crate reports that and its total allocations.

tracemalloc sees only memory allocated through Python's allocators. lxml builds its tree with
libxml2's C allocator, which it does not see, so the Python heap columns undercount upstream's real
memory (the tree itself is missing); the Rust columns count every allocation. Time is comparable;
heap is a lower bound for Python.
"""

import argparse
import gc
import json
import logging
import statistics
import sys
import time
import tracemalloc
import warnings
from pathlib import Path

RUNS = 5


def measure_page(load_html, data: bytes) -> dict:
    "Median time over RUNS, then one traced run for the heap; the tracer slows loads, so it never overlaps the timing."
    timings = []
    loaded = False
    for _ in range(RUNS):
        start = time.perf_counter()
        try:
            loaded = load_html(data) is not None
        except Exception:
            loaded = False
        timings.append(time.perf_counter() - start)
    gc.collect()
    tracemalloc.start(1)
    try:
        base, _ = tracemalloc.get_traced_memory()
        tracemalloc.reset_peak()
        try:
            tree = load_html(data)
        except Exception:
            tree = None
        current, peak = tracemalloc.get_traced_memory()
        blocks = len(tracemalloc.take_snapshot().traces)
    finally:
        tracemalloc.stop()
    del tree
    return {
        "input_bytes": len(data),
        "loaded": loaded,
        "median_ns": round(statistics.median(timings) * 1e9),
        "retained_bytes": current - base,
        "retained_blocks": blocks,
        "peak_bytes": peak - base,
    }


def run(upstream_root: Path, out: Path, only: str) -> None:
    from .generate import check_pin, corpus

    pin = check_pin(upstream_root)
    sys.path.insert(0, str(upstream_root))
    logging.disable(logging.CRITICAL)
    warnings.simplefilter("ignore")
    from trafilatura.utils import load_html

    pages = []
    for path in corpus(upstream_root):
        source = path.relative_to(upstream_root).as_posix()
        if only not in source:
            continue
        print(source, file=sys.stderr)
        pages.append({"id": source.replace("/", "__"), "source": source, **measure_page(load_html, path.read_bytes())})
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"{pin}-python.json"
    target.write_text(json.dumps({"pin": pin, "runs": RUNS, "pages": pages}, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(pages)} pages to {target}", file=sys.stderr)


def merge_pages(rust: dict, python: dict) -> list[dict]:
    "Pages present on both sides, in the Rust file's order."
    by_id = {p["id"]: p for p in python["pages"]}
    return [
        {"id": r["id"], "source": r["source"], "input_bytes": r["input_bytes"], "rust": r, "python": by_id[r["id"]]}
        for r in rust["pages"]
        if r["id"] in by_id
    ]


def totals(pages: list[dict]) -> dict:
    "Per side: summed median time and retained heap, summed allocations (Rust only), largest and mean peak."
    result = {}
    for side in ("rust", "python"):
        rows = [p[side] for p in pages]
        peaks = [r["peak_bytes"] for r in rows]
        result[side] = {
            "median_ms": sum(r["median_ns"] for r in rows) / 1e6,
            "alloc_bytes": sum(r["alloc_bytes"] for r in rows) if side == "rust" else None,
            "alloc_count": sum(r["alloc_count"] for r in rows) if side == "rust" else None,
            "retained_bytes": sum(r["retained_bytes"] for r in rows),
            "retained_blocks": sum(r["retained_blocks"] for r in rows),
            "peak_max_bytes": max(peaks, default=0),
            "peak_mean_bytes": sum(peaks) / len(peaks) if peaks else 0,
        }
    return result


def _ratio(python: float, rust: float) -> str:
    return f"{python / rust:.1f}x" if rust else "n/a"


def format_table(pages: list[dict], rows: int = 0) -> str:
    "Markdown: the first `rows` pages one per line, then the totals."
    lines = []
    if rows:
        lines += [
            "| page | input KiB | time ms Rust | time ms Python | Python / Rust | peak KiB Rust | peak KiB Python | Python / Rust |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
        for p in pages[:rows]:
            r, y = p["rust"], p["python"]
            lines.append(
                f"| {p['source']} | {p['input_bytes'] / 1024:.1f} | {r['median_ns'] / 1e6:.2f} | {y['median_ns'] / 1e6:.2f} "
                f"| {_ratio(y['median_ns'], r['median_ns'])} | {r['peak_bytes'] / 1024:.0f} | {y['peak_bytes'] / 1024:.0f} "
                f"| {_ratio(y['peak_bytes'], r['peak_bytes'])} |"
            )
        if len(pages) > rows:
            lines.append(f"| ... {len(pages) - rows} more pages | | | | | | | |")
        lines.append("")
    t = totals(pages)
    r, y = t["rust"], t["python"]
    lines += [
        f"Totals over {len(pages)} pages, decode + parse:",
        "",
        "| metric | Rust | Python | Python / Rust |",
        "| --- | ---: | ---: | ---: |",
        f"| median time, sum (ms) | {r['median_ms']:.1f} | {y['median_ms']:.1f} | {_ratio(y['median_ms'], r['median_ms'])} |",
        f"| peak heap, largest page (KiB) | {r['peak_max_bytes'] / 1024:.0f} | {y['peak_max_bytes'] / 1024:.0f} "
        f"| {_ratio(y['peak_max_bytes'], r['peak_max_bytes'])} |",
        f"| peak heap, mean per page (KiB) | {r['peak_mean_bytes'] / 1024:.0f} | {y['peak_mean_bytes'] / 1024:.0f} "
        f"| {_ratio(y['peak_mean_bytes'], r['peak_mean_bytes'])} |",
        f"| retained after parse, sum (KiB) | {r['retained_bytes'] / 1024:.0f} | {y['retained_bytes'] / 1024:.0f} "
        f"| {_ratio(y['retained_bytes'], r['retained_bytes'])} |",
        f"| retained blocks, sum | {r['retained_blocks']} | {y['retained_blocks']} "
        f"| {_ratio(y['retained_blocks'], r['retained_blocks'])} |",
        f"| allocated, total (KiB) | {r['alloc_bytes'] / 1024:.0f} | n/a | n/a |",
        f"| allocation count, total | {r['alloc_count']} | n/a | n/a |",
        "",
        "Python heap is tracemalloc: it misses the libxml2 tree lxml builds, so it is a lower bound.",
    ]
    return "\n".join(lines)


def merge(out: Path, rows: int) -> int:
    from .generate import pinned_commit

    pin = pinned_commit()
    try:
        rust = json.loads((out / f"{pin}-rust.json").read_text(encoding="utf-8"))
        python = json.loads((out / f"{pin}-python.json").read_text(encoding="utf-8"))
    except OSError as err:
        print(f"measure: {err}; run scripts/measure.sh first", file=sys.stderr)
        return 1
    pages = merge_pages(rust, python)
    if not pages:
        print("measure: the Rust and Python files share no page", file=sys.stderr)
        return 1
    merged = {
        "pin": pin,
        "runs": rust["runs"],
        "peak_scope": rust["peak_scope"],
        "totals": totals(pages),
        "pages": pages,
    }
    (out / f"{pin}.json").write_text(json.dumps(merged, indent=1) + "\n", encoding="utf-8")
    print(format_table(pages, rows))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--upstream", type=Path)
    parser.add_argument("--only", default="", help="keep pages whose source contains this substring")
    parser.add_argument("--merge", action="store_true", help="combine the Rust and Python files and print the table")
    parser.add_argument("--rows", type=int, default=0, help="with --merge: list this many pages before the totals")
    args = parser.parse_args(argv)
    if args.merge:
        return merge(args.out, args.rows)
    if args.upstream is None:
        parser.error("--upstream is required unless --merge")
    run(args.upstream.resolve(), args.out, args.only)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
