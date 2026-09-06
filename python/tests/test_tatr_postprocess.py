"""TATR-* — unit tests for the vendored canonical TATR post-processing.

These exercise :mod:`pdfspine._tatr_postprocess` directly with small,
hand-built dict/list structures (rows/columns/headers/supercells/spans), and
assert on the documented Microsoft Table Transformer semantics.
"""

from __future__ import annotations

import pytest

from pdfspine import _tatr_postprocess as pp


def _cell(row_nums, column_nums, bbox, **extra):
    cell = {"row_nums": list(row_nums), "column_nums": list(column_nums), "bbox": bbox}
    cell.update(extra)
    return cell


# --- TATR-022: iou / iob numeric semantics ---
def test_tatr_022_iou_and_iob_numbers():
    assert pp.iou([0, 0, 10, 10], [5, 5, 15, 15]) == pytest.approx(25 / 225)
    # Disjoint boxes: empty intersection over a positive union.
    assert pp.iou([0, 0, 10, 10], [20, 20, 30, 30]) == 0
    # Degenerate boxes give a zero-area union -> the guarded ``return 0``.
    assert pp.iou([0, 0, 0, 0], [0, 0, 0, 0]) == 0
    assert pp.iob([0, 0, 10, 10], [0, 0, 5, 10]) == pytest.approx(0.5)
    # Zero-area bbox1 -> the guarded ``return 0``.
    assert pp.iob([0, 0, 0, 0], [1, 1, 2, 2]) == 0


# --- TATR-023: per-class score thresholding ---
def test_tatr_023_apply_class_thresholds_filters_by_label():
    bboxes = [[0, 0, 1, 1], [0, 0, 2, 2], [0, 0, 3, 3]]
    labels = [0, 1, 0]
    scores = [0.9, 0.2, 0.4]
    class_names = {0: "a", 1: "b"}
    class_thresholds = {"a": 0.5, "b": 0.1}
    kept_boxes, kept_scores, kept_labels = pp.apply_class_thresholds(
        bboxes, labels, scores, class_names, class_thresholds
    )
    assert kept_boxes == [[0, 0, 1, 1], [0, 0, 2, 2]]
    assert kept_scores == [0.9, 0.2]
    assert kept_labels == [0, 1]


# --- TATR-024: span text extraction, superscripts, flags, line breaks ---
def test_tatr_024_extract_text_superscripts_flags_and_breaks():
    spans = [
        {"text": "2", "flags": 1, "block_num": 0, "line_num": 0, "span_num": 0},
        {"text": "x", "flags": 1, "block_num": 0, "line_num": 0, "span_num": 1},
        {"text": "y", "flags": 0, "block_num": 0, "line_num": 0, "span_num": 2},
    ]
    # Integer superscript removed; non-integer superscript flagged and kept.
    assert pp.extract_text_from_spans(spans) == "x y"
    assert spans[1]["superscript"] is True

    # Every remaining span removed -> empty string.
    only_int = [{"text": "7", "flags": 1, "block_num": 0, "line_num": 0, "span_num": 0}]
    assert pp.extract_text_from_spans(only_int) == ""

    # join_with_space=False forces a single trailing space between lines.
    two_lines = [
        {"text": "foo", "block_num": 0, "line_num": 0, "span_num": 0},
        {"text": "bar", "block_num": 0, "line_num": 1, "span_num": 0},
    ]
    assert pp.extract_text_from_spans(two_lines, join_with_space=False) == "foo bar"


