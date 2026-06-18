"""CLI surface tests for ``pdfspine.cli`` (the ``pdfspine`` command).

All fixtures are self-generated in-test (raw PDF bytes) — no external files
(PRD §10). Tests invoke ``cli.main([...])`` directly and capture stdout via
``capsys``. Catalog IDs ``CLI-*`` (test names ``test_cli_<n>_<desc>``).
"""

from __future__ import annotations

import json
import zlib

from pdfspine import cli
import pytest


# --- self-generated PDF assembler (classic xref) --------------------------
# Copied from test_text.py / test_m4.py so this file is fully self-contained.


def _build_pdf(objects: list[tuple[int, bytes]], root: int, extra_trailer: bytes = b"") -> bytes:
    """Assembles a classic-xref PDF from ``(num, body)`` object pairs."""
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    max_num = 0
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
        max_num = max(max_num, num)

    size = max_num + 1
    startxref = len(out)
    out += b"xref\n"
    out += f"0 {size}\n".encode()
    out += b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n"
    out += f"<< /Size {size} /Root {root} 0 R {extra_trailer.decode()} >>\n".encode()
    out += b"startxref\n"
    out += f"{startxref}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


def _escape(s: str) -> bytes:
    b = s.encode("latin-1")
    return b.replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)")


def _helvetica_font(first: int = 32, last: int = 125, width: int = 500) -> bytes:
    n = last - first + 1
    widths = b"[" + b" ".join(str(width).encode() for _ in range(n)) + b"]"
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding "
        + f"/FirstChar {first} /LastChar {last} ".encode()
        + b"/Widths " + widths + b" >>"
    )


def text_pdf(lines: list[str], font_widths: bool = True, ystart: int = 700, leading: int = 20) -> bytes:
    """A 1-page PDF (MediaBox [0 0 612 792]) drawing ``lines`` with /F1."""
    parts = [f"BT /F1 12 Tf 72 {ystart} Td".encode()]
    for i, line in enumerate(lines):
        if i:
            parts.append(f"0 -{leading} Td".encode())
        parts.append(b"(" + _escape(line) + b") Tj")
    parts.append(b"ET")
    content = b" ".join(parts)

    font = _helvetica_font() if font_widths else (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
            (5, font),
        ],
        root=1,
    )


def _raw_content_pdf(content: bytes, font: bytes) -> bytes:
    """A 1-page PDF whose content stream is supplied verbatim (raw bytes)."""
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
            (5, font),
        ],
        root=1,
    )


def multipage_text_pdf(page_texts: list[str]) -> bytes:
    """A PDF with one page per string in ``page_texts``, each drawing that text."""
    n = len(page_texts)
    kids = b" ".join(f"{3 + i} 0 R".encode() for i in range(n))
    objects: list[tuple[int, bytes]] = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count " + str(n).encode() + b" /Kids [" + kids + b"] >>"),
        (3 + n, _helvetica_font()),  # shared font, last object
    ]
    for i, txt in enumerate(page_texts):
        page_obj = 3 + i
        content_obj = 3 + n + 1 + i
        content = b"BT /F1 12 Tf 72 700 Td (" + _escape(txt) + b") Tj ET"
        objects.append(
            (
                page_obj,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                b"/Resources << /Font << /F1 " + str(3 + n).encode() + b" 0 R >> >> "
                b"/Contents " + str(content_obj).encode() + b" 0 R >>",
            )
        )
        objects.append(
            (content_obj, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream")
        )
    return _build_pdf(objects, root=1)


def image_pdf() -> bytes:
    """A 1-page PDF with a single 1x1 DeviceRGB image XObject painted as /Im0."""
    pix = zlib.compress(b"\x00\x00\x00")
    content = b"q 1 0 0 1 100 100 cm /Im0 Do Q"
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
        (
            5,
            b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 "
            b"/BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode "
            b"/Length " + str(len(pix)).encode() + b" >>\nstream\n" + pix + b"\nendstream",
        ),
    ]
    return _build_pdf(objs, root=1)


_PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def _write(tmp_path, name: str, data: bytes):
    p = tmp_path / name
    p.write_bytes(data)
    return p


