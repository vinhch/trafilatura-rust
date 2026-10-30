"""Time of upstream's decode + parse and extract, heap of its decode + parse, and the merged Rust/Python comparison.

    python -m parity.measure --upstream PATH --out DIR [--only SUBSTRING]
    python -m parity.measure --merge --out DIR [--rows N]
    python -m parity.measure --compare DIR [--head-only REASON]

The first form times `trafilatura.utils.load_html(bytes)` and `extract(bytes)` (TXT, fast mode, dedup off:
the fixture options; 5-run medians) and measures the heap of `load_html` with tracemalloc, per page of the
fixture corpus, into DIR/<version>-python.json. The second form combines
the crate's DIR/<version>-rust-time.json and DIR/<version>-rust-heap.json (tests/measure_time.rs,
tests/measure_heap.rs) into DIR/<version>-rust.json, merges that with the Python file into
DIR/<version>.json and prints a Markdown table.

Time is comparable. Heap is not: tracemalloc sees only memory allocated through Python allocators,
and lxml builds its tree with the libxml2 C allocator, so the Python heap columns miss the tree itself
while the Rust columns count every allocation. The table prints the numbers side by side and no
ratio for them. Upstream's extract heap is not measured at all.

The third form compares DIR/base and DIR/head, each holding `heap-sample.json` and a `criterion/` tree: it
prints a Markdown summary and returns 1 when the summed extraction allocation count or peak bytes over the
pages on both sides grow by more than GATE_PERCENT. Times are reported only. `--head-only` lists the head
numbers and never fails (no base to compare with).
"""

import argparse
import gc
import json
import math
import os
import platform
import statistics
import sys
import time
import tracemalloc
from pathlib import Path

RUNS = 5
HEAP_NOTE = "not comparable: tracemalloc excludes libxml2 C allocations (the lxml tree)"


def _median_ns(timings: list[float]) -> int:
    return round(statistics.median(timings) * 1e9)


def measure_page(load_html, extract, data: bytes, clear_caches=lambda: None) -> dict:
    """Median time of load_html and of extract over RUNS, then one traced load_html run for the heap; the
    tracer slows loads, so it never overlaps the timing. `clear_caches` runs before each timed extract
    call, outside the clock."""
    timings = []
    loaded = False
    for _ in range(RUNS):
        start = time.perf_counter()
        try:
            loaded = load_html(data) is not None
        except Exception:
            loaded = False
        timings.append(time.perf_counter() - start)
    extract_timings = []
    extract_ok = False
    for _ in range(RUNS):
        clear_caches()
        start = time.perf_counter()
        try:
            extract_ok = extract(data) is not None
        except Exception:
            extract_ok = False
        extract_timings.append(time.perf_counter() - start)
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
        "median_ns": _median_ns(timings),
        "extract_ok": extract_ok,
        "extract_median_ns": _median_ns(extract_timings),
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
    from .generate import _options, check_pin, corpus, pinned_version, quiet_upstream

    pin = check_pin(upstream_root)
    import_path(upstream_root)
    with quiet_upstream():
        from trafilatura import extract
        from trafilatura.settings import DEFAULT_CONFIG
        from trafilatura.utils import line_processing, load_html, trim

        options = _options("txt", False, DEFAULT_CONFIG.getboolean("DEFAULT", "EXTENSIVE_DATE_SEARCH"))

        def clear_caches() -> None:
            # lru_cache on page strings: runs 2 to 5 of one page would hit what a crawl of distinct pages misses
            trim.cache_clear()
            line_processing.cache_clear()

        pages = []
        for path in corpus(upstream_root):
            source = path.relative_to(upstream_root).as_posix()
            if only not in source:
                continue
            print(source, file=sys.stderr)
            pages.append({"id": page_id(source), "source": source, **measure_page(load_html, lambda data: extract(data, options=options), path.read_bytes(), clear_caches)})
        out.mkdir(parents=True, exist_ok=True)
        version = pinned_version()
        target = out / f"{version}-python.json"
        document = {"pin": pin, "upstream_version": version, "host": host(), "extra": {"runs": RUNS, "extract_options": "txt, fast=True, dedup=False"}, "pages": pages}
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


