"""TBLEVAL-* — offline tests for the table evaluation-set helpers.

``conformance/gt/table_metrics.py`` (TEDS-Struct, cell-alignment F1) and
``conformance/gt/table_gold.py`` (gold loading, HTML <-> cells, tags) are
pure-stdlib scripts outside the package, so they are loaded from the repo root
with ``importlib`` — the same trick ``test_onnx_tables.py`` uses for its
fixture module. No FinTabNet corpus is touched; every gold file is built in
``tmp_path``.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[2]
_GT_DIR = _REPO_ROOT / "conformance" / "gt"


def _load(name: str):
    spec = importlib.util.spec_from_file_location(
        f"_pdfspine_tbleval_{name}", _GT_DIR / f"{name}.py"
    )
    module = importlib.util.module_from_spec(spec)
    # dataclasses resolve string annotations through sys.modules[__module__].
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


metrics = _load("table_metrics")
gold = _load("table_gold")


def _cell(rows, cols, text="", bbox=None, header=False):
    return {
        "row_nums": list(rows),
        "column_nums": list(cols),
        "cell_text": text,
        "bbox": bbox,
        "header": header,
    }


def _grid(n_rows, n_cols, *, size=10.0):
    """Simple n_rows x n_cols grid with disjoint unit bboxes."""
    return [
        _cell(
            [r],
            [c],
            f"r{r}c{c}",
            [c * size, r * size, (c + 1) * size, (r + 1) * size],
        )
        for r in range(n_rows)
        for c in range(n_cols)
    ]


def _labels(node):
    return [node.label] + [
        [child.label] + [[g.label for g in child.children]] for child in node.children
    ]


def _structure(cells):
    return sorted(
        (
            tuple(c["row_nums"]),
            tuple(c["column_nums"]),
            c["cell_text"],
            bool(c.get("header")),
        )
        for c in cells
    )


# --------------------------------------------------------------------------- #
# TBLEVAL-001: cells -> structure tree (td labels, spans, rowspan once)
# --------------------------------------------------------------------------- #
def test_tbleval_001_cells_to_structure_tree():
    tree = metrics.cells_to_structure_tree(_grid(2, 2))
    assert tree.label == "table"
    assert [tr.label for tr in tree.children] == ["tr", "tr"]
    assert [[td.label for td in tr.children] for tr in tree.children] == [
        ["td", "td"],
        ["td", "td"],
    ]
    assert tree.size() == 7

    # Header cell spanning both columns, then a 2-row rowspan in column 0.
    cells = [
        _cell([0], [0, 1], "hdr"),
        _cell([1, 2], [0], "tall"),
        _cell([1], [1], "b"),
        _cell([2], [1], "d"),
        _cell([3], [0, 1], "x"),
    ]
    tree = metrics.cells_to_structure_tree(cells)
    rows = [[td.label for td in tr.children] for tr in tree.children]
    assert rows == [
        ["td[colspan=2]"],
        ["td[rowspan=2]", "td"],
        ["td"],  # the rowspan cell is NOT repeated on its second row
        ["td[colspan=2]"],
    ]

    both = metrics.cells_to_structure_tree([_cell([0, 1], [0, 1, 2], "big")])
    assert [[td.label for td in tr.children] for tr in both.children] == [
        ["td[colspan=3 rowspan=2]"],
        [],
    ]

    # Empty cell list -> bare root; unordered input is sorted by first column.
    assert metrics.cells_to_structure_tree([]).size() == 1
    shuffled = metrics.cells_to_structure_tree([_cell([0], [2]), _cell([0], [0, 1])])
    assert [td.label for td in shuffled.children[0].children] == ["td[colspan=2]", "td"]


# --------------------------------------------------------------------------- #
# TBLEVAL-002: Zhang-Shasha known answers
# --------------------------------------------------------------------------- #
def test_tbleval_002_tree_edit_distance_known_answers():
    Node = metrics.Node
    ted = metrics.tree_edit_distance

    a = metrics.cells_to_structure_tree(_grid(2, 3))
    assert ted(a, a) == 0

    relabel = Node("table", [Node("tr", [Node("td"), Node("td[colspan=2]")])])
    base = Node("table", [Node("tr", [Node("td"), Node("td")])])
    assert ted(base, relabel) == 1
    assert ted(relabel, base) == 1

    plus_leaf = Node("table", [Node("tr", [Node("td"), Node("td"), Node("td")])])
    assert ted(base, plus_leaf) == 1
    assert ted(plus_leaf, base) == 1

    # Empty root vs an n-node tree: every extra node is one insertion.
    assert ted(Node("table"), a) == a.size() - 1
    assert ted(a, Node("table")) == a.size() - 1

    # Classic Zhang-Shasha textbook pair: f(d(a c(b)) e) vs f(c(d(a b)) e) = 2.
    t1 = Node("f", [Node("d", [Node("a"), Node("c", [Node("b")])]), Node("e")])
    t2 = Node("f", [Node("c", [Node("d", [Node("a"), Node("b")])]), Node("e")])
    assert ted(t1, t2) == 2

    # Symmetric and respects the triangle-free unit cost on totally different labels.
    x = Node("x", [Node("y"), Node("z")])
    y = Node("p", [Node("q"), Node("r")])
    assert ted(x, y) == ted(y, x) == 3


# --------------------------------------------------------------------------- #
# TBLEVAL-003: TEDS-Struct score + guard rails
# --------------------------------------------------------------------------- #
def test_tbleval_003_teds_struct_scores_and_guards():
    g = _grid(3, 3)
    assert metrics.teds_struct(g, g) == pytest.approx(1.0)

    # Text differences are invisible to the structure variant.
    renamed = [dict(c, cell_text="zzz") for c in g]
    assert metrics.teds_struct(g, renamed) == pytest.approx(1.0)

    # One cell merged into a colspan: exactly one relabel + one delete.
    merged = [c for c in g if not (c["row_nums"] == [0] and c["column_nums"] == [1])]
    merged = [
        (
            dict(c, column_nums=[0, 1])
            if c["row_nums"] == [0] and c["column_nums"] == [0]
            else c
        )
        for c in merged
    ]
    score = metrics.teds_struct(g, merged)
    assert score is not None
    assert 0.0 < score < 1.0
    n_true = metrics.cells_to_structure_tree(g).size()
    assert score == pytest.approx(1.0 - 2 / n_true)

    assert metrics.teds_struct([], []) == pytest.approx(1.0)
    empty_vs_full = metrics.teds_struct([], g)
    assert empty_vs_full is not None
    assert empty_vs_full == pytest.approx(1 / n_true)

    assert metrics.teds_struct(g, g, max_nodes=5) is None
    assert metrics.teds_struct(g, g, timeout_s=0) is None
    assert metrics.teds_struct(g, g, timeout_s=None) == pytest.approx(1.0)


# --------------------------------------------------------------------------- #
# TBLEVAL-004: cell-alignment F1 matching, text gate, edge conventions
# --------------------------------------------------------------------------- #
def test_tbleval_004_cell_alignment_f1():
    g = _grid(2, 2)
    perfect = metrics.cell_alignment(g, [dict(c) for c in g])
    assert perfect["f1"] == pytest.approx(1.0)
    assert perfect["precision"] == pytest.approx(1.0)
    assert perfect["recall"] == pytest.approx(1.0)
    assert perfect["matched"] == 4
    assert perfect["n_true"] == perfect["n_pred"] == 4
    assert perfect["iou_threshold"] == 0.5

    # Half-overlap (IoU 1/3) is below the 0.5 threshold -> no match; lowering
    # the threshold makes it count.
    shifted = [
        dict(c, bbox=[c["bbox"][0] + 5, c["bbox"][1], c["bbox"][2] + 5, c["bbox"][3]])
        for c in g
    ]
    res = metrics.cell_alignment(g, shifted)
    assert res["matched"] == 0
    assert res["f1"] == 0.0
    res_low = metrics.cell_alignment(g, shifted, iou_threshold=0.3)
    assert res_low["matched"] == 4

    # Greedy one-to-one: two predictions on one gold box -> one match only.
    dup = [dict(g[0]), dict(g[0], cell_text="dupe")]
    res = metrics.cell_alignment(g, dup)
    assert res["matched"] == 1
    assert res["precision"] == pytest.approx(0.5)
    assert res["recall"] == pytest.approx(0.25)
    assert res["f1"] == pytest.approx(2 * 0.5 * 0.25 / 0.75)

    # Text gate: same geometry, different (but whitespace/case-equivalent) text.
    pred = [dict(c, cell_text="  " + c["cell_text"].upper() + " ") for c in g]
    assert metrics.cell_alignment(g, pred, require_text_match=True)["matched"] == 4
    pred[0]["cell_text"] = "wrong"
    assert metrics.cell_alignment(g, pred, require_text_match=True)["matched"] == 3
    assert metrics.cell_alignment(g, pred, require_text_match=False)["matched"] == 4

    # A side with no bbox at all is skipped, not scored 0.
    nobox = [dict(c, bbox=None) for c in g]
    res = metrics.cell_alignment(g, nobox)
    assert res["f1"] is None and res["precision"] is None and res["recall"] is None
    assert res["skipped_reason"] == "no bbox"
    res = metrics.cell_alignment(nobox, g)
    assert res["f1"] is None and res["skipped_reason"] == "no bbox"
    # ...but a single bbox-less cell on an otherwise boxed side just fails to match.
    partial = [dict(c) for c in g]
    partial[0]["bbox"] = None
    res = metrics.cell_alignment(g, partial)
    assert res["matched"] == 3 and "skipped_reason" not in res

    assert metrics.cell_alignment([], [])["f1"] == 1.0
    assert metrics.cell_alignment(g, [])["f1"] == 0.0
    assert metrics.cell_alignment([], g)["f1"] == 0.0


# --------------------------------------------------------------------------- #
# TBLEVAL-005: HTML -> cells (colspan, rowspan shift, th) and round trip
# --------------------------------------------------------------------------- #
def test_tbleval_005_cells_from_html_and_round_trip():
    html = """
    <table>
      <tr><th colspan="2">Group</th><th>Total</th></tr>
      <tr><td rowspan="2">Label</td><td>1</td><td>2 &amp; 3</td></tr>
      <tr><td>4</td><td>5<br>x</td></tr>
    </table>
    """
    cells = gold.cells_from_html(html)
    assert _structure(cells) == [
        ((0,), (0, 1), "Group", True),
        ((0,), (2,), "Total", True),
        ((1,), (1,), "1", False),
        ((1,), (2,), "2 & 3", False),
        ((1, 2), (0,), "Label", False),
        ((2,), (1,), "4", False),  # shifted right by the rowspan placeholder
        ((2,), (2,), "5 x", False),
    ]
    assert all(c["bbox"] is None for c in cells)
    assert gold.cells_from_html("<p>no table</p>") == []

    # Round trip keeps structure, text and header flags; escaping survives.
    out = gold.cells_to_html(cells)
    assert out.startswith("<table>") and out.endswith("</table>")
    assert '<th colspan="2">Group</th>' in out
    assert '<td rowspan="2">Label</td>' in out
    assert "2 &amp; 3" in out
    assert _structure(gold.cells_from_html(out)) == _structure(cells)

    # data-bbox round-trips geometry for exported drafts.
    boxed = _grid(1, 2)
    again = gold.cells_from_html(gold.cells_to_html(boxed))
    assert [c["bbox"] for c in again] == [c["bbox"] for c in boxed]

    # Two tables in one document.
    two = gold.tables_from_html(
        "<table><tr><td>a</td></tr></table><table><tr><td>b</td><td>c</td></tr></table>"
    )
    assert [len(t) for t in two] == [1, 2]


# --------------------------------------------------------------------------- #
# TBLEVAL-006: *.gold.json (both cell notations) and *.gold.html loading
# --------------------------------------------------------------------------- #
def test_tbleval_006_load_gold_table_file(tmp_path):
    long_form = {
        "document_id": "acme",
        "page_index": 3,
        "tables": [
            {
                "table_id": "t-long",
                "bbox": [0, 0, 100, 50],
                "tags": ["plain"],
                "cells": [
                    {
                        "row_nums": [0],
                        "column_nums": [0, 1],
                        "cell_text": "Hdr",
                        "bbox": [0, 0, 20, 10],
                        "header": True,
                    },
                    {
                        "row_nums": [1, 2],
                        "column_nums": [0],
                        "cell_text": "Tall",
                        "bbox": [0, 10, 10, 30],
                    },
                    {"row_nums": [1], "column_nums": [1], "cell_text": "v1"},
                    {"row_nums": [2], "column_nums": [1], "cell_text": "v2"},
                ],
            }
        ],
    }
    short_form = {
        "document_id": "acme",
        "page_index": 3,
        "tables": [
            {
                "table_id": "t-short",
                "bbox": [0, 0, 100, 50],
                "cells": [
                    {
                        "row": 0,
                        "col": 0,
                        "colspan": 2,
                        "text": "Hdr",
                        "bbox": [0, 0, 20, 10],
                        "header": True,
                    },
                    {
                        "row": 1,
                        "col": 0,
                        "rowspan": 2,
                        "text": "Tall",
                        "bbox": [0, 10, 10, 30],
                    },
                    {"row": 1, "col": 1, "text": "v1"},
                    {"row": 2, "col": 1, "text": "v2"},
                ],
            }
        ],
    }
    p_long = tmp_path / "acme.gold.json"
    p_short = tmp_path / "acme-short.gold.json"
    p_long.write_text(json.dumps(long_form), encoding="utf-8")
    p_short.write_text(json.dumps(short_form), encoding="utf-8")

    (t_long,) = gold.load_gold_table_file(p_long)
    (t_short,) = gold.load_gold_table_file(p_short)
    assert isinstance(t_long, gold.GoldTable)
    assert t_long.table_id == "t-long" and t_short.table_id == "t-short"
    assert t_long.cells == t_short.cells
    assert t_long.bbox == [0.0, 0.0, 100.0, 50.0]
    assert (t_long.n_rows, t_long.n_cols) == (3, 2)
    assert t_long.cells[0]["header"] is True and t_long.cells[1]["header"] is False
    assert t_long.cells[2]["bbox"] is None
    # Declared tags are kept and merged with the heuristics.
    assert t_long.tags == ["plain", "multi-header", "spanning"]
    assert t_short.tags == ["multi-header", "spanning"]

    page = gold.load_gold_page_file(p_long)
    assert page.document_id == "acme" and page.page_index == 3 and page.pdf is None

    p_html = tmp_path / "acme.gold.html"
    p_html.write_text(
        "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
        encoding="utf-8",
    )
    (t_html,) = gold.load_gold_table_file(p_html)
    assert t_html.bbox is None
    assert (t_html.n_rows, t_html.n_cols) == (2, 2)
    assert _structure(t_html.cells) == [
        ((0,), (0,), "A", True),
        ((0,), (1,), "B", True),
        ((1,), (0,), "1", False),
        ((1,), (1,), "2", False),
    ]
    assert gold.load_gold_page_file(p_html).document_id == "acme"

    with pytest.raises(ValueError):
        gold.load_gold_table_file(tmp_path / "acme.txt")


# --------------------------------------------------------------------------- #
# TBLEVAL-007: manifest loading with stale absolute paths + structure filter
# --------------------------------------------------------------------------- #
def test_tbleval_007_load_manifest_path_fallback_and_filter(tmp_path):
    corpus = tmp_path / "corpus"
    (corpus / "annotations").mkdir(parents=True)
    (corpus / "pdfs").mkdir()
    stale = "/nonexistent/old-checkout/conformance/gt/corpus-fintabnet"

    def anno_table(exclude, text_json, text_pdf):
        return {
            "structure_id": "s1" if not exclude else "s-excluded",
            "exclude_for_structure": exclude,
            "pdf_table_bbox": [10, 20, 300, 400],
            "rows": {"0": {"is_column_header": True}, "1": {"is_column_header": False}},
            "columns": {"0": {}, "1": {}},
            "cells": [
                {
                    "row_nums": [0],
                    "column_nums": [0],
                    "is_column_header": True,
                    "json_text_content": text_json,
                    "pdf_text_content": text_pdf,
                    "pdf_bbox": [10, 20, 100, 30],
                },
                {
                    "row_nums": [0],
                    "column_nums": [1],
                    "is_column_header": True,
                    "pdf_text_content": "only pdf text",
                    "pdf_bbox": [100, 20, 300, 30],
                },
                {
                    "row_nums": [1],
                    "column_nums": [0],
                    "json_text_content": "a",
                    "pdf_bbox": [10, 30, 100, 40],
                },
                {
                    "row_nums": [1],
                    "column_nums": [1],
                    "json_text_content": "b",
                    "pdf_bbox": [100, 30, 300, 40],
                },
            ],
        }

    (corpus / "annotations" / "DOC_1_tables.json").write_text(
        json.dumps(
            [anno_table(False, "json wins", "pdf loses"), anno_table(True, "x", "y")]
        ),
        encoding="utf-8",
    )
    (corpus / "pdfs" / "DOC_1.pdf").write_bytes(b"%PDF-1.4\n")
    (corpus / "annotations" / "DOC_2_tables.json").write_text(
        json.dumps([anno_table(False, "t2", "t2")]), encoding="utf-8"
    )
    manifest = {
        "dataset": "FinTabNet.c",
        "entries": [
            {
                "document_id": "DOC_1",
                "annotation": f"{stale}/annotations/DOC_1_tables.json",
                "pdf": f"{stale}/pdfs/DOC_1.pdf",
                "pdf_page_index": 2,
                "anno_license": "CDLA-Permissive-2.0",
                "pdf_license": "CDLA-Permissive-1.0",
            },
            {  # annotation exists via fallback; PDF exists nowhere -> pdf=None
                "document_id": "DOC_2",
                "annotation": f"{stale}/annotations/DOC_2_tables.json",
                "pdf": f"{stale}/pdfs/DOC_2.pdf",
                "pdf_page_index": 0,
            },
            {  # annotation exists nowhere -> skipped
                "document_id": "DOC_3",
                "annotation": f"{stale}/annotations/DOC_3_tables.json",
                "pdf": None,
            },
        ],
    }
    mpath = corpus / "manifest.json"
    mpath.write_text(json.dumps(manifest), encoding="utf-8")

    skipped: list[str] = []
    pages = gold.load_manifest(mpath, skipped=skipped)
    assert [p.document_id for p in pages] == ["DOC_1", "DOC_2"]
    assert skipped == ["DOC_3"]

    p1 = pages[0]
    assert isinstance(p1, gold.GoldPage)
    assert p1.pdf == corpus / "pdfs" / "DOC_1.pdf"
    assert p1.page_index == 2
    assert p1.anno_license == "CDLA-Permissive-2.0"
    assert p1.pdf_license == "CDLA-Permissive-1.0"
    assert [t.table_id for t in p1.tables] == ["s1"]  # excluded table filtered
    t = p1.tables[0]
    assert t.bbox == [10.0, 20.0, 300.0, 400.0]
    assert (t.n_rows, t.n_cols) == (2, 2)
    assert t.cells[0]["cell_text"] == "json wins"  # json_text_content preferred
    assert t.cells[1]["cell_text"] == "only pdf text"  # pdf_text_content fallback
    assert t.cells[0]["bbox"] == [10.0, 20.0, 100.0, 30.0]
    assert t.cells[0]["header"] is True and t.cells[2]["header"] is False

    assert pages[1].pdf is None

    # A relative annotation path resolves against the manifest directory.
    manifest["entries"] = [
        {"document_id": "DOC_2", "annotation": "annotations/DOC_2_tables.json"}
    ]
    mpath.write_text(json.dumps(manifest), encoding="utf-8")
    (rel_page,) = gold.load_manifest(mpath)
    assert rel_page.document_id == "DOC_2" and len(rel_page.tables) == 1


# --------------------------------------------------------------------------- #
# TBLEVAL-008: page_tags heuristics
# --------------------------------------------------------------------------- #
def test_tbleval_008_page_tags():
    def table(cells, table_id="t"):
        n_rows, n_cols = gold.grid_shape(cells)
        return gold.GoldTable(
            table_id=table_id, bbox=None, cells=cells, n_rows=n_rows, n_cols=n_cols
        )

    def page(*tables):
        return gold.GoldPage(
            document_id="d", pdf=None, page_index=0, tables=list(tables)
        )

    plain = table(_grid(3, 3))
    assert gold.page_tags(page(plain)) == ["plain"]
    assert gold.page_tags(page(plain), lines_detected=True) == ["plain"]
    assert gold.page_tags(page(plain), lines_detected=False) == ["plain", "borderless"]

    spanning = table(
        [_cell([0, 1], [0], "a"), _cell([0], [1], "b"), _cell([1], [1], "c")]
    )
    assert gold.page_tags(page(spanning)) == ["spanning"]

    # Two header rows -> multi-header (no spans involved).
    two_hdr_rows = table(
        [
            _cell([0], [0], "h", header=True),
            _cell([1], [0], "h2", header=True),
            _cell([2], [0], "v"),
        ]
    )
    assert gold.page_tags(page(two_hdr_rows)) == ["multi-header"]
    # A grouped (colspan) header cell alone is also multi-header (and spanning).
    grouped = table(
        [
            _cell([0], [0, 1], "g", header=True),
            _cell([1], [0], "a"),
            _cell([1], [1], "b"),
        ]
    )
    assert gold.page_tags(page(grouped)) == ["multi-header", "spanning"]

    assert gold.page_tags(page(table(_grid(2, 8)))) == ["wide"]
    assert gold.page_tags(page(table(_grid(2, 7)))) == ["plain"]
    assert gold.page_tags(page(table(_grid(20, 2)))) == ["tall"]
    assert gold.page_tags(page(table(_grid(19, 2)))) == ["plain"]

    assert gold.page_tags(page(plain, table(_grid(2, 2), "t2"))) == [
        "plain",
        "multi-table",
    ]
    assert gold.page_tags(
        page(spanning, table(_grid(20, 8), "t2")), lines_detected=False
    ) == [
        "borderless",
        "spanning",
        "wide",
        "tall",
        "multi-table",
    ]
    assert set(gold.page_tags(page(plain, spanning), lines_detected=False)) <= set(
        gold.TAG_VOCABULARY
    )


@pytest.mark.parametrize("field", ["serialization_error", "extract_error", "bbox_error"])
def test_legacy_eval_rejects_record_errors(field):
    harness = _load("eval_tables")
    with pytest.raises(ValueError, match=field):
        harness._slim_record({field: "failed", "cells": []})


@pytest.mark.parametrize("rows", [[], [-1], [True], [0.5]])
def test_legacy_eval_rejects_invalid_direct_cells(rows):
    harness = _load("eval_tables")
    with pytest.raises(ValueError, match="invalid predicted cell"):
        harness._pred_cells({"cells": [_cell(rows, [0], "bad")]})


def test_legacy_eval_explicit_empty_cells_do_not_fallback_to_html():
    harness = _load("eval_tables")
    record = {"cells": [], "html": "<table><tr><td>fabricated</td></tr></table>"}
    assert harness._pred_cells(record) == []


def test_table_record_explicit_empty_spans_do_not_fallback_to_rows():
    from types import SimpleNamespace

    harness = _load("tables_diff")
    table = SimpleNamespace(
        bbox=(0, 0, 10, 10),
        row_count=1,
        col_count=1,
        spans=[],
        rows=[SimpleNamespace(cells=[(0, 0, 10, 10)])],
        extract=lambda: [["fallback"]],
    )
    assert harness._table_record(table)["cells"] == []