# ==========================================================================
# CLI-INFO-*
# ==========================================================================


def test_cli_001_info_prints_page_count(tmp_path, capsys):
    pdf = _write(tmp_path, "doc.pdf", multipage_text_pdf(["A", "B", "C"]))
    rc = cli.main(["info", str(pdf)])
    out = capsys.readouterr().out
    assert rc == 0
    assert "3" in out
    # surfaces the salient facts
    assert "PDF" in out  # format
    assert "encrypt" in out.lower()


# ==========================================================================
# CLI-TEXT-*
# ==========================================================================


def test_cli_002_text_prints_known_text(tmp_path, capsys):
    pdf = _write(tmp_path, "t.pdf", text_pdf(["Hello CLI", "Second Line"]))
    rc = cli.main(["text", str(pdf)])
    out = capsys.readouterr().out
    assert rc == 0
    assert "Hello CLI" in out
    assert "Second Line" in out


def test_cli_003_text_writes_to_output_file(tmp_path, capsys):
    pdf = _write(tmp_path, "t.pdf", text_pdf(["Hello CLI"]))
    out_file = tmp_path / "out.txt"
    rc = cli.main(["text", str(pdf), "-o", str(out_file)])
    assert rc == 0
    assert out_file.exists()
    assert "Hello CLI" in out_file.read_text()


def test_cli_004_text_format_json_is_valid(tmp_path, capsys):
    pdf = _write(tmp_path, "t.pdf", text_pdf(["Hello CLI"]))
    rc = cli.main(["text", str(pdf), "--format", "json"])
    out = capsys.readouterr().out
    assert rc == 0
    parsed = json.loads(out)
    assert isinstance(parsed, dict)
    assert "blocks" in parsed


def test_cli_005_text_pages_range(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["PageOne", "PageTwo", "PageThree"]))
    rc = cli.main(["text", str(pdf), "--pages", "2"])
    out = capsys.readouterr().out
    assert rc == 0
    assert "PageTwo" in out
    assert "PageOne" not in out
    assert "PageThree" not in out


# ==========================================================================
# CLI-RENDER-*
# ==========================================================================


