"""Time and heap of upstream's decode + parse, and the merged Rust/Python comparison.

    python -m parity.measure --upstream PATH --out DIR [--only SUBSTRING]
    python -m parity.measure --merge --out DIR [--rows N]

The first form times `trafilatura.utils.load_html(bytes)` (5-run median) and measures its heap with
tracemalloc, per page of the fixture corpus, into DIR/<version>-python.json. The second form combines
the crate's DIR/<version>-rust-time.json and DIR/<version>-rust-heap.json (tests/measure_time.rs,
tests/measure_heap.rs) into DIR/<version>-rust.json, merges that with the Python file into
DIR/<version>.json and prints a Markdown table.

Time is comparable. Heap is not: tracemalloc sees only memory allocated through Python allocators,
and lxml builds its tree with the libxml2 C allocator, so the Python heap columns miss the tree itself
while the Rust columns count every allocation. The table prints the numbers side by side and no
ratio for them.
"""

import argparse
import gc
import json
import logging
import os
import platform
import statistics
import sys
import time
import tracemalloc
import warnings
from pathlib import Path

RUNS = 5
HEAP_NOTE = "not comparable: tracemalloc excludes libxml2 C allocations (the lxml tree)"


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
        # The snapshot call allocates too: the baseline count carries that cost, so the difference does not
        base_blocks = len(tracemalloc.take_snapshot().traces)
        base, _ = tracemalloc.get_traced_memory()
        tracemalloc.reset_peak()
        try:
            tree = load_html(data)
        except Exception:
            tree = None
        current, peak = tracemalloc.get_traced_memory()
        blocks = len(tracemalloc.take_snapshot().traces) - base_blocks
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


def host() -> dict:
    "Where this ran; scripts/measure sets TRAFILATURA_MEASURE_RUNTIME=docker inside the container."
    return {
        "os": platform.system().lower(),
        "arch": platform.machine().lower(),
        "runtime": os.environ.get("TRAFILATURA_MEASURE_RUNTIME", "native"),
    }


