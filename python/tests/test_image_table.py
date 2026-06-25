"""IMG-TABLE-* — end-to-end proof of pdfspine's ``Page.find_image_tables``:
reconstructing a table that lives INSIDE a raster image (a scanned / image-only
page with no text layer and no vector rulings) back into a structured grid,
preserving per-cell text, bbox, background color, text color and OCR
confidence.

The whole flow is offline and deterministic, and uses pdfspine ONLY:

1. We DRAW a real table on a normal vector page with pdfspine's own renderer:
   each cell is a ``draw_rect`` (a flat fill + a thin black border) and each
   label is real Helvetica text via ``insert_text`` (a distinct dark-red label
   in one cell to prove ``text_color``).
2. We RASTERIZE that page to an RGB ``Pixmap`` (``get_pixmap``) and re-insert the
   raw pixels as the entire content of a fresh page via ``insert_image`` — so the
   result is image-only: there is NO text layer (``get_text`` is empty) and no
   vector rulings survive the rasterize.
3. ``page.find_image_tables(engine="paddle")`` renders the page back to a
   pixmap, runs the pure-Rust PP-OCRv5 engine, clusters the words into a grid
   and samples each cell's colors + OCR confidence.

Rendering real anti-aliased glyphs (rather than a hand-rolled bitmap font) is
what the OCR detection/recognition models were trained on, so every cell is read
reliably while the fixture stays 100% deterministic and offline.

PaddleOCR is an OPT-IN build: the lean base wheel compiles it out and
``find_image_tables`` raises ``PdfUnsupportedError``. This whole module is
SKIPPED on a lean build (probed once, like ``test_ocr_paddle.py``), keeping CI
green everywhere.
"""

from __future__ import annotations

import pdfspine
import pytest


# --- the fixture grid: contents, fills (0..1 RGB), text colors --------------
#
# Tokens are SHORT uppercase ASCII / digits, distinct per cell so the OCR text
# is unambiguous and the grid assignment is checkable cell-by-cell.

_HEADER = ["NAME", "RANK", "TEAM"]
_ROW1 = ["FRED", "42", "RED"]
_ROW2 = ["MARK", "38", "BLUE"]
_CONTENTS = [_HEADER, _ROW1, _ROW2]

_R = len(_CONTENTS)  # 3 rows
_C = len(_CONTENTS[0])  # 3 cols

# Distinct, saturated, flat fills as 0..1 RGB (pdfspine's draw_rect fill space).
# Flat fills survive the render + rasterize as exact 8-bit triples, so each
# cell's sampled ``bg_color`` is the 8-bit rounding of these (asserted below).
_WHITE = (1.0, 1.0, 1.0)
_YELLOW = (1.0, 0.94, 0.59)
_BLUE = (0.59, 0.78, 1.0)
_GREEN = (0.71, 1.0, 0.71)
_FILLS = [
    [_WHITE, _YELLOW, _BLUE],
    [_GREEN, _WHITE, _YELLOW],
    [_BLUE, _GREEN, _WHITE],
]

# Text colors (0..1 RGB): black everywhere, except cell (1,2)="RED" drawn in
# dark red so its sampled ``text_color`` is provably reddish.
_BLACK = (0.0, 0.0, 0.0)
_DARKRED = (0.7, 0.0, 0.0)
_TEXT_COLORS = [
    [_BLACK, _BLACK, _BLACK],
    [_BLACK, _BLACK, _DARKRED],
    [_BLACK, _BLACK, _BLACK],
]

# Geometry (page points). Big cells + big text so PP-OCRv5 reads crisply and the
# gap-clustering is unambiguous.
_CELL_W = 180
_CELL_H = 90
_PAD = 24  # outer margin around the whole table
_BORDER = 2.0  # black grid-line thickness (points)
_FONTSIZE = 36
_RENDER_DPI = 150


def _rgb255(c: tuple[float, float, float]) -> tuple[int, int, int]:
    """The 8-bit RGB triple a 0..1 fill renders to (round-to-nearest)."""
    return tuple(round(ch * 255.0) for ch in c)  # type: ignore[return-value]


# --- fixture: a real, rasterized, image-only table --------------------------