# --- TATR-025: slotting packages into containers ---
def test_tatr_025_slot_into_containers_empty_and_multislot():
    # Empty container or package set short-circuits.
    c_assign, p_assign, scores = pp.slot_into_containers([], [{"bbox": [0, 0, 1, 1]}])
    assert (c_assign, p_assign, scores) == ([], [[]], [])

    containers = [
        {"bbox": [0, 0, 100, 100]},
        {"bbox": [50, 0, 150, 100]},
        {"bbox": [200, 0, 300, 100]},
    ]
    package = [{"bbox": [0, 0, 100, 100]}]
    container_assignments, package_assignments, best = pp.slot_into_containers(
        containers,
        package,
        overlap_threshold=0.25,
        unique_assignment=False,
        forced_assignment=False,
    )
    # Package slots into both overlapping containers, but not the disjoint one.
    assert container_assignments == [[0], [0], []]
    assert package_assignments == [[0, 1]]
    assert best == [pytest.approx(1.0)]


# --- TATR-026: customizable NMS metrics and divide-by-zero recovery ---
def test_tatr_026_nms_metrics_and_zero_division():
    overlapping = [
        {"bbox": [0, 0, 10, 10], "score": 0.9},
        {"bbox": [0, 0, 10, 10], "score": 0.5},
    ]
    assert len(pp.nms(list(overlapping))) == 1
    assert len(pp.nms(list(overlapping), match_criteria="object1_overlap")) == 1
    assert len(pp.nms(list(overlapping), match_criteria="iou", match_threshold=0.5)) == 1
    # Degenerate object2 area -> ZeroDivisionError is swallowed, nothing removed.
    degenerate = [
        {"bbox": [0, 0, 10, 10], "score": 0.9},
        {"bbox": [5, 5, 5, 5], "score": 0.5},
    ]
    assert len(pp.nms(degenerate)) == 2
    assert pp.nms([]) == []


# --- TATR-027: NMS by shared containment ---
def test_tatr_027_nms_by_containment_suppresses_empty_containers():
    containers = [
        {"bbox": [0, 0, 100, 100], "score": 0.9},
        {"bbox": [0, 0, 100, 100], "score": 0.8},
        {"bbox": [500, 0, 600, 100], "score": 0.7},
    ]
    packages = [{"bbox": [10, 10, 50, 50]}]
    kept = pp.nms_by_containment(containers, packages, overlap_threshold=0.5)
    # Only the highest-score container keeps the shared package; the duplicate
    # and the empty container are both suppressed.
    assert kept == [containers[0]]


# --- TATR-028: token-free row/column refinement uses NMS ---
def test_tatr_028_refine_rows_columns_without_tokens():
    rows = [
        {"bbox": [0, 0, 100, 10], "score": 0.9},
        {"bbox": [0, 20, 100, 30], "score": 0.8},
    ]
    refined_rows = pp.refine_rows([dict(r) for r in rows], [], 0.5)
    assert len(refined_rows) == 2
    assert refined_rows[0]["bbox"][1] < refined_rows[1]["bbox"][1]

    columns = [
        {"bbox": [0, 0, 10, 100], "score": 0.9},
        {"bbox": [20, 0, 30, 100], "score": 0.8},
    ]
    refined_columns = pp.refine_columns([dict(c) for c in columns], [], 0.5)
    assert len(refined_columns) == 2
    assert refined_columns[0]["bbox"][0] < refined_columns[1]["bbox"][0]


# --- TATR-029: score sorting and overlap predicate edges ---
def test_tatr_029_sort_ascending_and_overlaps_zero_area():
    ordered = pp.sort_objects_by_score(
        [{"score": 1}, {"score": 3}, {"score": 2}], reverse=False
    )
    assert [obj["score"] for obj in ordered] == [1, 2, 3]
    # A zero-area bbox1 can never overlap anything.
    assert pp.overlaps([0, 0, 0, 0], [0, 0, 10, 10]) is False
    assert pp.overlaps([0, 0, 10, 10], [0, 0, 10, 10]) is True


# --- TATR-030: pruning content-free objects ---
def test_tatr_030_remove_objects_without_content():
    objects = [{"bbox": [0, 0, 10, 10]}, {"bbox": [50, 50, 60, 60]}]
    spans = [{"text": "hi", "bbox": [1, 1, 9, 9], "block_num": 0, "line_num": 0, "span_num": 0}]
    pp.remove_objects_without_content(spans, objects)
    assert objects == [{"bbox": [0, 0, 10, 10]}]