def _extract_totals(rows: list[dict], side: str) -> dict:
    """When every row measured extraction: summed median time and pages that gave text; Rust also the
    summed allocations and the largest peak (Python's extract heap is not measured)."""
    if not rows or not all("extract_median_ns" in r for r in rows):
        return {}
    result = {
        "extract_median_ms": sum(r["extract_median_ns"] for r in rows) / 1e6,
        "extract_ok_pages": sum(1 for r in rows if r["extract_ok"]),
    }
    if side == "rust":
        result |= {
            "extract_alloc_bytes": sum(r["extract_alloc_bytes"] for r in rows),
            "extract_alloc_count": sum(r["extract_alloc_count"] for r in rows),
            "extract_peak_max_bytes": max(r["extract_peak_bytes"] for r in rows),
        }
    return result


def totals(pages: list[dict]) -> dict:
    "Per side over the loaded pages: summed median time and retained heap, summed allocations (Rust only), peaks, extraction."
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
            **_extract_totals(rows, side),
        }
    return result


def _ratio(python: float, rust: float) -> str:
    return f"{python / rust:.1f}x" if rust else "n/a"


def _where(h: dict) -> str:
    return f"{h['os']}/{h['arch']} ({h['runtime']})"


def _extraction_block(r: dict, y: dict, pages: int) -> list[str]:
    "The extraction totals: two sides when the Python file measured extract, Rust only for an older run."
    if "extract_median_ms" not in y:
        return [
            "",
            f"Rust extraction (`extract`, TXT, fast mode; it repeats the decode and parse), {r['extract_ok_pages']} of "
            f"{pages} pages gave text, no Python counterpart:",
            "",
            "| metric | Rust |",
            "| --- | ---: |",
            f"| extraction median time, sum (ms) | {r['extract_median_ms']:.1f} |",
            f"| extraction peak heap, largest page (KiB) | {r['extract_peak_max_bytes'] / 1024:.0f} |",
            f"| extraction allocated, total (KiB) | {r['extract_alloc_bytes'] / 1024:.0f} |",
            f"| extraction allocation count, total | {r['extract_alloc_count']} |",
            "",
        ]
    return [
        "",
        f"Extraction (`extract`, TXT, fast mode; it repeats the decode and parse), {pages} pages, text from "
        f"{r['extract_ok_pages']} on Rust and {y['extract_ok_pages']} on Python:",
        "",
        "| metric | Rust | Python | Python / Rust |",
        "| --- | ---: | ---: | ---: |",
        f"| extraction median time, sum (ms) | {r['extract_median_ms']:.1f} | {y['extract_median_ms']:.1f} "
        f"| {_ratio(y['extract_median_ms'], r['extract_median_ms'])} |",
        f"| extraction peak heap, largest page (KiB) | {r['extract_peak_max_bytes'] / 1024:.0f} | n/a | n/a |",
        f"| extraction allocated, total (KiB) | {r['extract_alloc_bytes'] / 1024:.0f} | n/a | n/a |",
        f"| extraction allocation count, total | {r['extract_alloc_count']} | n/a | n/a |",
        "",
    ]


