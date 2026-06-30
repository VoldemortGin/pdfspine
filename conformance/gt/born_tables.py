#!/usr/bin/env python3
"""Synthetic TABLE generator with PERFECT cell-grid ground truth.

Part of the objective ground-truth subsystem (alongside ``born_digital.py`` etc.).
FinTabNet (the planned objective table GT) is CDN-unreachable, so this manufactures
table PDFs whose true cell grid is known by construction — letting pdfspine and
fitz be scored against the SAME objective truth by ``score_tables_gt.py``.

Self-contained: emits PDFs with a tiny hand-rolled writer (Helvetica, one of the
14 standard fonts — no embedding, no third-party PDF lib, no fitz). Each PDF places
one cell of text per (row, col) grid position; ``bordered`` tables also draw ruling
lines. The ground truth (the 2-D grid + border style + the strategy a caller should
use) is written to ``corpus-tables/manifest.json``.

Run from ROOT:  ``.venv/bin/python conformance/gt/born_tables.py``
"""

from __future__ import annotations

import json
from pathlib import Path

OUT = Path(__file__).resolve().parent / "corpus-tables"


def _esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def _content(grid, *, bordered, x0, y0_top, col_w, row_h, fontsize) -> bytes:
    """Build a page content stream. Coordinates are PDF user space (y up)."""
    ncol = max(len(r) for r in grid)
    nrow = len(grid)
    page_h = 792.0
    xs = [x0]
    for c in range(ncol):
        xs.append(xs[-1] + col_w[c])
    # Row top edges in y-down sense, converted to PDF y-up baselines.
    ops = ["BT", f"/F1 {fontsize} Tf"]
    for r, row in enumerate(grid):
        for c, cell in enumerate(row):
            if cell is None or cell == "":
                continue
            tx = xs[c] + 4
            # baseline near the bottom of the cell band
            ty = page_h - (y0_top + r * row_h + row_h - 7)
            ops.append(f"1 0 0 1 {tx:.1f} {ty:.1f} Tm ({_esc(str(cell))}) Tj")
    ops.append("ET")
    if bordered:
        y_top = page_h - y0_top
        y_bot = page_h - (y0_top + nrow * row_h)
        ops.append("0.6 w")
        for x in xs:
            ops.append(f"{x:.1f} {y_top:.1f} m {x:.1f} {y_bot:.1f} l S")
        for r in range(nrow + 1):
            y = page_h - (y0_top + r * row_h)
            ops.append(f"{xs[0]:.1f} {y:.1f} m {xs[-1]:.1f} {y:.1f} l S")
    return ("\n".join(ops)).encode("latin-1")


def _write_pdf(path: Path, content: bytes) -> None:
    """Minimal single-page PDF: catalog, pages, page, Helvetica, content stream."""
    objs: list[bytes] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length %d >>\nstream\n%s\nendstream" % (len(content), content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref_pos = len(out)
    out += b"xref\n0 %d\n" % (len(objs) + 1)
    out += b"0000000000 65535 f \n"
    for off in offsets[1:]:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objs) + 1,
        xref_pos,
    )
    path.write_bytes(out)


def make(name, grid, *, bordered=True, col_w=90, x0=60, y0_top=80, row_h=24, fontsize=11):
    ncol = max(len(r) for r in grid)
    cw = [col_w] * ncol if isinstance(col_w, int) else col_w
    content = _content(
        grid, bordered=bordered, x0=x0, y0_top=y0_top, col_w=cw, row_h=row_h, fontsize=fontsize
    )
    _write_pdf(OUT / f"{name}.pdf", content)
    return {
        "name": name,
        "pdf": f"{name}.pdf",
        "rows": len(grid),
        "cols": ncol,
        "bordered": bordered,
        "strategy": "lines" if bordered else "text",
        "grid": grid,
    }


def main() -> int:
    OUT.mkdir(exist_ok=True)
    man = [
        make("t1_simple", [["Item", "Q1", "Q2", "Q3"], ["Sales", "100", "150", "200"],
                           ["Cost", "40", "55", "70"]]),
        make("t2_borderless", [["Name", "Age", "City"], ["Alice", "30", "NYC"],
                               ["Bob", "25", "LA"], ["Carol", "41", "SF"]], bordered=False),
        make("t3_5col", [["A", "B", "C", "D", "E"], ["1", "2", "3", "4", "5"],
                         ["6", "7", "8", "9", "10"]], col_w=70),
        make("t4_2col", [["Key", "Value"], ["alpha", "1"], ["beta", "2"], ["gamma", "3"]],
             col_w=120),
        make("t5_finance", [["Metric", "FY22", "FY23", "FY24"], ["Revenue", "2100", "2350", "2680"],
                            ["Profit", "1900", "2050", "2210"], ["Margin", "90%", "87%", "82%"]]),
        make("t6_longtext", [["Field", "Description"],
                             ["status", "the current processing state of the record"],
                             ["owner", "person responsible for this item"]], col_w=[90, 260]),
        make("t7_manyrows", [["ID", "Val"]] + [[str(i), str(i * i)] for i in range(1, 11)],
             col_w=80),
        make("t8_borderless_num", [["X", "Y", "Z"], ["10", "20", "30"], ["40", "50", "60"],
                                   ["70", "80", "90"]], bordered=False, col_w=80),
    ]
    (OUT / "manifest.json").write_text(json.dumps(man, ensure_ascii=False, indent=1))
    print(f"generated {len(man)} table PDFs + manifest in {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