# --- TATR-031: alignment tolerates immutable bboxes ---
def test_tatr_031_align_rows_columns_recover_from_immutable_bbox(capsys):
    columns = [{"bbox": (0, 0, 10, 10)}]
    result = pp.align_columns(columns, [0, 0, 20, 20])
    assert result is columns
    rows = [{"bbox": (0, 0, 10, 10)}]
    assert pp.align_rows(rows, [0, 0, 20, 20]) is rows
    out = capsys.readouterr().out
    assert "Could not align columns" in out and "Could not align rows" in out


# --- TATR-032: header alignment prepends leading rows and stops at gaps ---
def test_tatr_032_align_headers_prepends_and_breaks():
    rows = [
        {"bbox": [0, 0, 100, 10]},
        {"bbox": [0, 10, 100, 20]},
        {"bbox": [0, 20, 100, 30]},
    ]
    headers = [{"bbox": [0, 20, 100, 30]}]
    aligned = pp.align_headers(headers, rows)
    assert len(aligned) == 1
    # Header hull grows to cover rows 0..2; every leading row is marked header.
    assert aligned[0]["bbox"] == [0, 0, 100, 30]
    assert [row["header"] for row in rows] == [True, True, True]

    # No intersecting rows -> no aligned header at all.
    assert pp.align_headers([{"bbox": [0, 500, 100, 510]}], rows) == []


def _grid_rows(headers=(False, False, False)):
    return [
        {"bbox": [0, 0, 300, 50], "header": headers[0]},
        {"bbox": [0, 50, 300, 100], "header": headers[1]},
        {"bbox": [0, 100, 300, 150], "header": headers[2]},
    ]


def _grid_columns():
    return [
        {"bbox": [0, 0, 100, 150]},
        {"bbox": [100, 0, 200, 150]},
        {"bbox": [200, 0, 300, 150]},
    ]


# --- TATR-033: a plain supercell spanning multiple rows and columns ---
def test_tatr_033_align_supercells_multirow_multicolumn():
    supercells = [{"bbox": [0, 50, 300, 150], "score": 0.9}]
    aligned = pp.align_supercells(supercells, _grid_rows(), _grid_columns())
    assert len(aligned) == 1
    cell = aligned[0]
    assert sorted(cell["row_numbers"]) == [1, 2]
    assert cell["column_numbers"] == [0, 1, 2]
    assert cell["header"] is False


# --- TATR-034: header-boundary conflict resolution ---
def test_tatr_034_align_supercells_header_conflict_resolution():
    rows = [
        {"bbox": [0, 0, 300, 50], "header": True},
        {"bbox": [0, 50, 300, 100], "header": True},
        {"bbox": [0, 100, 300, 150], "header": False},
        {"bbox": [0, 150, 300, 200], "header": False},
    ]
    columns = [{"bbox": [0, 0, 100, 200]}, {"bbox": [100, 0, 200, 200]}]
    supercells = [
        # More data rows than header rows -> becomes a data cell.
        {"bbox": [0, 50, 100, 200], "score": 0.9},
        # More header rows than data rows -> becomes a header cell.
        {"bbox": [100, 0, 200, 150], "score": 0.8},
    ]
    aligned = pp.align_supercells(supercells, rows, columns)
    assert len(aligned) == 2
    data_cell = next(c for c in aligned if sorted(c["row_numbers"]) == [2, 3])
    header_cell = next(c for c in aligned if sorted(c["row_numbers"]) == [0, 1])
    assert data_cell["header"] is False
    assert header_cell["header"] is True


