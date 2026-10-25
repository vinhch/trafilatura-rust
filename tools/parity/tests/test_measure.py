import pytest

from parity import measure

HOST = {"os": "windows", "arch": "amd64", "runtime": "native"}


def _side(time_ns, peak, retained=100, blocks=10, loaded=True, **extra):
    return {
        "input_bytes": 2048,
        "loaded": loaded,
        "median_ns": time_ns,
        "peak_bytes": peak,
        "retained_bytes": retained,
        "retained_blocks": blocks,
        **extra,
    }


def _rust(page_id, time_ns, peak, loaded=True):
    return {"id": page_id, "source": page_id, **_side(time_ns, peak, loaded=loaded, alloc_bytes=1000, alloc_count=5)}


def _python(page_id, time_ns, peak, loaded=True):
    return {"id": page_id, "source": page_id, **_side(time_ns, peak, retained=400, blocks=40, loaded=loaded)}


def _merged(rust_pages, python_pages):
    return measure.merge_pages({"pages": rust_pages}, {"pages": python_pages})


def test_merge_keeps_pages_on_both_sides_and_counts_the_rest():
    pages, dropped = _merged([_rust("a", 1, 1), _rust("b", 1, 1)], [_python("b", 2, 2), _python("c", 2, 2)])
    assert [p["id"] for p in pages] == ["b"]
    assert dropped == 2


def test_totals_sum_time_and_take_peak_max():
    pages, _ = _merged(
        [_rust("a", 1_000_000, 100), _rust("b", 3_000_000, 300)],
        [_python("a", 4_000_000, 1000), _python("b", 8_000_000, 500)],
    )
    t = measure.totals(pages)
    assert t["rust"]["median_ms"] == 4.0
    assert t["python"]["median_ms"] == 12.0
    assert t["rust"]["peak_max_bytes"] == 300
    assert t["python"]["peak_max_bytes"] == 1000
    assert t["rust"]["alloc_count"] == 10
    assert t["python"]["alloc_count"] is None


def test_totals_exclude_a_page_not_loaded_on_either_side():
    pages, _ = _merged(
        [_rust("a", 1_000_000, 1), _rust("b", 5_000_000, 1, loaded=False), _rust("c", 7_000_000, 1)],
        [_python("a", 2_000_000, 1), _python("b", 5_000_000, 1), _python("c", 9_000_000, 1, loaded=False)],
    )
    assert measure.totals(pages)["rust"]["median_ms"] == 1.0
    table = measure.format_table(pages)
    assert "Totals over 1 pages" in table
    assert "1 compared, 2 excluded (not loaded on one side), 0 dropped" in table


def test_table_lists_rows_then_totals_with_time_ratio_only():
    pages, dropped = _merged(
        [_rust("a", 1_000_000, 2048), _rust("b", 1_000_000, 2048)],
        [_python("a", 5_000_000, 4096), _python("b", 5_000_000, 4096)],
    )
    table = measure.format_table(pages, rows=1, dropped=dropped)
    assert "| a | 2.0 | 1.00 | 5.00 | 5.0x | 2 | 4 |" in table
    assert "| ... 1 more pages |" in table
    assert "| median time, sum (ms) | 2.0 | 10.0 | 5.0x |" in table
    assert "| peak heap, largest page (KiB) (*) | 2 | 4 | n/a |" in table
    assert "| retained after parse, sum (KiB) (*) | 0 | 1 | n/a |" in table
    assert "| allocation count, total | 10 | n/a | n/a |" in table
    assert "(*) not comparable: tracemalloc excludes libxml2 C allocations" in table


def test_table_without_rows_has_totals_only():
    pages, _ = _merged([_rust("a", 1, 1)], [_python("a", 2, 2)])
    assert "| page |" not in measure.format_table(pages)


def test_table_names_the_platform_of_each_side():
    pages, _ = _merged([_rust("a", 1, 1)], [_python("a", 2, 2)])
    docker = {**HOST, "os": "linux", "runtime": "docker"}
    different = measure.format_table(pages, hosts={"rust": HOST, "python": docker})
    assert "Rust on windows/amd64 (native), Python on linux/amd64 (docker): the times come from different" in different
    same = measure.format_table(pages, hosts={"rust": HOST, "python": HOST})
    assert "both sides on windows/amd64 (native)" in same


