#!/usr/bin/env python3
"""Generate the tiny, committed, license-clean CI regression fixtures.

These are hand-authored, deterministic, born-digital PDFs (pure ASCII PDF
syntax, Helvetica Type1 base-14 fonts, NO compression) — a clean-room corpus
with **no MuPDF / PyMuPDF derivation**. They are CC0-1.0 / public-domain
(self-generated) and are committed under ``fixtures/born/`` so the extraction +
render regression gates (``ci_gate.py``) can run in CI without fetching the
large gitignored real-document corpus.

Outputs (all under ``fixtures/born/``):

* ``pangrams.pdf``       — one page, four pangram lines (reading-order gate).
* ``reading-order.pdf``  — two pages of single-column prose (cross-page order).
* ``render-fixture.pdf`` — one dense page (filled rectangles + a text band) so
  the SSIM render gate has substantial, varied ink and is sensitive to renderer
  regressions instead of being dominated by white space.

Run from the repo root::

    python conformance/gt/make_ci_fixtures.py        # (re)write the fixtures

The exact ``gt_text`` for each fixture is the literal text drawn below; it is
inlined into ``conformance/gt/ci_manifest.json`` (see ``ci_gate.py``) so the
reading-order gate needs no oracle. Re-running this script is deterministic: it
must reproduce byte-identical PDFs (and therefore the committed sha256s).
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BORN_DIR = REPO_ROOT / "fixtures" / "born"


def _escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def _assemble(objs: dict[int, bytes]) -> bytes:
    """Serialize a {id: body} object map into a minimal, valid PDF with xref."""
    maxid = max(objs)
    out = bytearray(b"%PDF-1.4\n")
    offsets: dict[int, int] = {}
    for i in range(1, maxid + 1):
        offsets[i] = len(out)
        out += str(i).encode() + b" 0 obj\n" + objs[i] + b"\nendobj\n"
    xref_pos = len(out)
    size = maxid + 1
    out += b"xref\n0 " + str(size).encode() + b"\n"
    out += b"0000000000 65535 f \n"
    for i in range(1, maxid + 1):
        out += ("%010d 00000 n \n" % offsets[i]).encode()
    out += b"trailer\n<< /Size " + str(size).encode() + b" /Root 1 0 R >>\n"
    out += b"startxref\n" + str(xref_pos).encode() + b"\n%%EOF\n"
    return bytes(out)


def build_text_pdf(pages_lines: list[list[str]]) -> bytes:
    """A simple single-column text PDF: 12pt Helvetica, lines top-to-bottom."""
    n_pages = len(pages_lines)
    font_id = 3
    objs: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        font_id: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    }
    page_ids: list[int] = []
    nid = 4
    for i, lines in enumerate(pages_lines):
        pid, cid = nid, nid + 1
        nid += 2
        page_ids.append(pid)
        ops = ["BT", "/F1 12 Tf", "14 TL", "72 720 Td"]
        for ln in lines:
            ops.append(f"({_escape(ln)}) Tj")
            ops.append("T*")
        ops.append("ET")
        content = "\n".join(ops).encode("latin-1")
        objs[pid] = (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 " + str(font_id).encode() + b" 0 R >> >> "
            b"/Contents " + str(cid).encode() + b" 0 R >>"
        )
        objs[cid] = (
            b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n"
            + content + b"\nendstream"
        )
    kids = b" ".join(f"{pid} 0 R".encode() for pid in page_ids)
    objs[2] = (
        b"<< /Type /Pages /Kids [" + kids + b"] /Count " + str(n_pages).encode() + b" >>"
    )
    return _assemble(objs)


def build_render_pdf(boxes: list[tuple[int, int, int, int, float]], lines: list[str]) -> bytes:
    """A dense page: filled gray rectangles + an 18pt black text band.

    The filled boxes exercise the rasterizer's fill path and give the page
    substantial, spatially-varied ink so the SSIM render gate is sensitive to
    regressions instead of being dominated by white space.
    """
    ops: list[str] = []
    for x, y, w, h, g in boxes:
        ops.append(f"{g:.2f} {g:.2f} {g:.2f} rg")
        ops.append(f"{x} {y} {w} {h} re f")
    ops.append("0 0 0 rg")
    ops += ["BT", "/F1 18 Tf", "20 TL", "72 360 Td"]
    for ln in lines:
        ops.append(f"({_escape(ln)}) Tj")
        ops.append("T*")
    ops.append("ET")
    content = "\n".join(ops).encode("latin-1")
    objs: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
        3: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        4: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
        ),
        5: (
            b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n"
            + content + b"\nendstream"
        ),
    }
    return _assemble(objs)


# --------------------------------------------------------------------------- #
# Fixture content (this literal text is the ground truth for the order gate).
# --------------------------------------------------------------------------- #
PANGRAMS = [
    "The quick brown fox jumps over the lazy dog.",
    "Pack my box with five dozen liquor jugs.",
    "Sphinx of black quartz judge my vow.",
    "How razorback jumping frogs can level six piqued gymnasts.",
]
READING_ORDER_P1 = [
    "Reading order matters when a document has multiple lines.",
    "Each line should appear in the same sequence it was written.",
    "A faithful extractor preserves top to bottom ordering.",
    "This page exercises simple single column flow.",
]
READING_ORDER_P2 = [
    "The second page continues the same logical narrative.",
    "Page breaks must not scramble the overall reading order.",
    "Extraction joins pages in their natural document sequence.",
    "End of the born digital reading order fixture.",
]
RENDER_BOXES = [
    (72, 600, 200, 120, 0.20),
    (320, 600, 220, 120, 0.60),
    (72, 420, 468, 120, 0.85),
]
RENDER_LINES = [
    "RENDER REGRESSION FIXTURE",
    "pdfspine raster vs committed reference",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789",
    "the rasterizer fill and glyph paths",
]

FIXTURES: dict[str, bytes] = {}


def _materialize() -> None:
    FIXTURES["pangrams.pdf"] = build_text_pdf([PANGRAMS])
    FIXTURES["reading-order.pdf"] = build_text_pdf([READING_ORDER_P1, READING_ORDER_P2])
    FIXTURES["render-fixture.pdf"] = build_render_pdf(RENDER_BOXES, RENDER_LINES)


def main() -> int:
    _materialize()
    BORN_DIR.mkdir(parents=True, exist_ok=True)
    for name, data in FIXTURES.items():
        (BORN_DIR / name).write_bytes(data)
        print(f"wrote fixtures/born/{name} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