def test_cli_006_render_writes_png_per_page(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2"]))
    outdir = tmp_path / "imgs"
    rc = cli.main(["render", str(pdf), "-o", str(outdir)])
    assert rc == 0
    pngs = sorted(outdir.glob("*.png"))
    assert len(pngs) == 2
    for png in pngs:
        assert png.read_bytes()[:8] == _PNG_MAGIC


def test_cli_007_render_pages_range_subset(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2", "P3"]))
    outdir = tmp_path / "imgs"
    rc = cli.main(["render", str(pdf), "--pages", "1,3", "-o", str(outdir)])
    assert rc == 0
    pngs = sorted(outdir.glob("*.png"))
    assert len(pngs) == 2


# ==========================================================================
# CLI-MERGE-*
# ==========================================================================


def test_cli_008_merge_page_count_is_sum(tmp_path, capsys):
    a = _write(tmp_path, "a.pdf", multipage_text_pdf(["A1", "A2"]))
    b = _write(tmp_path, "b.pdf", multipage_text_pdf(["B1", "B2", "B3"]))
    out = tmp_path / "merged.pdf"
    rc = cli.main(["merge", str(a), str(b), "-o", str(out)])
    assert rc == 0
    import pdfspine

    assert pdfspine.open(str(out)).page_count == 5


# ==========================================================================
# CLI-SPLIT-*
# ==========================================================================


def test_cli_009_split_writes_one_pdf_per_page(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2", "P3"]))
    outdir = tmp_path / "split"
    rc = cli.main(["split", str(pdf), "-o", str(outdir)])
    assert rc == 0
    pdfs = sorted(outdir.glob("*.pdf"))
    assert len(pdfs) == 3
    import pdfspine

    for p in pdfs:
        assert pdfspine.open(str(p)).page_count == 1


def test_cli_010_split_ranges(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2", "P3", "P4"]))
    outdir = tmp_path / "split"
    rc = cli.main(["split", str(pdf), "--ranges", "1-2,3-4", "-o", str(outdir)])
    assert rc == 0
    pdfs = sorted(outdir.glob("*.pdf"))
    assert len(pdfs) == 2
    import pdfspine

    for p in pdfs:
        assert pdfspine.open(str(p)).page_count == 2


# ==========================================================================
# CLI-PAGES-*
# ==========================================================================


def test_cli_011_pages_select_subset(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2", "P3", "P4"]))
    out = tmp_path / "subset.pdf"
    rc = cli.main(["pages", str(pdf), "--select", "1,3", "-o", str(out)])
    assert rc == 0
    import pdfspine

    d = pdfspine.open(str(out))
    assert d.page_count == 2
    assert "P1" in d[0].get_text()
    assert "P3" in d[1].get_text()


def test_cli_012_pages_select_reorder(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2", "P3"]))
    out = tmp_path / "reordered.pdf"
    rc = cli.main(["pages", str(pdf), "--select", "3,1", "-o", str(out)])
    assert rc == 0
    import pdfspine

    d = pdfspine.open(str(out))
    assert d.page_count == 2
    assert "P3" in d[0].get_text()
    assert "P1" in d[1].get_text()


# ==========================================================================
# CLI-IMAGES-*
# ==========================================================================


def test_cli_013_images_extract(tmp_path, capsys):
    pdf = _write(tmp_path, "i.pdf", image_pdf())
    outdir = tmp_path / "extracted"
    rc = cli.main(["images", str(pdf), "-o", str(outdir)])
    assert rc == 0
    files = list(outdir.iterdir())
    assert len(files) >= 1


# ==========================================================================
# CLI-TOC-*
# ==========================================================================


def test_cli_014_toc_runs_on_no_outline(tmp_path, capsys):
    # A PDF with no outline → clean run (empty TOC), exit 0.
    pdf = _write(tmp_path, "t.pdf", text_pdf(["Hello"]))
    rc = cli.main(["toc", str(pdf)])
    assert rc == 0


def test_cli_015_toc_prints_entries(tmp_path, capsys):
    # Build a doc, attach a TOC, save, then dump it via the CLI.
    import pdfspine

    src = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2"]))
    doc = pdfspine.open(str(src))
    doc.set_toc([[1, "Chapter One", 1], [1, "Chapter Two", 2]])
    with_toc = tmp_path / "with_toc.pdf"
    doc.save(str(with_toc))

    rc = cli.main(["toc", str(with_toc)])
    out = capsys.readouterr().out
    assert rc == 0
    assert "Chapter One" in out
    assert "Chapter Two" in out


# ==========================================================================
# CLI errors / version
# ==========================================================================


def test_cli_016_missing_file_nonzero_no_traceback(tmp_path, capsys):
    missing = tmp_path / "does_not_exist.pdf"
    rc = cli.main(["info", str(missing)])
    captured = capsys.readouterr()
    assert rc != 0
    msg = captured.out + captured.err
    assert "Traceback" not in msg
    assert msg.strip()  # a real error message was printed


def test_cli_017_bad_pdf_nonzero_no_traceback(tmp_path, capsys):
    bad = _write(tmp_path, "bad.pdf", b"this is not a pdf at all")
    rc = cli.main(["info", str(bad)])
    captured = capsys.readouterr()
    assert rc != 0
    msg = captured.out + captured.err
    assert "Traceback" not in msg
    assert msg.strip()


def test_cli_018_bad_page_range_nonzero_no_traceback(tmp_path, capsys):
    pdf = _write(tmp_path, "m.pdf", multipage_text_pdf(["P1", "P2"]))
    rc = cli.main(["text", str(pdf), "--pages", "9-12"])
    captured = capsys.readouterr()
    assert rc != 0
    msg = captured.out + captured.err
    assert "Traceback" not in msg
    assert msg.strip()


def test_cli_019_version(capsys):
    import pdfspine

    rc = cli.main(["--version"])
    out = capsys.readouterr().out
    assert rc == 0
    assert pdfspine.__version__ in out


def test_cli_020_no_args_shows_help_nonzero(capsys):
    rc = cli.main([])
    captured = capsys.readouterr()
    msg = captured.out + captured.err
    assert rc != 0
    assert "usage" in msg.lower()
