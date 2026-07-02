"""TS-3 (PRD-NEXT §10) — system-CJK TTC glyph-subset embedding via ``insert_text``.

``insert_text(fontfile=…)`` embeds a usage-based TrueType subset by default,
which is what makes a multi-megabyte system TTC (macOS ``Songti.ttc`` ≈ 64 MB)
embeddable at all. Gates here: output **< 5 % of the source TTC**, pdfspine
text read-back, and the REAL PyMuPDF oracle (.venv-oracle) read-back.

Platform-guarded: skipped when ``Songti.ttc`` (macOS) is absent; the oracle
test is additionally skipped when ``.venv-oracle`` is missing.
"""

import os
import subprocess

import pytest

import pdfspine

SONGTI = "/System/Library/Fonts/Supplemental/Songti.ttc"
ORACLE = os.path.join(
    os.path.dirname(__file__), "..", "..", ".venv-oracle", "bin", "python"
)
TEXT = "永和九年，岁在癸丑"

pytestmark = pytest.mark.skipif(
    not os.path.exists(SONGTI), reason="macOS Songti.ttc not present"
)


def _blank_doc() -> "pdfspine.Document":
    """A minimal one-page 612x792 document (no fonts, no content)."""
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>"),
    ]
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
    startxref = len(out)
    out += b"xref\n0 4\n0000000000 65535 f \n"
    for num in range(1, 4):
        out += f"{offsets[num]:010} 00000 n \n".encode()
    out += b"trailer\n<< /Size 4 /Root 1 0 R >>\n"
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return pdfspine.open(stream=bytes(out))


def _subset_pdf_bytes() -> tuple[bytes, int]:
    """(PDF bytes with TEXT in a Songti subset, source TTC size)."""
    with open(SONGTI, "rb") as fh:
        ttc = fh.read()
    doc = _blank_doc()
    doc[0].insert_text((72, 100), TEXT, fontsize=24, fontfile=ttc)
    return doc.tobytes(), len(ttc)


def test_ts3_songti_subset_size_and_readback():
    pdf, source_size = _subset_pdf_bytes()
    # TS-3 gate: the whole PDF (subset FontFile2 included) < 5% of the TTC.
    assert len(pdf) < source_size * 0.05, (
        f"subset PDF {len(pdf)} B must be < 5% of the {source_size} B TTC"
    )
    got = pdfspine.open(stream=pdf)[0].get_text()
    for ch in TEXT:
        assert ch in got, f"{ch!r} missing from read-back: {got!r}"


def test_ts3_songti_subset_fitz_oracle_readback(tmp_path):
    if not os.path.exists(ORACLE):
        pytest.skip("real-PyMuPDF oracle not available")
    pdf, _ = _subset_pdf_bytes()
    path = tmp_path / "subset.pdf"
    path.write_bytes(pdf)
    # get_pixmap forces MuPDF to parse the embedded subset program (a stronger
    # oracle than text extraction alone, which only reads the ToUnicode CMap).
    code = (
        "import fitz, sys;"
        "page = fitz.open(sys.argv[1])[0];"
        "pm = page.get_pixmap(dpi=96);"
        "print('INK' if len(set(pm.samples)) > 1 else 'BLANK');"
        "print(page.get_text())"
    )
    out = subprocess.run(
        [ORACLE, "-c", code, str(path)],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    assert out.startswith("INK"), f"fitz rendered a blank page: {out!r}"
    for ch in TEXT:
        assert ch in out, f"{ch!r} missing from fitz oracle read-back: {out!r}"