def format_table(pages: list[dict], rows: int = 0, dropped: int = 0, hosts: dict | None = None) -> str:
    "Markdown: the first `rows` loaded pages one per line, the totals, then the caveats."
    usable = loaded_pages(pages)
    lines = []
    t = totals(pages)
    r, y = t["rust"], t["python"]
    extracted = "extract_median_ms" in r
    compared = extracted and "extract_median_ms" in y
    if rows:
        header = "| page | input KiB | time ms Rust | time ms Python | Python / Rust | peak KiB Rust | peak KiB Python (*) |"
        rule = "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"
        if extracted:
            header += " extract ms Rust |"
            rule += " ---: |"
            if compared:
                header += " extract ms Python | extract Python / Rust |"
                rule += " ---: | ---: |"
            header += " extract peak KiB Rust |"
            rule += " ---: |"
        lines += [header, rule]
        for p in usable[:rows]:
            pr, py = p["rust"], p["python"]
            row = (
                f"| {p['source']} | {p['input_bytes'] / 1024:.1f} | {pr['median_ns'] / 1e6:.2f} | {py['median_ns'] / 1e6:.2f} "
                f"| {_ratio(py['median_ns'], pr['median_ns'])} | {pr['peak_bytes'] / 1024:.0f} | {py['peak_bytes'] / 1024:.0f} |"
            )
            if extracted:
                row += f" {pr['extract_median_ns'] / 1e6:.2f} |"
                if compared:
                    row += f" {py['extract_median_ns'] / 1e6:.2f} | {_ratio(py['extract_median_ns'], pr['extract_median_ns'])} |"
                row += f" {pr['extract_peak_bytes'] / 1024:.0f} |"
            lines.append(row)
        if len(usable) > rows:
            lines.append(f"| ... {len(usable) - rows} more pages | | | | | | |{' | |' * (2 if compared else 1) if extracted else ''}")
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
        *(_extraction_block(r, y, len(usable)) if extracted else []),
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


GATE_PERCENT = 1.0
DIFF_ROWS = 20
GATED = ("extract_alloc_count", "extract_peak_bytes")


def _percent(base: int, head: int) -> float:
    if base == 0:
        return 0.0 if head == 0 else math.inf
    return (head - base) * 100 / base


def _signed(percent: float) -> str:
    return f"{percent:+.2f}%"


def heap_change(base: dict, head: dict) -> dict:
    """Extraction heap of base and head over the pages in both: summed allocation count and peak, with the
    percent change, and the pages whose numbers moved, largest first."""
    head_pages = {p["id"]: p for p in head["pages"]}
    common = [(b, head_pages[b["id"]]) for b in base["pages"] if b["id"] in head_pages]
    result = {"one_side": len(base["pages"]) + len(head["pages"]) - 2 * len(common)}
    for key in GATED:
        base_sum = sum(b[key] for b, _ in common)
        head_sum = sum(h[key] for _, h in common)
        result[key] = (base_sum, head_sum, _percent(base_sum, head_sum))
    pages = [
        {
            "id": b["id"],
            "alloc_count_delta": h["extract_alloc_count"] - b["extract_alloc_count"],
            "peak_bytes_delta": h["extract_peak_bytes"] - b["extract_peak_bytes"],
        }
        for b, h in common
    ]
    pages = [p for p in pages if p["alloc_count_delta"] or p["peak_bytes_delta"]]
    pages.sort(key=lambda p: (abs(p["alloc_count_delta"]), abs(p["peak_bytes_delta"])), reverse=True)
    result["pages"] = pages
    return result


def criterion_medians(root: Path) -> dict[str, float]:
    "Median time in ns per `<group>/<bench>` from the last criterion run under `root`."
    return {
        estimates.relative_to(root).parent.parent.as_posix(): json.loads(estimates.read_text(encoding="utf-8"))["median"]["point_estimate"]
        for estimates in sorted(root.glob("*/*/new/estimates.json"))
    }


def _heap_table(heap: dict, head_heap: dict, head_only: str | None) -> list[str]:
    if head_only:
        lines = ["| metric | head |", "| --- | ---: |"]
        for key in GATED:
            lines.append(f"| {key} | {sum(p[key] for p in head_heap['pages'])} |")
        return lines
    lines = ["| metric | base | head | change |", "| --- | ---: | ---: | ---: |"]
    for key in GATED:
        base_sum, head_sum, percent = heap[key]
        verdict = "FAIL" if percent > GATE_PERCENT else "OK"
        lines.append(f"| {key} | {base_sum} | {head_sum} | {_signed(percent)} {verdict} |")
    return lines


