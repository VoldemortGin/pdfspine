"""MARKDOWN-TO-PDF-* — ``pdfspine.markdown_to_pdf`` (original extension,
PRD-NEXT §9).

Renders Markdown (text or file path) to a new, opened
:class:`~pdfspine.Document`. Not part of the fitz-compat surface. All fixtures
are self-generated in-test (raw PNG bytes, repo-local Liberation TTFs) — no
external files, no network (PRD §10).
"""

from __future__ import annotations

import pathlib
import struct
import zlib

import pdfspine
import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
SANS_TTF = REPO_ROOT / "crates" / "pdf-fonts" / "fonts" / "liberation" / "LiberationSans-Regular.ttf"


def _png(w: int, h: int, rgb: tuple[int, int, int] = (255, 0, 0)) -> bytes:
    """A minimal 8-bit RGB PNG of size ``w`` x ``h`` filled with ``rgb``."""

    def chunk(typ: bytes, data: bytes) -> bytes:
        body = typ + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    return sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def _full_text(doc: pdfspine.Document) -> str:
    return "".join(page.get_text() for page in doc)


# --- MARKDOWN-TO-PDF-001: text input → opened Document, text round-trips ---


def test_markdown_to_pdf_001_text_input_returns_document():
    doc = pdfspine.markdown_to_pdf("# Title\n\nHello markdown body.")
    assert isinstance(doc, pdfspine.Document)
    assert doc.is_pdf
    assert doc.page_count == 1
    text = _full_text(doc)
    assert "Title" in text
    assert "Hello markdown body." in text


# --- MARKDOWN-TO-PDF-002: file-path input (str and Path) reads the file ----


def test_markdown_to_pdf_002_file_path_input(tmp_path):
    p = tmp_path / "doc.md"
    p.write_text("# From File\n\nRead from disk.", encoding="utf-8")
    for arg in (p, str(p)):
        doc = pdfspine.markdown_to_pdf(arg)
        text = _full_text(doc)
        assert "From File" in text
        assert "Read from disk." in text
    # A non-Markdown-looking string that is not an existing file stays text.
    doc = pdfspine.markdown_to_pdf("no/such/file.md")
    assert "no/such/file.md" in _full_text(doc)


# --- MARKDOWN-TO-PDF-003: headings + lists + tables content ----------------


def test_markdown_to_pdf_003_headings_lists_tables():
    md = (
        "# H1 Head\n\n## H2 Head\n\n"
        "- first item\n- second item\n  1. nested one\n\n"
        "| Col A | Col B |\n|---|---|\n| cell1 | cell2 |\n"
    )
    doc = pdfspine.markdown_to_pdf(md)
    text = _full_text(doc)
    for needle in (
        "H1 Head",
        "H2 Head",
        "first item",
        "second item",
        "nested one",
        "Col A",
        "Col B",
        "cell1",
        "cell2",
    ):
        assert needle in text, f"missing {needle!r} in {text!r}"


# --- MARKDOWN-TO-PDF-004: font= accepts a TTF path and raw bytes -----------


def test_markdown_to_pdf_004_font_path_and_bytes():
    md = "# Custom Face\n\nUser font body."
    for font in (SANS_TTF, str(SANS_TTF), SANS_TTF.read_bytes(), bytearray(SANS_TTF.read_bytes())):
        doc = pdfspine.markdown_to_pdf(md, font=font)
        text = _full_text(doc)
        assert "Custom Face" in text
        assert "User font body." in text


# --- MARKDOWN-TO-PDF-005: cjk_font= per-character fallback round-trips -----


def test_markdown_to_pdf_005_cjk_font_fallback_roundtrip():
    # Liberation covers Cyrillic — which the Base-14 (WinAnsi) body face cannot
    # encode — so the per-character fallback path is observable without a CJK
    # font fixture: the fallback text must round-trip.
    md = "Hello Привет world"
    for cjk in (SANS_TTF, SANS_TTF.read_bytes()):
        doc = pdfspine.markdown_to_pdf(md, cjk_font=cjk)
        text = _full_text(doc)
        assert "Hello" in text
        assert "Привет" in text
        assert "world" in text