def _scanned_table_doc() -> pdfspine.Document:
    """A 1-page image-only "scanned" doc whose only content is a rasterized
    table — no text layer, no vector rulings.

    Built with pdfspine only: draw the table with the vector renderer, rasterize
    to an RGB pixmap, then re-insert the raw pixels as a full-page image.
    """
    w = _PAD * 2 + _C * _CELL_W
    h = _PAD * 2 + _R * _CELL_H

    # 1. Draw the table on a normal vector page.
    src = pdfspine.open()
    page = src.new_page(width=float(w), height=float(h))
    for r in range(_R):
        for c in range(_C):
            x0 = _PAD + c * _CELL_W
            y0 = _PAD + r * _CELL_H
            page.draw_rect(
                (x0, y0, x0 + _CELL_W, y0 + _CELL_H),
                color=_BLACK,
                fill=_FILLS[r][c],
                width=_BORDER,
            )
    for r in range(_R):
        for c in range(_C):
            x0 = _PAD + c * _CELL_W
            y0 = _PAD + r * _CELL_H
            page.insert_text(
                (x0 + 26, y0 + _CELL_H / 2 + 12),
                _CONTENTS[r][c],
                fontsize=_FONTSIZE,
                color=_TEXT_COLORS[r][c],
            )

    # 2. Rasterize to a flat RGB pixmap.
    pix = page.get_pixmap(dpi=_RENDER_DPI)
    assert pix.n == 3 and not pix.alpha, "expected a flat RGB pixmap"
    rgb = bytes(pix.samples)
    pw, ph = pix.width, pix.height

    # 3. Re-insert the raw pixels as the whole content of a fresh image-only page.
    doc = pdfspine.open()
    ip = doc.new_page(width=float(pw), height=float(ph))
    ip.insert_image((0, 0, float(pw), float(ph)), stream=rgb, width=pw, height=ph)
    assert ip.get_text("text").strip() == "", "fixture must be image-only"
    return doc


def _contains(haystack: str | None, needle: str) -> bool:
    """Case- and whitespace-insensitive containment (mirrors test_ocr_paddle)."""
    if haystack is None:
        return False
    norm = "".join(haystack.split()).upper()
    return "".join(needle.split()).upper() in norm


# --- skip guard: PaddleOCR (the ocr build) must be present ------------------


def _paddle_available() -> bool:
    """Whether the wheel was built with the opt-in ``ocr`` feature, probed by
    calling ``find_image_tables`` on a tiny page: a lean build raises
    ``PdfUnsupportedError`` before any model work."""
    doc = pdfspine.open()
    page = doc.new_page(width=8.0, height=8.0)
    page.insert_image((0, 0, 8.0, 8.0), stream=b"\xff" * (8 * 8 * 3), width=8, height=8)
    try:
        page.find_image_tables(engine="paddle", dpi=72)
    except pdfspine.PdfUnsupportedError:
        return False
    return True


_HAS_PADDLE = _paddle_available()

_requires_paddle = pytest.mark.skipif(
    not _HAS_PADDLE,
    reason="PaddleOCR engine not compiled in (lean build); install pdfspine[ocr]",
)


# A single fixture render + OCR pass is shared by every assertion (it is the
# expensive step). Module-scoped so OCR runs once.
@pytest.fixture(scope="module")
def table() -> pdfspine.ImageTable:
    doc = _scanned_table_doc()
    tables = doc[0].find_image_tables(engine="paddle", dpi=_RENDER_DPI)
    assert len(tables) == 1, f"expected exactly one table, got {len(tables)}"
    return tables[0]


# --- IMG-TABLE-001: exactly one table, correct grid dims --------------------


@_requires_paddle
def test_grid_dimensions(table: pdfspine.ImageTable) -> None:
    assert table.row_count == _R, f"row_count={table.row_count}, want {_R}"
    assert table.col_count == _C, f"col_count={table.col_count}, want {_C}"


# --- IMG-TABLE-002: grid-line geometry sanity -------------------------------


