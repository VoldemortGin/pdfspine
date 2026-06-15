#!/usr/bin/env python3
"""Ad-hoc diagnostic: side-by-side oxide vs fitz text for one PDF page.

Run with the PROJECT venv python (has oxide_pdf). Calls the oracle venv as a
subprocess for fitz output. Prints, for a chosen page:
  - first N lines of oxide get_text("text")
  - first N lines of fitz  get_text("text")
  - block bboxes + first text snippet from oxide get_text("blocks") and fitz
Usage:
  env -u CONDA_PREFIX .venv/bin/python conformance/diag.py <pdf> [page] [mode]
mode: lines (default) | blocks | cropbox
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
ORACLE_PY = REPO / ".venv-oracle" / "bin" / "python"


def fitz_blocks(pdf: str, page: int) -> list:
    code = (
        "import fitz,sys,json;"
        "d=fitz.open(sys.argv[1]);p=d[int(sys.argv[2])];"
        "print(json.dumps({'rect':list(p.rect),'cropbox':list(p.cropbox),"
        "'mediabox':list(p.mediabox),'rot':p.rotation,"
        "'blocks':[[round(b[0],1),round(b[1],1),round(b[2],1),round(b[3],1),b[4]] for b in p.get_text('blocks')],"
        "'text':p.get_text('text')}))"
    )
    out = subprocess.run([str(ORACLE_PY), "-c", code, pdf, str(page)],
                         capture_output=True, text=True)
    if out.returncode != 0:
        print("FITZ ERR", out.stderr[:500]); sys.exit(1)
    return json.loads(out.stdout)


def main() -> int:
    pdf = sys.argv[1]
    page = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    mode = sys.argv[3] if len(sys.argv) > 3 else "lines"
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 40

    import oxide_pdf
    d = oxide_pdf.open(pdf)
    p = d.load_page(page)
    ox_text = p.get_text("text")
    ox_blocks = p.get_text("blocks")

    f = fitz_blocks(pdf, page)

    print(f"=== page {page}  oxide_rect=? fitz rect={f['rect']} cropbox={f['cropbox']} mediabox={f['mediabox']} rot={f['rot']}")
    if mode == "cropbox":
        # show oxide blocks whose bbox is outside fitz cropbox
        cb = f["cropbox"]
        print("--- oxide blocks possibly outside cropbox ---")
        for b in ox_blocks:
            x0, y0, x1, y1 = b[0], b[1], b[2], b[3]
            outside = x1 < cb[0] or x0 > cb[2] or y1 < cb[1] or y0 > cb[3]
            tag = "OUT" if outside else "in "
            print(f"{tag} [{x0:.0f},{y0:.0f},{x1:.0f},{y1:.0f}] {b[4][:60]!r}")
        return 0

    if mode == "blocks":
        print("--- OXIDE blocks ---")
        for b in ox_blocks[:n]:
            print(f"[{b[0]:.0f},{b[1]:.0f},{b[2]:.0f},{b[3]:.0f}] #{b[5]} {b[4][:70]!r}")
        print("--- FITZ blocks ---")
        for b in f["blocks"][:n]:
            print(f"[{b[0]:.0f},{b[1]:.0f},{b[2]:.0f},{b[3]:.0f}] {b[4][:70]!r}")
        return 0

    # lines
    ox_lines = ox_text.splitlines()
    fz_lines = f["text"].splitlines()
    print(f"--- OXIDE first {n} lines (total {len(ox_lines)}) ---")
    for ln in ox_lines[:n]:
        print(repr(ln))
    print(f"--- FITZ first {n} lines (total {len(fz_lines)}) ---")
    for ln in fz_lines[:n]:
        print(repr(ln))
    return 0


if __name__ == "__main__":
    sys.exit(main())
