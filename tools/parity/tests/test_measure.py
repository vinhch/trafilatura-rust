import json
import math

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


def test_measure_page_times_extract():
    extract_calls = []
    cache_clears = []
    result = measure.measure_page(
        lambda data: object(),
        lambda data: extract_calls.append(data) or "text",
        b"<p>x</p>",
        clear_caches=lambda: cache_clears.append(1),
    )
    assert result["extract_ok"] is True
    assert isinstance(result["extract_median_ns"], int) and result["extract_median_ns"] >= 0
    assert len(extract_calls) == measure.RUNS
    assert len(cache_clears) == measure.RUNS


def test_measure_page_records_a_raising_extract():
    def raising(data):
        raise ValueError("boom")

    for extract in (raising, lambda data: None):
        result = measure.measure_page(lambda data: object(), extract, b"<p>x</p>")
        assert result["extract_ok"] is False
        assert "extract_median_ns" in result


def _python_extracted(page_id, extract_ns):
    return {**_python(page_id, 1_000_000, 100), "extract_ok": True, "extract_median_ns": extract_ns}


def test_totals_add_python_extraction_time():
    pages, _ = _merged(
        [_rust_extracted("a", 2_000_000, 1), _rust_extracted("b", 2_000_000, 1)],
        [_python_extracted("a", 2_000_000), _python_extracted("b", 4_000_000)],
    )
    python = measure.totals(pages)["python"]
    assert python["extract_median_ms"] == 6.0
    assert "extract_alloc_count" not in python


def test_table_shows_python_extract_column_and_ratio():
    pages, dropped = _merged(
        [_rust_extracted("a", 2_000_000, 4096), _rust_extracted("b", 2_000_000, 4096)],
        [_python_extracted("a", 8_000_000), _python_extracted("b", 8_000_000)],
    )
    table = measure.format_table(pages, rows=1, dropped=dropped)
    assert "extract ms Python | extract Python / Rust" in table
    assert "| a | 2.0 | 1.00 | 1.00 | 1.0x | 0 | 0 | 2.00 | 8.00 | 4.0x | 4 |" in table
    assert "| ... 1 more pages | | | | | | | | | | |" in table
    assert "| extraction median time, sum (ms) | 4.0 | 16.0 | 4.0x |" in table
    assert "no Python counterpart" not in table


def _heap_doc(*pages):
    "pages: (id, alloc_count, peak_bytes)"
    return {"pages": [{"id": i, "extract_alloc_count": c, "extract_peak_bytes": p} for i, c, p in pages]}


def test_heap_change_sums_common_pages_only():
    base = _heap_doc(("a", 100, 10), ("b", 200, 20))
    head = _heap_doc(("b", 250, 30), ("c", 999, 99))
    change = measure.heap_change(base, head)
    assert change["extract_alloc_count"] == (200, 250, 25.0)
    assert change["extract_peak_bytes"] == (20, 30, 50.0)
    assert change["one_side"] == 2


def test_heap_change_orders_pages_by_largest_delta():
    base = _heap_doc(("a", 10, 5), ("b", 100, 5), ("c", 7, 5))
    head = _heap_doc(("a", 12, 5), ("b", 50, 5), ("c", 7, 5))
    pages = measure.heap_change(base, head)["pages"]
    assert [p["id"] for p in pages] == ["b", "a"]
    assert (pages[0]["alloc_count_delta"], pages[0]["peak_bytes_delta"]) == (-50, 0)


def test_heap_change_from_zero_base():
    same = measure.heap_change(_heap_doc(("a", 0, 0)), _heap_doc(("a", 0, 0)))
    assert same["extract_alloc_count"][2] == 0.0
    grown = measure.heap_change(_heap_doc(("a", 0, 0)), _heap_doc(("a", 5, 0)))
    assert grown["extract_alloc_count"][2] == math.inf


def _summary(base_count, head_count, base_peak=1000, head_peak=1000, head_only=None):
    base, head = _heap_doc(("a", base_count, base_peak)), _heap_doc(("a", head_count, head_peak))
    return measure.regression_summary(measure.heap_change(base, head), head, {}, {}, head_only)


def test_summary_fails_above_the_gate():
    text, failed = _summary(1000, 1011)
    assert failed
    assert "+1.10%" in text and "FAIL" in text
    text, failed = _summary(1000, 1010)
    assert not failed
    assert "+1.00%" in text and "FAIL" not in text


def test_summary_fails_on_peak_alone():
    _, failed = _summary(1000, 1000, base_peak=1000, head_peak=1020)
    assert failed


def test_summary_head_only_never_fails():
    text, failed = _summary(1, 10_000_000, head_only="no base artifact")
    assert not failed
    assert text.startswith("Head only: no base artifact.")
    assert "10000000" in text


def _write_estimates(root, bench, run, median):
    target = root / bench / run
    target.mkdir(parents=True, exist_ok=True)
    (target / "estimates.json").write_text(json.dumps({"median": {"point_estimate": median}}))


def test_criterion_medians_reads_new_estimates(tmp_path):
    _write_estimates(tmp_path, "extract/x", "new", 1500.0)
    _write_estimates(tmp_path, "extract/x", "base", 9999.0)
    assert measure.criterion_medians(tmp_path) == {"extract/x": 1500.0}
    assert measure.criterion_medians(tmp_path / "absent") == {}