# --- MARKDOWN-TO-PDF-006: Chinese without cjk_font degrades, never crashes -


def test_markdown_to_pdf_006_cjk_without_font_degrades():
    doc = pdfspine.markdown_to_pdf("# 标题\n\nlatin 中文正文 tail")
    assert doc.page_count == 1
    text = _full_text(doc)
    # CJK Option A: unencodable characters degrade to missing glyphs (never a
    # crash); the Latin runs around them must survive.
    assert "latin" in text
    assert "tail" in text


# --- MARKDOWN-TO-PDF-007: Chinese with a (non-CJK) cjk_font stays typed ----


def test_markdown_to_pdf_007_cjk_with_fallback_font_no_crash():
    # Liberation carries no CJK glyphs; the contract here is structural: no
    # panic, a well-formed one-page PDF, and extraction still returns a str.
    doc = pdfspine.markdown_to_pdf("# 标题\n\nbody 中文 body", cjk_font=SANS_TTF)
    assert doc.page_count == 1
    text = _full_text(doc)
    assert isinstance(text, str)
    assert "body" in text


# --- MARKDOWN-TO-PDF-008: margins scalar / 4-tuple + page geometry ---------


def test_markdown_to_pdf_008_margins_and_page_geometry():
    doc = pdfspine.markdown_to_pdf("Body.", page_width=400, page_height=500, margins=36)
    rect = doc[0].rect
    assert abs(rect.width - 400) < 0.01
    assert abs(rect.height - 500) < 0.01
    doc = pdfspine.markdown_to_pdf("Body.", margins=(18, 36, 18, 36))
    # Default page size is A4.
    rect = doc[0].rect
    assert abs(rect.width - 595.32) < 0.01
    assert abs(rect.height - 841.92) < 0.01
    assert "Body." in _full_text(doc)


# --- MARKDOWN-TO-PDF-009: bad inputs raise typed exceptions, never panic ---


def test_markdown_to_pdf_009_bad_inputs_typed_errors():
    with pytest.raises(TypeError):
        pdfspine.markdown_to_pdf(123)  # type: ignore[arg-type]
    with pytest.raises(ValueError):
        pdfspine.markdown_to_pdf("x", margins=(1, 2, 3))  # not a 4-tuple
    with pytest.raises(ValueError):
        pdfspine.markdown_to_pdf("x", margins="wide")  # type: ignore[arg-type]
    with pytest.raises(OSError):
        pdfspine.markdown_to_pdf("x", font="/no/such/font-file.ttf")
    with pytest.raises(TypeError):
        pdfspine.markdown_to_pdf("x", cjk_font=3.14)  # type: ignore[arg-type]
    with pytest.raises(pdfspine.PdfError):
        pdfspine.markdown_to_pdf("x", font=b"this is not a font program")
    with pytest.raises(pdfspine.PdfError):
        # Margins that leave no usable content area.
        pdfspine.markdown_to_pdf("x", margins=400)


# --- MARKDOWN-TO-PDF-010: images — file default base_dir + explicit base_dir


def test_markdown_to_pdf_010_relative_image_base_dir(tmp_path):
    (tmp_path / "pic.png").write_bytes(_png(6, 4))
    md_file = tmp_path / "doc.md"
    md_file.write_text("before\n\n![p](pic.png)\n\nafter", encoding="utf-8")
    # File input: base_dir defaults to the file's parent directory.
    doc = pdfspine.markdown_to_pdf(md_file)
    assert len(doc[0].get_images()) == 1
    assert "before" in _full_text(doc)
    # Text input: relative paths need an explicit base_dir...
    doc = pdfspine.markdown_to_pdf("![p](pic.png)", base_dir=tmp_path)
    assert len(doc[0].get_images()) == 1
    # ...and without one they are a typed error, not a crash.
    with pytest.raises(pdfspine.PdfError):
        pdfspine.markdown_to_pdf("![p](pic.png)")