def test_combine_rust_joins_time_and_heap_and_ands_loaded():
    time_run = {"pin": "p", "upstream_version": "v", "host": HOST, "extra": {"runs": 5}, "pages": [{"id": "a", "loaded": True, "median_ns": 7}]}
    heap_run = {
        "pin": "p",
        "host": HOST,
        "extra": {"peak_scope": "page"},
        "pages": [{"id": "a", "loaded": False, "peak_bytes": 9}],
    }
    page = measure.combine_rust(time_run, heap_run)["pages"][0]
    assert (page["median_ns"], page["peak_bytes"], page["loaded"]) == (7, 9, False)


def test_combine_rust_fails_on_a_page_in_only_one_run():
    def run(*ids):
        return {"pin": "p", "upstream_version": "v", "host": HOST, "extra": {}, "pages": [{"id": i, "loaded": True} for i in ids]}

    with pytest.raises(ValueError, match="1 only in time, 2 only in heap"):
        measure.combine_rust(run("a", "b"), run("a", "c", "d"))


def test_ratio_of_zero_is_not_a_division_error():
    assert measure._ratio(5, 0) == "n/a"


def _rust_extracted(page_id, extract_ns, extract_peak, extract_ok=True):
    return {
        **_rust(page_id, 1_000_000, 100),
        "extract_ok": extract_ok,
        "extract_median_ns": extract_ns,
        "extract_alloc_bytes": 2048,
        "extract_alloc_count": 7,
        "extract_peak_bytes": extract_peak,
    }


def test_totals_add_extraction_when_every_page_has_it():
    pages, _ = _merged(
        [_rust_extracted("a", 2_000_000, 4096), _rust_extracted("b", 3_000_000, 8192, extract_ok=False)],
        [_python("a", 1, 1), _python("b", 1, 1)],
    )
    rust = measure.totals(pages)["rust"]
    assert rust["extract_median_ms"] == 5.0
    assert rust["extract_peak_max_bytes"] == 8192
    assert (rust["extract_alloc_bytes"], rust["extract_alloc_count"], rust["extract_ok_pages"]) == (4096, 14, 1)
    assert "extract_median_ms" not in measure.totals(pages)["python"]


def test_totals_have_no_extraction_for_a_run_without_it():
    pages, _ = _merged([_rust("a", 1, 1), _rust_extracted("b", 1, 1)], [_python("a", 1, 1), _python("b", 1, 1)])
    assert "extract_median_ms" not in measure.totals(pages)["rust"]
    assert "extraction" not in measure.format_table(pages)


def test_table_shows_extraction_columns_and_totals():
    pages, dropped = _merged(
        [_rust_extracted("a", 2_500_000, 4096), _rust_extracted("b", 1, 1)], [_python("a", 5_000_000, 4096), _python("b", 1, 1)]
    )
    table = measure.format_table(pages, rows=1, dropped=dropped)
    assert "| extract ms Rust | extract peak KiB Rust |" in table
    assert "| a | 2.0 | 1.00 | 5.00 | 5.0x | 0 | 4 | 2.50 | 4 |" in table
    assert "| ... 1 more pages | | | | | | | | |" in table
    assert "2 of 2 pages gave text" in table
    assert "| extraction median time, sum (ms) | 2.5 |" in table
    assert "| extraction allocation count, total | 14 |" in table


def test_combine_rust_ands_extract_ok():
    time_run = {"pin": "p", "upstream_version": "v", "host": HOST, "extra": {}, "pages": [{"id": "a", "loaded": True, "extract_ok": True}]}
    heap_run = {"pin": "p", "host": HOST, "extra": {}, "pages": [{"id": "a", "loaded": True, "extract_ok": False}]}
    assert measure.combine_rust(time_run, heap_run)["pages"][0]["extract_ok"] is False
