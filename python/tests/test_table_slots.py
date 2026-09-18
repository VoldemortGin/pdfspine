"""Typed table slots agree across native and both offline vision adapters.

The PDF is generated with the public drawing/text API. Only model inference
and rasterization are replaced for vision backends: public dispatch, cropping,
TATR post-processing, ONNX token decoding, and text assignment all run normally.
No model weights or optional vision runtime are required.
"""

from __future__ import annotations

from dataclasses import FrozenInstanceError
from types import SimpleNamespace
from typing import get_type_hints

import pdfspine
import pytest
from pdfspine import _onnx, _tatr


# A third, populated row keeps TATR from pruning a structurally valid column
# whose first row is merged and whose second row is blank.
_CASES = {
    "present": ((), ("ppp", "ppp", "ppp")),
    "blank": ((), ("ppp", "ppb", "ppp")),
    "no_text": ((), ("uuu", "uuu", "uuu")),
    "outside_text": ((), ("bbb", "bbb", "bbb")),
    "rowspan": (((0, 0, 2, 1),), ("ppp", "cpp", "ppp")),
    "colspan": (((0, 1, 1, 2),), ("ppc", "ppp", "ppp")),
    "mixed": (((0, 0, 2, 1), (0, 1, 1, 2)), ("ppc", "cpb", "ppp")),
    "blank_merge": (((0, 0, 2, 1), (0, 1, 1, 2)), ("bpc", "cpb", "ppp")),
    "no_text_merged": (
        ((0, 0, 2, 1), (0, 1, 1, 2)),
        ("uuc", "cuu", "uuu"),
    ),
}
_STATE = {
    "p": "present",
    "b": "blank",
    "u": "unavailable",
    "c": "continuation",
}
_TABLE_BBOX = (100.0, 92.0, 400.0, 182.0)


def _origins(case):
    merges, states = _CASES[case]
    spans = {(row, col): (rs, cs) for row, col, rs, cs in merges}
    return [
        (row, col, *spans.get((row, col), (1, 1)))
        for row in range(3)
        for col in range(3)
        if states[row][col] != "c"
    ]


def _bbox(row, col, row_span=1, col_span=1):
    return (
        100.0 + 100.0 * col,
        92.0 + 30.0 * row,
        100.0 + 100.0 * (col + col_span),
        92.0 + 30.0 * (row + row_span),
    )


def _page(case):
    merges, states = _CASES[case]
    page = pdfspine.open().new_page(width=612, height=792)
    for row in range(4):
        x0 = 200 if row == 1 and (0, 0, 2, 1) in merges else 100
        page.draw_line((x0, 92 + 30 * row), (400, 92 + 30 * row))
    for col in range(4):
        y0 = 122 if col == 2 and (0, 1, 1, 2) in merges else 92
        page.draw_line((100 + 100 * col, y0), (100 + 100 * col, 182))
    for row in range(3):
        for col in range(3):
            if states[row][col] == "p":
                page.insert_text(
                    (110 + 100 * col, 107 + 30 * row),
                    f"R{row}C{col}",
                    fontsize=10,
                )
    if case == "outside_text":
        page.insert_text((72, 50), "Text outside the table", fontsize=10)
    return page


class _Image:
    def __init__(self, size=(612, 792)):
        self.size = size

    def crop(self, bbox):
        return _Image((round(bbox[2] - bbox[0]), round(bbox[3] - bbox[1])))