# --- TATR-035: span supercell in the header propagates ancestors ---
def test_tatr_035_align_supercells_span_header_propagation():
    rows = _grid_rows(headers=(True, True, False))
    columns = _grid_columns()
    supercells = [{"bbox": [0, 50, 200, 100], "span": True, "score": 0.9}]
    aligned = pp.align_supercells(supercells, rows, columns)
    assert len(aligned) == 2
    primary = next(c for c in aligned if not c.get("propagated"))
    propagated = next(c for c in aligned if c.get("propagated"))
    assert primary["header"] is True
    assert primary["column_numbers"] == [0, 1]
    assert propagated["row_numbers"] == [0]
    assert propagated["column_numbers"] == [0, 1]


# --- TATR-036: supercells with no anchor rows/columns are dropped ---
def test_tatr_036_align_supercells_drops_unanchored():
    # A span cell that never reaches the header is discarded.
    span_data = [{"bbox": [0, 100, 100, 200], "span": True, "score": 0.9}]
    assert pp.align_supercells(span_data, _grid_rows(), _grid_columns()) == []
    # A cell intersecting no rows at all is discarded.
    no_rows = [{"bbox": [0, 1000, 100, 1100], "score": 0.9}]
    assert pp.align_supercells(no_rows, _grid_rows(), _grid_columns()) == []
    # A cell intersecting rows but no columns is discarded.
    no_cols = [{"bbox": [1000, 50, 1100, 100], "score": 0.9}]
    assert pp.align_supercells(no_cols, _grid_rows(), _grid_columns()) == []


# --- TATR-037: supercell NMS shrinks then suppresses ---
def test_tatr_037_nms_supercells_shrinks_and_suppresses():
    supercells = [
        {"score": 0.9, "row_numbers": [0, 1], "column_numbers": [0, 1]},
        {"score": 0.5, "row_numbers": [0], "column_numbers": [0, 1]},
    ]
    kept = pp.nms_supercells(supercells)
    assert kept == [supercells[0]]


# --- TATR-038: overlap removal shrinks the lower-confidence supercell ---
def test_tatr_038_remove_supercell_overlap_branches():
    # Fewer rows than columns -> remove overlapping columns (max then min).
    high = {"row_numbers": [0], "column_numbers": [1, 2]}
    low = {"row_numbers": [0], "column_numbers": [0, 1, 2]}
    pp.remove_supercell_overlap(high, low)
    assert low["column_numbers"] == [0]

    # More rows than columns -> remove overlapping rows (max then min).
    high_r = {"row_numbers": [1, 2], "column_numbers": [0]}
    low_r = {"row_numbers": [0, 1, 2], "column_numbers": [0]}
    pp.remove_supercell_overlap(high_r, low_r)
    assert low_r["row_numbers"] == [0]

    # Overlap at the low columns -> the ``min_column`` branch is taken.
    high_min = {"row_numbers": [0], "column_numbers": [0, 1]}
    low_min = {"row_numbers": [0], "column_numbers": [0, 1, 2]}
    pp.remove_supercell_overlap(high_min, low_min)
    assert low_min["column_numbers"] == [2]

    # Overlap at the low rows -> the ``min_row`` branch is taken.
    high_minr = {"row_numbers": [0, 1], "column_numbers": [0]}
    low_minr = {"row_numbers": [0, 1, 2], "column_numbers": [0]}
    pp.remove_supercell_overlap(high_minr, low_minr)
    assert low_minr["row_numbers"] == [2]

    # Overlap only at an interior column -> the whole axis is cleared.
    high_i = {"row_numbers": [0], "column_numbers": [1, 2]}
    low_i = {"row_numbers": [0], "column_numbers": [0, 1, 2, 3]}
    pp.remove_supercell_overlap(high_i, low_i)
    assert low_i["column_numbers"] == []

    # Overlap only at an interior row -> the whole axis is cleared.
    high_ir = {"row_numbers": [1, 2], "column_numbers": [0]}
    low_ir = {"row_numbers": [0, 1, 2, 3], "column_numbers": [0]}
    pp.remove_supercell_overlap(high_ir, low_ir)
    assert low_ir["row_numbers"] == []