def _page_rows(pages: list[dict]) -> list[str]:
    if not pages:
        return []
    lines = [
        "",
        "Per-page heap deltas (informational: one page varies up to about 1% between runs of the same commit, the gate is on the sums):",
        "",
        "| page | alloc count delta | peak bytes delta |",
        "| --- | ---: | ---: |",
    ]
    lines += [f"| {p['id']} | {p['alloc_count_delta']:+d} | {p['peak_bytes_delta']:+d} |" for p in pages[:DIFF_ROWS]]
    if len(pages) > DIFF_ROWS:
        lines.append(f"| ... {len(pages) - DIFF_ROWS} more pages | | |")
    return lines


def _time_rows(base_times: dict[str, float], head_times: dict[str, float], head_only: str | None) -> list[str]:
    if head_only:
        return ["| bench | head ms |", "| --- | ---: |", *(f"| {k} | {v / 1e6:.3f} |" for k, v in sorted(head_times.items()))]
    rows = [(k, base_times[k], v, _percent(base_times[k], v)) for k, v in head_times.items() if k in base_times]
    rows.sort(key=lambda r: abs(r[3]), reverse=True)
    return [
        "| bench | base ms | head ms | change |",
        "| --- | ---: | ---: | ---: |",
        *(f"| {k} | {b / 1e6:.3f} | {h / 1e6:.3f} | {_signed(pct)} |" for k, b, h, pct in rows),
    ]


def regression_summary(heap: dict, head_heap: dict, base_times: dict[str, float], head_times: dict[str, float], head_only: str | None) -> tuple[str, bool]:
    """Markdown of the base against head comparison, and whether the heap gate failed. `heap` is
    `heap_change`'s result, `head_heap` the head's heap file (listed alone in head-only mode)."""
    failed = not head_only and any(heap[key][2] > GATE_PERCENT for key in GATED)
    lines = [f"Head only: {head_only}.", ""] if head_only else []
    if head_only:
        lines += ["Extraction heap, summed over the head pages:", ""]
    else:
        lines += [f"Extraction heap, summed over the pages on both sides (gate: more than +{GATE_PERCENT:.2f}% fails):", ""]
    lines += _heap_table(heap, head_heap, head_only)
    if not head_only:
        lines += _page_rows(heap["pages"])
        if heap["one_side"]:
            lines += ["", f"{heap['one_side']} pages on one side only, not compared."]
    lines += ["", "Bench median times (reported only, runner noise is 10 to 20%):", ""]
    lines += _time_rows(base_times, head_times, head_only)
    return "\n".join(lines), failed


def compare(root: Path, head_only: str | None) -> int:
    "Print the summary of `root/base` against `root/head`; 1 when the heap gate fails or a file is missing."
    try:
        head_heap = _read(root / "head" / "heap-sample.json")
        base_heap = head_heap if head_only else _read(root / "base" / "heap-sample.json")
    except OSError as err:
        print(f"measure: {err}; run scripts/regression.sh first", file=sys.stderr)
        return 1
    base_times = {} if head_only else criterion_medians(root / "base" / "criterion")
    text, failed = regression_summary(
        heap_change(base_heap, head_heap), head_heap, base_times, criterion_medians(root / "head" / "criterion"), head_only
    )
    print(text)
    return 1 if failed else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--upstream", type=Path)
    parser.add_argument("--only", default="", help="keep pages whose source contains this substring")
    parser.add_argument("--merge", action="store_true", help="combine the Rust and Python files and print the table")
    parser.add_argument("--rows", type=int, default=0, help="with --merge: list this many pages before the totals")
    parser.add_argument("--compare", type=Path, metavar="DIR", help="compare DIR/base with DIR/head and apply the heap gate")
    parser.add_argument("--head-only", metavar="REASON", help="with --compare: list the head numbers only, never fail")
    args = parser.parse_args(argv)
    if args.compare:
        return compare(args.compare, args.head_only)
    if args.out is None:
        parser.error("--out is required unless --compare")
    if args.merge:
        return merge(args.out, args.rows)
    if args.upstream is None:
        parser.error("--upstream is required unless --merge")
    run(args.upstream.resolve(), args.out, args.only)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