class _Models:
    def __init__(self, case, backend):
        self.case = case
        self.metadata = {"backend": backend, "revision": "offline-slots"}

    def detect(self, _image, _threshold):
        return [{"label": "table", "score": 0.99, "bbox": _TABLE_BBOX}]

    def detect_layout(self, image, _options):
        return self.detect(image, 0.5)

    def recognize(self, _image, _threshold):
        def obj(label, bbox):
            return {"label": label, "score": 0.99, "bbox": bbox}

        objects = [obj("table", [0, 0, 300, 90])]
        objects.extend(
            obj("table row", [0, 30 * row, 300, 30 * (row + 1)]) for row in range(3)
        )
        objects.extend(
            obj("table column", [100 * col, 0, 100 * (col + 1), 90]) for col in range(3)
        )
        objects.extend(
            obj(
                "table spanning cell",
                [100 * col, 30 * row, 100 * (col + cs), 30 * (row + rs)],
            )
            for row, col, rs, cs in _CASES[self.case][0]
        )
        return objects

    def recognize_table(self, _image, _options):
        tokens = ["<tbody>"]
        boxes = []
        origins = _origins(self.case)
        for row in range(3):
            tokens.append("<tr>")
            for r, col, rs, cs in origins:
                if r != row:
                    continue
                if rs == cs == 1:
                    tokens.append("<td></td>")
                else:
                    tokens.append("<td")
                    if rs > 1:
                        tokens.append(f' rowspan="{rs}"')
                    if cs > 1:
                        tokens.append(f' colspan="{cs}"')
                    tokens.extend([">", "</td>"])
                boxes.append([100 * col, 30 * row, 100 * (col + cs), 30 * (row + rs)])
            tokens.append("</tr>")
        tokens.append("</tbody>")
        return tokens, boxes, [0.99] * len(tokens)


def _table(case, backend, monkeypatch):
    page = _page(case)
    if backend == "native":
        finder = page.find_tables(strategy="lines")
    else:
        words = page.get_text("words", sort=True)
        rendered = _tatr._RenderedPage(
            image=_Image(),
            tokens=[
                {
                    "bbox": list(word[:4]),
                    "text": word[4],
                    "block_num": word[5],
                    "line_num": word[6],
                    "span_num": word[7],
                }
                for word in words
            ],
            page_bbox=(0.0, 0.0, 612.0, 792.0),
            scale_x=1.0,
            scale_y=1.0,
            # Successful native extraction can still return zero words. The
            # adapters must conservatively mark that as unavailable.
            text_source="pdfspine-native",
        )
        module = _tatr if backend == "tatr" else _onnx
        monkeypatch.setattr(module, "_render_page", lambda *_: rendered)
        monkeypatch.setattr(module, "_get_runtime", lambda *_: _Models(case, backend))
        options = {"crop_padding": 0, "ocr_if_no_text": False}
        if backend == "tatr":
            options.update(native_line_guidance=False, adaptive_crop=False)
        finder = page.find_tables(
            strategy="vision", backend=backend, vision_options=options
        )
    assert len(finder) == 1
    assert finder[0].source == backend
    return finder[0]


@pytest.mark.parametrize("backend", ["native", "tatr", "onnx"])
@pytest.mark.parametrize("case", _CASES)
def test_typed_slots_match_across_backends(monkeypatch, backend, case):
    table = _table(case, backend, monkeypatch)
    states = _CASES[case][1]
    expected_origins = _origins(case)
    legacy_grid = table.extract()
    legacy_cells = table.cells
    assert legacy_grid == [
        [f"R{r}C{c}" if state == "p" else None for c, state in enumerate(row)]
        for r, row in enumerate(states)
    ]
    assert (table.row_count, table.col_count) == (3, 3)
    assert tuple(table.bbox) == pytest.approx(_TABLE_BBOX)

    slots = table.slots
    origins = table.origin_cells
    assert isinstance(slots, tuple) and all(isinstance(row, tuple) for row in slots)
    assert isinstance(origins, tuple)
    assert [[slot.state for slot in row] for row in slots] == [
        [_STATE[value] for value in row] for row in states
    ]
    assert [(c.row, c.col, c.row_span, c.col_span) for c in origins] == expected_origins
    assert [span[:4] for span in table.spans] == expected_origins

    for row in range(3):
        for col in range(3):
            slot = slots[row][col]
            assert isinstance(slot, pdfspine.TableSlot)
            assert (slot.row, slot.col) == (row, col)
            covering = [
                cell
                for cell in origins
                if cell.row <= row < cell.row + cell.row_span
                and cell.col <= col < cell.col + cell.col_span
            ]
            assert len(covering) == 1
            assert slot.cell is covering[0]
            cell = slot.cell
            assert isinstance(cell, pdfspine.TableCell)
            assert isinstance(cell.bbox, pdfspine.Rect)
            assert slot.origin == (cell.row, cell.col)
            assert tuple(cell.bbox) == pytest.approx(
                _bbox(cell.row, cell.col, cell.row_span, cell.col_span)
            )
            if slot.state == "continuation":
                assert slot.origin != (row, col)
                assert legacy_cells[row][col] is None
            else:
                assert slot.origin == (row, col)
                assert slot.state == cell.state
                assert tuple(legacy_cells[row][col]) == tuple(cell.bbox)
                expected_text = (
                    f"R{row}C{col}"
                    if slot.state == "present"
                    else ""
                    if slot.state == "blank"
                    else None
                )
                assert cell.text == expected_text

    # Typed inspection is cached and does not alter either compatibility API.
    assert table.slots is slots
    assert table.origin_cells is origins
    assert table.extract() == legacy_grid
    assert table.cells == legacy_cells


