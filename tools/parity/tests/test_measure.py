from parity import measure


def _side(time_ns, peak, retained=100, blocks=10, **extra):
    return {
        "input_bytes": 2048,
        "loaded": True,
        "median_ns": time_ns,
        "peak_bytes": peak,
        "retained_bytes": retained,
        "retained_blocks": blocks,
        **extra,
    }


def _rust(page_id, time_ns, peak):
    return {"id": page_id, "source": page_id, **_side(time_ns, peak, alloc_bytes=1000, alloc_count=5)}


def _python(page_id, time_ns, peak):
    return {"id": page_id, "source": page_id, **_side(time_ns, peak, retained=400, blocks=40)}


def test_merge_keeps_pages_on_both_sides():
    rust = {"pages": [_rust("a", 1, 1), _rust("b", 1, 1)]}
    python = {"pages": [_python("b", 2, 2), _python("c", 2, 2)]}
    assert [p["id"] for p in measure.merge_pages(rust, python)] == ["b"]


def test_totals_sum_time_and_take_peak_max():
    rust = {"pages": [_rust("a", 1_000_000, 100), _rust("b", 3_000_000, 300)]}
    python = {"pages": [_python("a", 4_000_000, 1000), _python("b", 8_000_000, 500)]}
    t = measure.totals(measure.merge_pages(rust, python))
    assert t["rust"]["median_ms"] == 4.0
    assert t["python"]["median_ms"] == 12.0
    assert t["rust"]["peak_max_bytes"] == 300
    assert t["python"]["peak_max_bytes"] == 1000
    assert t["rust"]["alloc_count"] == 10
    assert t["python"]["alloc_count"] is None


def test_table_lists_rows_then_totals_with_ratio():
    rust = {"pages": [_rust("a", 1_000_000, 2048), _rust("b", 1_000_000, 2048)]}
    python = {"pages": [_python("a", 5_000_000, 4096), _python("b", 5_000_000, 4096)]}
    table = measure.format_table(measure.merge_pages(rust, python), rows=1)
    assert "| a | 2.0 | 1.00 | 5.00 | 5.0x | 2 | 4 | 2.0x |" in table
    assert "| ... 1 more pages |" in table
    assert "| median time, sum (ms) | 2.0 | 10.0 | 5.0x |" in table
    assert "| allocation count, total | 10 | n/a | n/a |" in table


def test_table_without_rows_has_totals_only():
    pages = measure.merge_pages({"pages": [_rust("a", 1, 1)]}, {"pages": [_python("a", 2, 2)]})
    assert "| page |" not in measure.format_table(pages)


def test_ratio_of_zero_is_not_a_division_error():
    assert measure._ratio(5, 0) == "n/a"


def test_table_says_python_heap_is_a_lower_bound():
    pages = measure.merge_pages({"pages": [_rust("a", 1, 1)]}, {"pages": [_python("a", 2, 2)]})
    assert "lower bound" in measure.format_table(pages)