def run(upstream_root: Path, out: Path, only: str) -> None:
    from . import import_path, page_id
    from .generate import check_pin, corpus, pinned_version

    pin = check_pin(upstream_root)
    import_path(upstream_root)
    logging.disable(logging.CRITICAL)
    warnings.simplefilter("ignore")
    from trafilatura.utils import load_html

    pages = []
    for path in corpus(upstream_root):
        source = path.relative_to(upstream_root).as_posix()
        if only not in source:
            continue
        print(source, file=sys.stderr)
        pages.append({"id": page_id(source), "source": source, **measure_page(load_html, path.read_bytes())})
    out.mkdir(parents=True, exist_ok=True)
    version = pinned_version()
    target = out / f"{version}-python.json"
    document = {"pin": pin, "upstream_version": version, "host": host(), "extra": {"runs": RUNS}, "pages": pages}
    target.write_text(json.dumps(document, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(pages)} pages to {target}", file=sys.stderr)


def combine_rust(time_run: dict, heap_run: dict) -> dict:
    """The crate's two runs as one file: per page the time fields and the heap fields.

    Raises ValueError when a page is in only one of the runs (the runs were made on different
    corpora, or one was cut short)."""
    heap = {p["id"]: p for p in heap_run["pages"]}
    time_ids = {p["id"] for p in time_run["pages"]}
    only_time = len(time_ids - heap.keys())
    only_heap = len(heap.keys() - time_ids)
    if only_time or only_heap:
        raise ValueError(f"rust time and heap runs cover different pages: {only_time} only in time, {only_heap} only in heap")
    pages = []
    for t in time_run["pages"]:
        h = heap[t["id"]]
        page = {**h, **t, "loaded": t["loaded"] and h["loaded"]}
        if "extract_ok" in t and "extract_ok" in h:
            page["extract_ok"] = t["extract_ok"] and h["extract_ok"]
        pages.append(page)
    return {
        "pin": time_run["pin"],
        "upstream_version": time_run["upstream_version"],
        "host": time_run["host"],
        "extra": {**time_run["extra"], **heap_run["extra"]},
        "pages": pages,
    }


def merge_pages(rust: dict, python: dict) -> tuple[list[dict], int]:
    "Pages on both sides in the Rust file's order, and how many ids are on one side only."
    by_id = {p["id"]: p for p in python["pages"]}
    pages = [
        {"id": r["id"], "source": r["source"], "input_bytes": r["input_bytes"], "rust": r, "python": by_id[r["id"]]}
        for r in rust["pages"]
        if r["id"] in by_id
    ]
    return pages, len(rust["pages"]) + len(python["pages"]) - 2 * len(pages)


def loaded_pages(pages: list[dict]) -> list[dict]:
    "Pages both sides parsed; a page one side rejected would skew the comparison."
    return [p for p in pages if p["rust"]["loaded"] and p["python"]["loaded"]]


def _extract_totals(rows: list[dict]) -> dict:
    "Rust only, when the run measured extraction: summed median time and allocations, the largest peak, pages that gave text."
    if not rows or not all("extract_median_ns" in r for r in rows):
        return {}
    return {
        "extract_median_ms": sum(r["extract_median_ns"] for r in rows) / 1e6,
        "extract_alloc_bytes": sum(r["extract_alloc_bytes"] for r in rows),
        "extract_alloc_count": sum(r["extract_alloc_count"] for r in rows),
        "extract_peak_max_bytes": max(r["extract_peak_bytes"] for r in rows),
        "extract_ok_pages": sum(1 for r in rows if r["extract_ok"]),
    }


def totals(pages: list[dict]) -> dict:
    "Per side over the loaded pages: summed median time and retained heap, summed allocations (Rust only), peaks."
    result = {}
    for side in ("rust", "python"):
        rows = [p[side] for p in loaded_pages(pages)]
        peaks = [r["peak_bytes"] for r in rows]
        result[side] = {
            "median_ms": sum(r["median_ns"] for r in rows) / 1e6,
            "alloc_bytes": sum(r["alloc_bytes"] for r in rows) if side == "rust" else None,
            "alloc_count": sum(r["alloc_count"] for r in rows) if side == "rust" else None,
            "retained_bytes": sum(r["retained_bytes"] for r in rows),
            "retained_blocks": sum(r["retained_blocks"] for r in rows),
            "peak_max_bytes": max(peaks, default=0),
            "peak_mean_bytes": sum(peaks) / len(peaks) if peaks else 0,
            **(_extract_totals(rows) if side == "rust" else {}),
        }
    return result


def _ratio(python: float, rust: float) -> str:
    return f"{python / rust:.1f}x" if rust else "n/a"


def _where(h: dict) -> str:
    return f"{h['os']}/{h['arch']} ({h['runtime']})"


def format_table(pages: list[dict], rows: int = 0, dropped: int = 0, hosts: dict | None = None) -> str:
    "Markdown: the first `rows` loaded pages one per line, the totals, then the caveats."
    usable = loaded_pages(pages)
    lines = []
    t = totals(pages)
    r, y = t["rust"], t["python"]
    extracted = "extract_median_ms" in r
    if rows:
        header = "| page | input KiB | time ms Rust | time ms Python | Python / Rust | peak KiB Rust | peak KiB Python (*) |"
        rule = "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"
        if extracted:
            header += " extract ms Rust | extract peak KiB Rust |"
            rule += " ---: | ---: |"
        lines += [header, rule]
        for p in usable[:rows]:
            pr, py = p["rust"], p["python"]
            row = (
                f"| {p['source']} | {p['input_bytes'] / 1024:.1f} | {pr['median_ns'] / 1e6:.2f} | {py['median_ns'] / 1e6:.2f} "
                f"| {_ratio(py['median_ns'], pr['median_ns'])} | {pr['peak_bytes'] / 1024:.0f} | {py['peak_bytes'] / 1024:.0f} |"
            )
            if extracted:
                row += f" {pr['extract_median_ns'] / 1e6:.2f} | {pr['extract_peak_bytes'] / 1024:.0f} |"
            lines.append(row)
        if len(usable) > rows:
            lines.append(f"| ... {len(usable) - rows} more pages | | | | | | |{' | |' if extracted else ''}")
        lines.append("")
    lines += [
        f"Totals over {len(usable)} pages, decode + parse:",
        "",
        "| metric | Rust | Python | Python / Rust |",
        "| --- | ---: | ---: | ---: |",
        f"| median time, sum (ms) | {r['median_ms']:.1f} | {y['median_ms']:.1f} | {_ratio(y['median_ms'], r['median_ms'])} |",
        f"| peak heap, largest page (KiB) (*) | {r['peak_max_bytes'] / 1024:.0f} | {y['peak_max_bytes'] / 1024:.0f} | n/a |",
        f"| peak heap, mean per page (KiB) (*) | {r['peak_mean_bytes'] / 1024:.0f} | {y['peak_mean_bytes'] / 1024:.0f} | n/a |",
        f"| retained after parse, sum (KiB) (*) | {r['retained_bytes'] / 1024:.0f} | {y['retained_bytes'] / 1024:.0f} | n/a |",
        f"| retained blocks, sum (*) | {r['retained_blocks']} | {y['retained_blocks']} | n/a |",
        f"| allocated, total (KiB) | {r['alloc_bytes'] / 1024:.0f} | n/a | n/a |",
        f"| allocation count, total | {r['alloc_count']} | n/a | n/a |",
        "",
        f"(*) {HEAP_NOTE}.",
        *(
            [
                "",
                f"Rust extraction (`extract`, TXT, fast mode; it repeats the decode and parse), {r['extract_ok_pages']} of "
                f"{len(usable)} pages gave text, no Python counterpart:",
                "",
                "| metric | Rust |",
                "| --- | ---: |",
                f"| extraction median time, sum (ms) | {r['extract_median_ms']:.1f} |",
                f"| extraction peak heap, largest page (KiB) | {r['extract_peak_max_bytes'] / 1024:.0f} |",
                f"| extraction allocated, total (KiB) | {r['extract_alloc_bytes'] / 1024:.0f} |",
                f"| extraction allocation count, total | {r['extract_alloc_count']} |",
                "",
            ]
            if extracted
            else []
        ),
        f"Pages: {len(usable)} compared, {len(pages) - len(usable)} excluded (not loaded on one side), "
        f"{dropped} dropped (on one side only).",
    ]
    if hosts:
        rust_where, python_where = _where(hosts["rust"]), _where(hosts["python"])
        if rust_where == python_where:
            lines.append(f"Platform: both sides on {rust_where}.")
        else:
            lines.append(
                f"Platform: Rust on {rust_where}, Python on {python_where}: the times come from different "
                "systems; compare ratios between runs of the same setup."
            )
    return "\n".join(lines)


def _read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def merge(out: Path, rows: int) -> int:
    from .generate import pinned_commit, pinned_version

    version = pinned_version()
    try:
        rust = combine_rust(_read(out / f"{version}-rust-time.json"), _read(out / f"{version}-rust-heap.json"))
        python = _read(out / f"{version}-python.json")
    except OSError as err:
        print(f"measure: {err}; run scripts/measure.sh first", file=sys.stderr)
        return 1
    except ValueError as err:
        print(f"measure: {err}; rerun scripts/measure.sh", file=sys.stderr)
        return 1
    pages, dropped = merge_pages(rust, python)
    if not loaded_pages(pages):
        print("measure: no page loaded on both sides", file=sys.stderr)
        return 1
    hosts = {"rust": rust["host"], "python": python["host"]}
    (out / f"{version}-rust.json").write_text(json.dumps(rust, indent=1) + "\n", encoding="utf-8")
    merged = {
        "pin": pinned_commit(),
        "upstream_version": version,
        "host": hosts,
        "extra": {"rust": rust["extra"], "python": python["extra"], "heap_note": HEAP_NOTE},
        "dropped": dropped,
        "totals": totals(pages),
        "pages": pages,
    }
    (out / f"{version}.json").write_text(json.dumps(merged, indent=1) + "\n", encoding="utf-8")
    print(format_table(pages, rows, dropped, hosts))
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