@_requires_paddle
def test_grid_lines(table: pdfspine.ImageTable) -> None:
    assert len(table.cols) == _C + 1, table.cols
    assert len(table.rows) == _R + 1, table.rows
    assert all(a < b for a, b in zip(table.cols, table.cols[1:])), table.cols
    assert all(a < b for a, b in zip(table.rows, table.rows[1:])), table.rows

    # table.bbox encloses every cell bbox.
    bb = table.bbox
    for cell in table.cells:
        cb = cell.bbox
        assert cb.x0 >= bb.x0 - 1 and cb.y0 >= bb.y0 - 1
        assert cb.x1 <= bb.x1 + 1 and cb.y1 <= bb.y1 + 1


# --- IMG-TABLE-003: every grid cell is present and addressable --------------


@_requires_paddle
def test_cells_present(table: pdfspine.ImageTable) -> None:
    assert len(table.cells) == _R * _C, f"got {len(table.cells)} cells"
    for r in range(_R):
        for c in range(_C):
            cell = table.cell(r, c)
            assert cell is not None, f"missing cell ({r},{c})"
            assert cell.row == r and cell.col == c
            assert cell.row_span == 1 and cell.col_span == 1


# --- IMG-TABLE-004: OCR text per cell ---------------------------------------


@_requires_paddle
def test_cell_text(table: pdfspine.ImageTable) -> None:
    grid = table.extract()
    assert len(grid) == _R and all(len(row) == _C for row in grid)

    mismatches: list[str] = []
    for r in range(_R):
        for c in range(_C):
            want = _CONTENTS[r][c]
            got = grid[r][c]
            if not _contains(got, want):
                mismatches.append(f"({r},{c}) want {want!r} got {got!r}")

    # Aim for ALL cells correct; tolerate at most one stubborn OCR garble but
    # require the grid shape and the large majority of tokens to be recovered.
    assert len(mismatches) <= 1, "too many OCR text mismatches:\n" + "\n".join(
        mismatches
    )
    # The structurally-important header row must be fully correct.
    for c in range(_C):
        assert _contains(grid[0][c], _HEADER[c]), (
            f"header cell ({0},{c}) want {_HEADER[c]!r} got {grid[0][c]!r}"
        )


# --- IMG-TABLE-005: per-cell background color -------------------------------


@_requires_paddle
def test_cell_background_color(table: pdfspine.ImageTable) -> None:
    for r in range(_R):
        for c in range(_C):
            cell = table.cell(r, c)
            assert cell is not None
            got = tuple(cell.bg_color)
            want = _rgb255(_FILLS[r][c])
            # Flat fills survive the render + rasterize exactly; allow only a
            # tiny tolerance for rounding / sub-pixel sampling.
            assert all(abs(g - w) <= 6 for g, w in zip(got, want)), (
                f"bg ({r},{c}) got {got} want {want}"
            )


# --- IMG-TABLE-006: per-cell text color -------------------------------------


@_requires_paddle
def test_cell_text_color(table: pdfspine.ImageTable) -> None:
    # Dark-red text cell (1,2)="RED": clearly reddish (red >> green/blue).
    red_cell = table.cell(1, 2)
    assert red_cell is not None
    rr, rg, rb = red_cell.text_color
    assert rr > rg + 40 and rr > rb + 40, f"text_color not reddish: {(rr, rg, rb)}"

    # Black-text header cell (0,0)="NAME": near-black.
    black_cell = table.cell(0, 0)
    assert black_cell is not None
    br, bg, bb = black_cell.text_color
    assert br < 90 and bg < 90 and bb < 90, f"text_color not near-black: {(br, bg, bb)}"


# --- IMG-TABLE-007: per-cell OCR confidence ---------------------------------


@_requires_paddle
def test_cell_confidence(table: pdfspine.ImageTable) -> None:
    for r in range(_R):
        for c in range(_C):
            cell = table.cell(r, c)
            assert cell is not None
            if cell.text and cell.text.strip():
                assert 0.0 < cell.confidence <= 100.0, (
                    f"cell ({r},{c}) confidence={cell.confidence}"
                )

    # The clearly-readable header cells must be confident.
    for c in range(_C):
        cell = table.cell(0, c)
        assert cell is not None
        assert cell.confidence > 30.0, (
            f"header ({0},{c}) low confidence {cell.confidence}"
        )