def test_typed_slots_are_frozen_and_exported(monkeypatch):
    table = _table("mixed", "tatr", monkeypatch)
    cell = table.origin_cells[0]
    slot = table.slots[1][0]
    with pytest.raises(FrozenInstanceError):
        cell.text = "changed"
    with pytest.raises(FrozenInstanceError):
        slot.cell = None
    assert "TableCell" in pdfspine.__all__
    assert "TableSlot" in pdfspine.__all__
    assert get_type_hints(pdfspine.TableCell)["bbox"] is pdfspine.Rect
    assert get_type_hints(pdfspine.TableSlot)["cell"] == pdfspine.TableCell | None


def test_missing_structure_and_unavailable_physical_cells_are_distinct():
    class Record:
        row_count = 2
        col_count = 3
        cell_records = [
            (0, 0, 2, 1, (0.0, 0.0, 10.0, 20.0), None),
            (0, 1, 1, 1, (10.0, 0.0, 20.0, 10.0), ""),
            (1, 1, 1, 1, (10.0, 10.0, 20.0, 20.0), "text"),
        ]

        @property
        def cells(self):
            raise AssertionError(
                "typed slots must not infer structure from legacy cells"
            )

        def extract(self):
            raise AssertionError("typed slots must not infer state from legacy extract")

    table = pdfspine.Table(Record())
    physical = table.slots[0][0]
    missing = table.slots[0][2]
    assert physical.state == missing.state == "unavailable"
    assert physical.origin == (0, 0)
    assert physical.cell is table.slots[1][0].cell
    assert table.slots[1][0].state == "continuation"
    assert physical.cell.text is None
    assert missing.cell is missing.origin is None
    assert table.slots[1][2].cell is None
    assert len(table.origin_cells) == 3
    assert table.slots[0][1].state == "blank"
    assert table.slots[0][1].cell.text == ""


@pytest.mark.parametrize(
    "records",
    [
        [
            (0, 0, 2, 1, (0, 0, 10, 20), "merged"),
            (1, 0, 1, 1, (0, 10, 10, 20), "overlap"),
        ],
        [(0, 1, 1, 2, (10, 0, 30, 10), "out-of-bounds")],
        [(0, 0, 0, 1, (0, 0, 10, 10), "zero span")],
        [(-1, 0, 1, 1, (0, 0, 10, 10), "negative row")],
    ],
    ids=["overlapping-origins", "out-of-bounds", "zero-span", "negative-row"],
)
def test_ambiguous_or_invalid_origin_records_are_rejected(records):
    table = pdfspine.Table(
        SimpleNamespace(row_count=2, col_count=2, cell_records=records)
    )
    with pytest.raises(ValueError):
        _ = table.slots


@pytest.mark.parametrize(
    "record_type", [_tatr._TatrTableRecord, _onnx._OnnxTableRecord]
)
def test_vision_record_retains_explicit_text_availability(record_type):
    records = [
        {"row_nums": [0], "column_nums": [0], "bbox": (0, 0, 10, 10)},
        {
            "row_nums": [0],
            "column_nums": [1],
            "bbox": (10, 0, 20, 10),
            "cell_text": "",
        },
    ]
    record = record_type(
        records,
        [(0, 0, 20, 10)],
        [(0, 0, 10, 10), (10, 0, 20, 10)],
        0.9,
        "pdfspine-native",
        {},
    )
    table = pdfspine.Table(record)
    assert table.extract() == [[None, None]]
    assert [slot.state for slot in table.slots[0]] == ["unavailable", "blank"]
    assert [cell.text for cell in table.origin_cells] == [None, ""]