def test_criterion_medians_names_a_malformed_file(tmp_path):
    _write_estimates(tmp_path, "extract/x", "new", 1.0)
    (tmp_path / "extract" / "x" / "new" / "estimates.json").write_text('{"median": {}}')
    with pytest.raises(ValueError, match="estimates.json"):
        measure.criterion_medians(tmp_path)


def _write_side(root, side, count):
    (root / side).mkdir(parents=True, exist_ok=True)
    (root / side / "heap-sample.json").write_text(json.dumps(_heap_doc(("a", count, 100))))


def test_compare_names_a_truncated_heap_file(tmp_path, capsys):
    _write_side(tmp_path, "base", 1000)
    _write_side(tmp_path, "head", 1000)
    (tmp_path / "head" / "heap-sample.json").write_text('{"pages": [{"id": "a", "extract_al')
    assert measure.compare(tmp_path, None) == 1
    err = capsys.readouterr().err
    assert "head" in err and "heap-sample.json" in err


def test_compare_names_a_page_without_the_gated_fields(tmp_path, capsys):
    _write_side(tmp_path, "head", 1000)
    (tmp_path / "base").mkdir()
    (tmp_path / "base" / "heap-sample.json").write_text(json.dumps({"pages": [{"id": "a"}]}))
    assert measure.compare(tmp_path, None) == 1
    err = capsys.readouterr().err
    assert "base" in err and "heap-sample.json" in err and "extract_alloc_count" in err


def test_compare_names_a_page_with_a_null_gated_field(tmp_path, capsys):
    _write_side(tmp_path, "head", 1000)
    (tmp_path / "base").mkdir()
    page = {"id": "a", "extract_alloc_count": None, "extract_peak_bytes": 100}
    (tmp_path / "base" / "heap-sample.json").write_text(json.dumps({"pages": [page]}))
    assert measure.compare(tmp_path, None) == 1
    err = capsys.readouterr().err
    assert "base" in err and "heap-sample.json" in err and "extract_alloc_count" in err


def test_compare_names_a_malformed_criterion_file(tmp_path, capsys):
    _write_side(tmp_path, "base", 1000)
    _write_side(tmp_path, "head", 1000)
    _write_estimates(tmp_path / "head" / "criterion", "extract/x", "new", 1.0)
    (tmp_path / "head" / "criterion" / "extract" / "x" / "new" / "estimates.json").write_text("{")
    assert measure.compare(tmp_path, None) == 1
    assert "estimates.json" in capsys.readouterr().err


def test_compare_fails_when_no_page_is_on_both_sides(tmp_path, capsys):
    (tmp_path / "base").mkdir()
    (tmp_path / "base" / "heap-sample.json").write_text(json.dumps(_heap_doc(("a", 1, 1))))
    (tmp_path / "head").mkdir()
    (tmp_path / "head" / "heap-sample.json").write_text(json.dumps(_heap_doc(("b", 1, 1))))
    assert measure.compare(tmp_path, None) == 1
    assert "no page is present on both sides" in capsys.readouterr().err
    assert measure.compare(tmp_path, "reason") == 0


def test_compare_names_the_script_when_a_file_is_missing(tmp_path, capsys):
    _write_side(tmp_path, "head", 1000)
    assert measure.compare(tmp_path, None) == 1
    assert "scripts/regression.sh" in capsys.readouterr().err


def test_compare_returns_one_on_failure(tmp_path):
    _write_side(tmp_path, "base", 1000)
    _write_side(tmp_path, "head", 1100)
    assert measure.compare(tmp_path, None) == 1
    _write_side(tmp_path, "head", 1000)
    assert measure.compare(tmp_path, None) == 0


def test_main_compare_needs_no_out(tmp_path):
    _write_side(tmp_path, "head", 1000)
    assert measure.main(["--compare", str(tmp_path), "--head-only", "x"]) == 0
    with pytest.raises(SystemExit):
        measure.main(["--merge"])


def test_main_rejects_an_empty_reason_and_head_only_without_compare(tmp_path, capsys):
    _write_side(tmp_path, "head", 1000)
    with pytest.raises(SystemExit):
        measure.main(["--compare", str(tmp_path), "--head-only", ""])
    assert "--head-only needs a non-empty REASON" in capsys.readouterr().err
    with pytest.raises(SystemExit):
        measure.main(["--merge", "--out", str(tmp_path), "--head-only", "x"])
    assert "--head-only needs --compare" in capsys.readouterr().err


def test_signed_has_no_negative_zero():
    assert measure._signed(-0.001) == "+0.00%"
    assert measure._signed(0.0) == "+0.00%"
    assert measure._signed(1.1) == "+1.10%"
    assert measure._signed(-0.35) == "-0.35%"


def _time_summary(base_times, head_times, head_only=None):
    heap = _heap_doc(("a", 1, 1))
    return measure.regression_summary(measure.heap_change(heap, heap), heap, base_times, head_times, head_only)[0]


def test_time_table_from_a_zero_base_does_not_divide():
    text = _time_summary({"g/x": 0.0}, {"g/x": 2e6})
    assert "| g/x | 0.000 | 2.000 | +inf% |" in text


def test_time_table_counts_benches_on_one_side_only():
    text = _time_summary({"g/x": 1e6, "g/old": 1e6}, {"g/x": 1e6, "g/new": 1e6, "g/newer": 1e6})
    assert "3 benches on one side only, not compared." in text
    assert "benches on one side only" not in _time_summary({"g/x": 1e6}, {"g/x": 1e6})
