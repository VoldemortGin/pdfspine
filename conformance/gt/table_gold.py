#!/usr/bin/env python3
"""Gold-table loading for the financial-report table evaluation set.

Everything here is pure stdlib and produces the repo-wide GriTS cell dict::

    {"row_nums": [int, ...], "column_nums": [int, ...], "cell_text": str,
     "bbox": [x0, y0, x1, y1] | None, "header": bool}

``row_nums``/``column_nums`` list every grid index the cell occupies (a
spanning cell lists several); there are no rowspan/colspan fields. Coordinates
are PDF points, origin top-left, y down — the same system as pdfspine
``Table.bbox`` and as FinTabNet.c ``pdf_bbox``, so no flip is needed.

Three gold sources are supported:

1. **FinTabNet.c manifests** (``fetch_fintabnet.py`` output) via
   :func:`load_manifest`. The manifests written so far store ``annotation`` /
   ``pdf`` as *absolute* paths of the machine that fetched them, which broke
   ``tables_diff.load_gold_manifest`` as soon as the checkout moved. This
   loader tries the recorded path first and then falls back to
   ``<manifest dir>/annotations/<basename>`` (``pdfs/<basename>`` for the PDF).
   A page is skipped only when the annotation is found nowhere; a missing PDF
   just yields ``pdf=None`` so the caller can decide.
2. **Hand-written ``<name>.gold.json``** via :func:`load_gold_page_file` /
   :func:`load_gold_table_file`::

       {"document_id": "acme-2023", "page_index": 0,
        "tables": [{"table_id": "t0", "bbox": [x0, y0, x1, y1],
                    "tags": ["plain"],
                    "cells": [{"row_nums": [0], "column_nums": [0],
                               "cell_text": "Revenue",
                               "bbox": [x0, y0, x1, y1], "header": true}]}]}

   To make manual annotation less tedious a cell may instead be written as
   ``{"row": 0, "col": 0, "rowspan": 1, "colspan": 1, "text": "Revenue",
   "bbox": [...], "header": true}``; it is expanded to ``row_nums`` /
   ``column_nums`` / ``cell_text`` on load. Both notations may be mixed.
   ``table_id``, ``bbox``, ``tags``, ``bbox`` of a cell and ``header`` are
   all optional.
3. **Hand-written ``<name>.gold.html``** — one or more ``<table>`` elements
   with ordinary ``colspan``/``rowspan``/``<th>`` markup, converted through
   :func:`cells_from_html`. Cells carry no bbox unless the ``<td>`` has a
   ``data-bbox="x0 y0 x1 y1"`` attribute (which :func:`cells_to_html` emits
   when exporting a draft, so a round trip keeps geometry).

Tags
----
:func:`page_tags` assigns a fixed vocabulary of heuristic labels so the CLI can
break scores down by table kind: ``plain``, ``borderless``, ``multi-header``,
``spanning``, ``wide``, ``tall``, ``multi-table`` (see the function docstring
for the exact rules). ``borderless`` cannot be derived from the gold data —
annotations carry no ruling-line information — so it is *injected by the
caller* through the ``lines_detected`` argument once the ``strategy="lines"``
detector has run on the page.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import html as _html
from html.parser import HTMLParser
import json
import logging
from pathlib import Path

_log = logging.getLogger(__name__)

TAG_VOCABULARY: tuple[str, ...] = (
    "plain",
    "borderless",
    "multi-header",
    "spanning",
    "wide",
    "tall",
    "multi-table",
)
WIDE_MIN_COLS = 8
TALL_MIN_ROWS = 20


# --------------------------------------------------------------------------- #
# Data model
# --------------------------------------------------------------------------- #
@dataclass
class GoldTable:
    """One gold table: GriTS cells plus table-level metadata."""

    table_id: str
    bbox: list[float] | None
    cells: list[dict]
    n_rows: int
    n_cols: int
    tags: list[str] = field(default_factory=list)


@dataclass
class GoldPage:
    """One gold page (a PDF page with its structure-eligible tables)."""

    document_id: str
    pdf: Path | None
    page_index: int
    tables: list[GoldTable]
    anno_license: str | None = None
    pdf_license: str | None = None


# --------------------------------------------------------------------------- #
# Cell helpers
# --------------------------------------------------------------------------- #
def _norm_text(s: object) -> str:
    return " ".join(str(s or "").split())


def _bbox_or_none(raw: object) -> list[float] | None:
    if not isinstance(raw, (list, tuple)) or len(raw) < 4:
        return None
    try:
        return [float(v) for v in raw[:4]]
    except (TypeError, ValueError):
        return None


def grid_shape(cells: list[dict]) -> tuple[int, int]:
    """``(n_rows, n_cols)`` of a cell list (``(0, 0)`` when empty)."""
    n_rows = max((max(c["row_nums"]) for c in cells if c["row_nums"]), default=-1)
    n_cols = max((max(c["column_nums"]) for c in cells if c["column_nums"]), default=-1)
    return n_rows + 1, n_cols + 1


def normalize_cell(raw: dict) -> dict | None:
    """Accept either cell notation and return the canonical GriTS cell.

    * ``row_nums``/``column_nums`` (GriTS / FinTabNet.c) are used verbatim;
    * otherwise ``row``/``col`` (+ optional ``rowspan``/``colspan``, default 1)
      are expanded.

    Text comes from ``cell_text``, then ``text``, then ``json_text_content``,
    then ``pdf_text_content``; bbox from ``bbox`` then ``pdf_bbox``; header
    from ``header`` then ``is_column_header``. Returns ``None`` for a cell
    that occupies no grid position.
    """
    if raw.get("row_nums") is not None or raw.get("column_nums") is not None:
        rows = [int(v) for v in raw.get("row_nums") or []]
        cols = [int(v) for v in raw.get("column_nums") or []]
    elif raw.get("row") is not None and raw.get("col") is not None:
        r0, c0 = int(raw["row"]), int(raw["col"])
        rs = max(1, int(raw.get("rowspan") or 1))
        cs = max(1, int(raw.get("colspan") or 1))
        rows = list(range(r0, r0 + rs))
        cols = list(range(c0, c0 + cs))
    else:
        return None
    if not rows or not cols:
        return None
    text = None
    for key in ("cell_text", "text", "json_text_content", "pdf_text_content"):
        if raw.get(key) is not None:
            text = raw[key]
            break
    bbox = _bbox_or_none(raw.get("bbox"))
    if bbox is None:
        bbox = _bbox_or_none(raw.get("pdf_bbox"))
    header = bool(raw.get("header", raw.get("is_column_header", False)))
    return {
        "row_nums": rows,
        "column_nums": cols,
        "cell_text": _norm_text(text),
        "bbox": bbox,
        "header": header,
    }


def _cells_from_list(raw_cells: list[dict]) -> list[dict]:
    out: list[dict] = []
    for raw in raw_cells or []:
        cell = normalize_cell(raw)
        if cell is not None:
            out.append(cell)
    return out


# --------------------------------------------------------------------------- #
# HTML <-> cells
# --------------------------------------------------------------------------- #
class _GoldTableHTMLParser(HTMLParser):
    """Collect every ``<table>`` in a document as a list of GriTS cells.

    Uses an occupancy grid like a browser: each ``<td>``/``<th>`` claims the
    next free column of the current row (skipping columns already taken by a
    ``rowspan`` from an earlier row) and marks every row/column it spans.
    ``<th>`` sets ``header=True``. ``<br>`` becomes a space. A ``data-bbox``
    attribute (``"x0 y0 x1 y1"`` or comma-separated) becomes ``bbox``.
    """

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.tables: list[list[dict]] = []
        self._cells: list[dict] | None = None
        self._row = -1
        self._occupied: set[tuple[int, int]] = set()
        self._col_cursor = 0
        self._cur: dict | None = None
        self._text: list[str] = []

    @staticmethod
    def _span(value: str | None) -> int:
        try:
            return max(1, int(value or 1))
        except ValueError:
            return 1

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        a = dict(attrs)
        if tag == "table":
            self._cells = []
            self.tables.append(self._cells)
            self._row = -1
            self._occupied = set()
        elif tag == "tr":
            if self._cells is None:
                self._cells = []
                self.tables.append(self._cells)
            self._row += 1
            self._col_cursor = 0
        elif tag in ("td", "th"):
            if self._cells is None:
                self._cells = []
                self.tables.append(self._cells)
            if self._row < 0:
                self._row = 0
            colspan = self._span(a.get("colspan"))
            rowspan = self._span(a.get("rowspan"))
            col = self._col_cursor
            while (self._row, col) in self._occupied:
                col += 1
            row_nums = list(range(self._row, self._row + rowspan))
            column_nums = list(range(col, col + colspan))
            for r in row_nums:
                for cc in column_nums:
                    self._occupied.add((r, cc))
            self._col_cursor = col + colspan
            bbox = None
            raw_bbox = a.get("data-bbox")
            if raw_bbox:
                bbox = _bbox_or_none(raw_bbox.replace(",", " ").split())
            self._cur = {
                "row_nums": row_nums,
                "column_nums": column_nums,
                "cell_text": "",
                "bbox": bbox,
                "header": tag == "th",
            }
            self._text = []
        elif tag == "br":
            self._text.append(" ")

    def handle_data(self, data: str) -> None:
        if self._cur is not None:
            self._text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag in ("td", "th") and self._cur is not None and self._cells is not None:
            self._cur["cell_text"] = _norm_text("".join(self._text))
            self._cells.append(self._cur)
            self._cur = None
            self._text = []
        elif tag == "table":
            self._cells = None


def tables_from_html(html: str) -> list[list[dict]]:
    """Every ``<table>`` in ``html`` -> list of GriTS cell lists (document order)."""
    p = _GoldTableHTMLParser()
    p.feed(html)
    p.close()
    return p.tables


def cells_from_html(html: str) -> list[dict]:
    """First ``<table>`` in ``html`` -> GriTS cells (``[]`` when there is none).

    ``colspan``/``rowspan`` are resolved with an occupancy grid, so a cell
    after a rowspan placeholder gets the correct (shifted) column index.
    ``<th>`` cells carry ``header=True``; ``bbox`` is ``None`` unless the
    element has a ``data-bbox`` attribute.
    """
    tables = tables_from_html(html)
    return tables[0] if tables else []


def cells_to_html(cells: list[dict]) -> str:
    """GriTS cells -> canonical ``<table>`` HTML with colspan/rowspan.

    Each cell is written once, in the ``<tr>`` of its first row, ordered by
    first column; text is escaped; ``header`` cells become ``<th>``; a bbox
    is preserved as ``data-bbox="x0 y0 x1 y1"`` so
    ``cells_from_html(cells_to_html(cells))`` round-trips structure, text,
    header flags and geometry.
    """
    n_rows, _ = grid_shape(cells)
    by_row: dict[int, list[tuple[int, dict]]] = {}
    for c in cells:
        if not c.get("row_nums") or not c.get("column_nums"):
            continue
        by_row.setdefault(min(c["row_nums"]), []).append((min(c["column_nums"]), c))
    lines = ["<table>"]
    for r in range(n_rows):
        parts = ["<tr>"]
        for _, c in sorted(by_row.get(r, []), key=lambda t: t[0]):
            tag = "th" if c.get("header") else "td"
            attrs = ""
            cs = max(c["column_nums"]) - min(c["column_nums"]) + 1
            rs = max(c["row_nums"]) - min(c["row_nums"]) + 1
            if cs > 1:
                attrs += f' colspan="{cs}"'
            if rs > 1:
                attrs += f' rowspan="{rs}"'
            bbox = _bbox_or_none(c.get("bbox"))
            if bbox is not None:
                attrs += ' data-bbox="' + " ".join(f"{v:g}" for v in bbox) + '"'
            text = _html.escape(_norm_text(c.get("cell_text")), quote=False)
            parts.append(f"<{tag}{attrs}>{text}</{tag}>")
        parts.append("</tr>")
        lines.append("".join(parts))
    lines.append("</table>")
    return "\n".join(lines)


# --------------------------------------------------------------------------- #
# Tags
# --------------------------------------------------------------------------- #
def table_tags(table: GoldTable) -> list[str]:
    """Structural tags of one table: ``spanning`` / ``multi-header`` / ``wide`` / ``tall``.

    * ``spanning`` — any cell with rowspan > 1 or colspan > 1.
    * ``multi-header`` — header cells occupy more than one distinct row, or a
      header cell has colspan > 1 (a grouped column header).
    * ``wide`` — ``n_cols >= 8``; ``tall`` — ``n_rows >= 20``.
    """
    tags: list[str] = []
    header_rows: set[int] = set()
    spanning = False
    grouped_header = False
    for c in table.cells:
        rs, cs = len(c["row_nums"]), len(c["column_nums"])
        if rs > 1 or cs > 1:
            spanning = True
        if c.get("header"):
            header_rows.update(c["row_nums"])
            if cs > 1:
                grouped_header = True
    if len(header_rows) > 1 or grouped_header:
        tags.append("multi-header")
    if spanning:
        tags.append("spanning")
    if table.n_cols >= WIDE_MIN_COLS:
        tags.append("wide")
    if table.n_rows >= TALL_MIN_ROWS:
        tags.append("tall")
    return tags


def page_tags(page: GoldPage, *, lines_detected: bool | None = None) -> list[str]:
    """Heuristic tags for a gold page, drawn from :data:`TAG_VOCABULARY`.

    * The union of every table's :func:`table_tags` (plus any tags declared
      in the gold file) — ``multi-header``, ``spanning``, ``wide``, ``tall``.
    * ``multi-table`` — the page holds two or more gold tables.
    * ``plain`` — no table on the page is multi-header / spanning / wide /
      tall (``multi-table`` and ``borderless`` do not disqualify ``plain``).
    * ``borderless`` — **not derivable from the gold data** (annotations have
      no ruling-line information). Pass ``lines_detected=False`` once the
      ``strategy="lines"`` detector found no ruled table on the page to add
      it; ``True`` means ruled; the default ``None`` leaves the tag out.

    The result is ordered as in :data:`TAG_VOCABULARY`.
    """
    found: set[str] = set()
    for t in page.tables:
        found.update(t.tags)
        found.update(table_tags(t))
    if len(page.tables) >= 2:
        found.add("multi-table")
    if lines_detected is False:
        found.add("borderless")
    if not found & {"multi-header", "spanning", "wide", "tall"}:
        found.add("plain")
    return [t for t in TAG_VOCABULARY if t in found]


# --------------------------------------------------------------------------- #
# FinTabNet.c manifest
# --------------------------------------------------------------------------- #
def _resolve_entry_path(raw: object, manifest_dir: Path, subdir: str) -> Path | None:
    """Recorded path if it exists, else ``<manifest_dir>/<subdir>/<basename>``."""
    if not raw:
        return None
    p = Path(str(raw))
    if not p.is_absolute():
        p = manifest_dir / p
    if p.exists():
        return p
    fallback = manifest_dir / subdir / p.name
    if fallback.exists():
        return fallback
    return None


def gold_table_from_annotation(anno: dict, table_id: str) -> GoldTable:
    """One FinTabNet.c annotation table -> :class:`GoldTable`.

    Text prefers ``json_text_content`` over ``pdf_text_content``; cell bbox is
    ``pdf_bbox``; table bbox is ``pdf_table_bbox``; ``is_column_header`` becomes
    ``header``. ``n_rows``/``n_cols`` come from the annotation's ``rows`` /
    ``columns`` maps when present, else from the cell grid.
    """
    cells = _cells_from_list(anno.get("cells") or [])
    n_rows, n_cols = grid_shape(cells)
    rows_meta = anno.get("rows")
    cols_meta = anno.get("columns")
    if isinstance(rows_meta, dict) and rows_meta:
        n_rows = max(n_rows, len(rows_meta))
    if isinstance(cols_meta, dict) and cols_meta:
        n_cols = max(n_cols, len(cols_meta))
    table = GoldTable(
        table_id=str(anno.get("structure_id") or table_id),
        bbox=_bbox_or_none(anno.get("pdf_table_bbox")),
        cells=cells,
        n_rows=n_rows,
        n_cols=n_cols,
    )
    table.tags = table_tags(table)
    return table


def load_manifest(path: Path, *, skipped: list[str] | None = None) -> list[GoldPage]:
    """Load a FinTabNet.c ``manifest.json`` into :class:`GoldPage` objects.

    Paths recorded in the manifest are tried first; when they do not exist
    (the manifests store absolute paths from the fetching machine) the loader
    falls back to ``<manifest dir>/annotations/<name>`` and
    ``<manifest dir>/pdfs/<name>``. A page whose annotation is found nowhere
    is skipped (logged, and its document id appended to ``skipped`` when a
    list is given); a missing PDF yields ``pdf=None``.

    Tables flagged ``exclude_for_structure`` are dropped.
    """
    path = Path(path)
    data = json.loads(path.read_text(encoding="utf-8"))
    entries = data.get("entries") if isinstance(data, dict) else data
    mdir = path.resolve().parent
    pages: list[GoldPage] = []
    for entry in entries or []:
        doc_id = str(
            entry.get("document_id") or Path(str(entry.get("annotation") or "")).stem
        )
        anno_path = _resolve_entry_path(entry.get("annotation"), mdir, "annotations")
        if anno_path is None:
            _log.warning(
                "gold annotation not found for %s (%s)", doc_id, entry.get("annotation")
            )
            if skipped is not None:
                skipped.append(doc_id)
            continue
        try:
            annos = json.loads(anno_path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            _log.warning("unreadable annotation %s: %s", anno_path, exc)
            if skipped is not None:
                skipped.append(doc_id)
            continue
        if isinstance(annos, dict):
            annos = [annos]
        tables = [
            gold_table_from_annotation(a, f"{doc_id}_t{i}")
            for i, a in enumerate(annos or [])
            if isinstance(a, dict) and not a.get("exclude_for_structure")
        ]
        pages.append(
            GoldPage(
                document_id=doc_id,
                pdf=_resolve_entry_path(entry.get("pdf"), mdir, "pdfs"),
                page_index=int(entry.get("pdf_page_index", 0) or 0),
                tables=tables,
                anno_license=entry.get("anno_license"),
                pdf_license=entry.get("pdf_license"),
            )
        )
    return pages


# --------------------------------------------------------------------------- #
# Hand-written gold files
# --------------------------------------------------------------------------- #
def _gold_stem(path: Path) -> str:
    name = path.name
    for suffix in (".gold.json", ".gold.html"):
        if name.endswith(suffix):
            return name[: -len(suffix)]
    return path.stem


def _gold_table_from_json(raw: dict, table_id: str) -> GoldTable:
    cells = _cells_from_list(raw.get("cells") or [])
    n_rows, n_cols = grid_shape(cells)
    declared = [str(t) for t in raw.get("tags") or [] if str(t) in TAG_VOCABULARY]
    table = GoldTable(
        table_id=str(raw.get("table_id") or table_id),
        bbox=_bbox_or_none(raw.get("bbox")),
        cells=cells,
        n_rows=n_rows,
        n_cols=n_cols,
    )
    table.tags = sorted(
        set(declared) | set(table_tags(table)), key=TAG_VOCABULARY.index
    )
    return table


def load_gold_page_file(path: Path) -> GoldPage:
    """Load ``<name>.gold.json`` or ``<name>.gold.html`` as a :class:`GoldPage`.

    JSON: the page object described in the module docstring (a bare list of
    tables is also accepted; ``document_id`` then defaults to ``<name>`` and
    ``page_index`` to 0). HTML: every ``<table>`` becomes one table with
    ``table_id`` ``t<i>`` and no table bbox. Raises ``ValueError`` for any
    other extension.
    """
    path = Path(path)
    name = path.name.lower()
    doc_id = _gold_stem(path)
    if name.endswith(".gold.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, list):
            data = {"tables": data}
        if not isinstance(data, dict):
            raise ValueError(f"{path}: expected a JSON object or list of tables")
        return GoldPage(
            document_id=str(data.get("document_id") or doc_id),
            pdf=None,
            page_index=int(data.get("page_index", 0) or 0),
            tables=[
                _gold_table_from_json(t, f"t{i}")
                for i, t in enumerate(data.get("tables") or [])
                if isinstance(t, dict)
            ],
        )
    if name.endswith(".gold.html"):
        tables: list[GoldTable] = []
        for i, cells in enumerate(tables_from_html(path.read_text(encoding="utf-8"))):
            n_rows, n_cols = grid_shape(cells)
            table = GoldTable(
                table_id=f"t{i}", bbox=None, cells=cells, n_rows=n_rows, n_cols=n_cols
            )
            table.tags = table_tags(table)
            tables.append(table)
        return GoldPage(document_id=doc_id, pdf=None, page_index=0, tables=tables)
    raise ValueError(f"{path}: expected a *.gold.json or *.gold.html file")


def load_gold_table_file(path: Path) -> list[GoldTable]:
    """Tables of a ``*.gold.json`` / ``*.gold.html`` file (see :func:`load_gold_page_file`)."""
    return load_gold_page_file(path).tables