# --- TATR-039: header supercell tree enforces single parents ---
def test_tatr_039_header_supercell_tree_single_parent():
    a = {"header": True, "row_numbers": [0], "column_numbers": [0, 1], "score": 0.9}
    b = {"header": True, "row_numbers": [0], "column_numbers": [0, 1], "score": 0.8}
    c = {"header": True, "row_numbers": [1], "column_numbers": [0, 1], "score": 0.7}
    supercells = [a, b, c]
    pp.header_supercell_tree(supercells)
    # C has two parents in row 0 -> it is removed.
    assert c not in supercells
    assert a in supercells and b in supercells

    # With exactly one parent the child survives.
    d = {"header": True, "row_numbers": [0], "column_numbers": [0, 1], "score": 0.9}
    e = {"header": True, "row_numbers": [1], "column_numbers": [0, 1], "score": 0.7}
    valid = [d, e]
    pp.header_supercell_tree(valid)
    assert valid == [d, e]


# --- TATR-040: cell export with no page tokens yields zero confidence ---
def test_tatr_040_table_structure_to_cells_empty_spans():
    structures = {
        "columns": [{"bbox": [0, 0, 50, 100]}, {"bbox": [50, 0, 100, 100]}],
        "rows": [
            {"bbox": [0, 0, 100, 50], "header": False},
            {"bbox": [0, 50, 100, 100], "header": False},
        ],
        "supercells": [],
    }
    cells, confidence = pp.table_structure_to_cells(structures, [], [0, 0, 100, 100])
    assert len(cells) == 4
    assert confidence == 0
    assert all(cell["cell_text"] == "" for cell in cells)


# --- TATR-041: end-to-end objects_to_cells, valid and invalid ---
def test_tatr_041_objects_to_cells_valid_and_invalid():
    class_map = {
        0: "table column",
        1: "table row",
        2: "table column header",
        3: "table spanning cell",
        4: "table projected row header",
        5: "no object",
    }
    thresholds = {
        "table column": 0.0,
        "table row": 0.0,
        "table column header": 0.0,
        "table spanning cell": 0.0,
        "table projected row header": 0.0,
    }
    table = {"bbox": [0, 0, 200, 100], "page_num": 0}
    objects = [
        {"label": 0, "score": 0.9, "bbox": [0, 0, 100, 100]},
        {"label": 0, "score": 0.9, "bbox": [100, 0, 200, 100]},
        {"label": 1, "score": 0.9, "bbox": [0, 0, 200, 50]},
        {"label": 1, "score": 0.9, "bbox": [0, 50, 200, 100]},
        {"label": 4, "score": 0.9, "bbox": [0, 50, 200, 100]},
    ]
    tokens = [
        {"bbox": [10, 10, 50, 40], "text": "A", "block_num": 0, "line_num": 0, "span_num": 0},
        {"bbox": [110, 10, 150, 40], "text": "B", "block_num": 0, "line_num": 0, "span_num": 1},
        {"bbox": [10, 60, 50, 90], "text": "C", "block_num": 1, "line_num": 0, "span_num": 0},
        {"bbox": [110, 60, 150, 90], "text": "D", "block_num": 1, "line_num": 0, "span_num": 1},
    ]
    structures, cells, confidence = pp.objects_to_cells(
        table, objects, tokens, class_map, thresholds
    )
    assert len(structures["columns"]) == 2
    assert len(structures["rows"]) == 2
    # Row 1 carries a projected row header, so its two base cells merge into a
    # single spanning subheader cell (2 header cells + 1 spanning row = 3).
    assert len(cells) == 3
    assert any(sorted(cell["column_nums"]) == [0, 1] for cell in cells)
    assert isinstance(confidence, float)

    # No rows/columns -> empty cells and zero confidence.
    empty_structures, empty_cells, empty_conf = pp.objects_to_cells(
        {"bbox": [0, 0, 200, 100], "page_num": 0}, [], tokens, class_map, thresholds
    )
    assert empty_cells == []
    assert empty_conf == 0
