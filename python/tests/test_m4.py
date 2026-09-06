"""M4e Python gates: the full M4 edit surface through ``pdfspine`` + the ``fitz``
deprecated-alias shim (PRD §8.8 / §9.4 / §9.5 / §12 M4).

Covers content insert / draw / Shape, the annotation family + ``/AP``
portability, the redaction Python gate (gone-after-reopen), forms + ``Widget``,
embedded files, ``scrub``, and ``fitz`` camelCase parity. All fixtures are
self-generated in-test (PRD §10). Catalog IDs ``PYM4-*``.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys

import pytest

import pdfspine


# --- fixtures (self-built raw PDF bytes; no external files) ----------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    objects = sorted(objects, key=lambda o: o[0])
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
    size = max(offsets) + 1
    startxref = len(out)
    out += b"xref\n" + f"0 {size}\n".encode() + b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n" + f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


def _widths_font() -> bytes:
    """Helvetica with explicit ``/Widths`` so the interpreter can measure glyph
    advances — required for the redaction glyph-overlap test (PyMuPDF too)."""
    widths = b" ".join(b"600" for _ in range(32, 127))
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths ["
        + widths
        + b"] >>"
    )


def blank_doc(
    media: tuple[int, int, int, int] = (0, 0, 612, 792),
) -> "pdfspine.Document":
    """A one-page doc with a shared ``/Widths`` Helvetica under ``/F1`` and no
    content."""
    mb = " ".join(str(v) for v in media).encode()
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [" + mb + b"] "
            b"/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, b"<< /Length 0 >>\nstream\n\nendstream"),
        (5, _widths_font()),
    ]
    return pdfspine.open(stream=_build_pdf(objects, root=1))


def secret_doc(
    lead: str, secret: str
) -> tuple[bytes, tuple[float, float, float, float]]:
    """A page showing ``lead`` then ``secret`` on one line; returns the bytes and
    the top-left rect covering only ``secret`` (mirrors the Rust harness)."""
    char_w = 12.0 * 0.6
    x_lead = 72.0
    x_secret = x_lead + len(lead) * char_w
    x_end = x_secret + len(secret) * char_w
    body = (
        f"BT /F1 12 Tf 1 0 0 1 {x_lead:g} 700 Tm ({lead}) Tj "
        f"1 0 0 1 {x_secret:g} 700 Tm ({secret}) Tj ET"
    ).encode()
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (
            4,
            b"<< /Length "
            + str(len(body)).encode()
            + b" >>\nstream\n"
            + body
            + b"\nendstream",
        ),
        (5, _widths_font()),
    ]
    # Top-left rect: user y 698..710 → top-left y (792-710)..(792-698) = 82..94.
    rect = (x_secret - 1.0, 82.0, x_end + 1.0, 96.0)
    return _build_pdf(objects, root=1), rect


def acroform_doc() -> bytes:
    """A single text-field AcroForm (merged field+widget), value ``init``."""
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R /AcroForm 10 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Contents 4 0 R /Annots [11 0 R] "
            b"/Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, b"<< /Length 0 >>\nstream\n\nendstream"),
        (5, _widths_font()),
        (
            11,
            b"<< /Type /Annot /Subtype /Widget /P 3 0 R "
            b"/FT /Tx /T (tx1) /TU (Text One) /Rect [72 700 272 720] "
            b"/V (init) /DA (0 0 1 rg /F1 12 Tf) /Q 0 >>",
        ),
        (
            10,
            b"<< /Fields [11 0 R] /NeedAppearances false "
            b"/DA (0 0 1 rg /F1 12 Tf) /DR << /Font << /F1 5 0 R >> >> >>",
        ),
    ]
    return _build_pdf(objects, root=1)


# --- PYM4-INSERT-* : content insert / draw / Shape -------------------------


def test_pym4_insert_001_insert_text(tmp_path):
    doc = blank_doc()
    doc[0].insert_text((72, 100), "INSERTED", fontname="helv", fontsize=12)
    re = pdfspine.open(stream=doc.tobytes())
    assert "INSERTED" in re[0].get_text()


def test_pym4_insert_002_insert_textbox():
    doc = blank_doc()
    rv = doc[0].insert_textbox(pdfspine.Rect(72, 72, 400, 200), "BOXED TEXT")
    assert isinstance(rv, float)
    re = pdfspine.open(stream=doc.tobytes())
    assert "BOXED" in re[0].get_text()


def test_pym4_draw_001_draw_rect_line_get_drawings():
    doc = blank_doc()
    page = doc[0]
    page.draw_rect((50, 50, 150, 150), color=(0, 0, 1), width=2)
    page.draw_line((10, 10), (200, 200), color=(1, 0, 0))
    drawings = page.get_drawings()
    assert len(drawings) >= 2
    # reopen stays valid
    re = pdfspine.open(stream=doc.tobytes())
    assert re.page_count == 1


def test_pym4_shape_001_new_shape_commit():
    doc = blank_doc()
    page = doc[0]
    shape = page.new_shape()
    shape.draw_rect((40, 40, 120, 120))
    shape.finish(color=(0, 0, 0), fill=(0.5, 0.5, 0.5), width=1)
    shape.commit()
    re = pdfspine.open(stream=doc.tobytes())
    assert re.page_count == 1
    assert len(re[0].get_drawings()) >= 1


# --- PYM4-ANNOT-* : annotations + /AP portability --------------------------

_HL_QUAD = (60, 100, 200, 100, 60, 120, 200, 120)


def test_pym4_annot_001_add_and_list():
    doc = blank_doc()
    page = doc[0]
    hl = page.add_highlight_annot([_HL_QUAD])
    ft = page.add_freetext_annot(pdfspine.Rect(60, 200, 260, 240), "Free text")
    assert hl.type[1] == "Highlight"
    assert ft.type[1] == "FreeText"
    listed = {a.type[1] for a in page.annots()}
    assert {"Highlight", "FreeText"} <= listed
    # rect is a fitz Rect value type
    assert isinstance(ft.rect, pdfspine.Rect)


def test_pym4_annot_002_set_colors_update_persists():
    doc = blank_doc()
    page = doc[0]
    a = page.add_rect_annot((60, 60, 200, 120), color=(0, 0, 0))
    a.set_colors(stroke=(1, 0, 0))
    a.update()
    assert a.colors["stroke"] == (1.0, 0.0, 0.0)
    re = pdfspine.open(stream=doc.tobytes())
    ra = list(re[0].annots())
    assert len(ra) == 1
    assert ra[0].type[1] == "Square"
    assert ra[0].has_appearance  # /AP /N regenerated and persisted


def test_pym4_annot_003_delete():
    doc = blank_doc()
    page = doc[0]
    a = page.add_rect_annot((60, 60, 200, 120))
    assert len(page.annot_xrefs()) == 1
    page.delete_annot(a)
    assert page.annot_xrefs() == []
    re = pdfspine.open(stream=doc.tobytes())
    assert list(re[0].annots()) == []


def test_pym4_annot_004_ap_portability():
    """Every added subtype reopens with an /AP /N appearance stream."""
    doc = blank_doc()
    page = doc[0]
    page.add_highlight_annot([_HL_QUAD])
    page.add_freetext_annot(pdfspine.Rect(60, 200, 260, 240), "FT")
    page.add_rect_annot((60, 300, 200, 360), color=(0, 0, 0))
    page.add_circle_annot((60, 400, 200, 460), color=(0, 0, 0))
    page.add_line_annot((10, 10), (100, 100), color=(0, 0, 0))
    re = pdfspine.open(stream=doc.tobytes())
    annots = list(re[0].annots())
    assert len(annots) == 5
    for a in annots:
        assert a.has_appearance, f"{a.type[1]} missing /AP /N after reopen"


# --- PYM4-REDACT-* : redaction Python gate ---------------------------------


def test_pym4_redact_001_secret_gone_after_reopen(tmp_path):
    data, rect = secret_doc("PUBLIC ", "TOPSECRET")
    doc = pdfspine.open(stream=data)
    page = doc[0]
    assert "TOPSECRET" in page.get_text()
    page.add_redact_annot(rect)
    applied = page.apply_redactions()
    assert applied == 1
    out = tmp_path / "redacted.pdf"
    doc.save(str(out))
    re = pdfspine.open(str(out))
    text = re[0].get_text()
    assert "TOPSECRET" not in text  # gone after reopen (the M4 exit gate)
    assert "PUBLIC" in text  # neighbouring text intact


def test_pym4_redact_002_no_annots_noop():
    doc = blank_doc()
    assert doc[0].apply_redactions() == 0


# Top-left rect covering only ``SECRET`` on line two of ``_quote_ops_doc``:
# baseline 686 (user y 684..696 → top-left 96..108), x 108..151.2 at 7.2 pt per
# glyph, kept clear of the neighbouring spaces.
_QUOTE_OPS_RECT = (108.5, 96.0, 150.7, 108.0)


def _quote_ops_doc(*, explicit: bool = False) -> bytes:
    """Four lines at leading 14 typeset with ``Tj``, ``'`` and ``"``: the ``"``
    on line three sets ``Tw``/``Tc`` that the ``'`` on line four inherits.
    ``explicit=True`` spells the same page with ``T*`` / ``Tw`` / ``Tc`` / ``Tj``
    (identical glyph placement, no ``'`` / ``"``)."""
    if explicit:
        body = (
            b"BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm (LINE ONE) Tj "
            b"T* (KEEP SECRET TAIL) Tj 3 Tw 1 Tc T* (LINE THREE) Tj "
            b"T* (LINE FOUR) Tj ET"
        )
    else:
        body = (
            b"BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm (LINE ONE) Tj "
            b"(KEEP SECRET TAIL) ' 3 1 (LINE THREE) \" (LINE FOUR) ' ET"
        )
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (
            4,
            b"<< /Length "
            + str(len(body)).encode()
            + b" >>\nstream\n"
            + body
            + b"\nendstream",
        ),
        (5, _widths_font()),
    ]
    return _build_pdf(objects, root=1)


# Runs under the REAL PyMuPDF: redacts ``src`` over ``rect`` into ``theirs``,
# then reads and renders ``ours`` / ``theirs`` (words + SSIM) with one reader.
_ORACLE_SCRIPT = r"""
import json, sys
import pymupdf

src, rect, ours, theirs = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3], sys.argv[4]
doc = pymupdf.open(src)
doc[0].add_redact_annot(pymupdf.Rect(*rect), fill=(0, 0, 0))
doc[0].apply_redactions()
doc.save(theirs)


def words(path):
    return [
        [w[4], round(w[0], 3), round(w[1], 3), round(w[2], 3), round(w[3], 3)]
        for w in pymupdf.open(path)[0].get_text("words")
    ]


def gray(path):
    pm = pymupdf.open(path)[0].get_pixmap(dpi=36, colorspace=pymupdf.csGRAY)
    return pm.width, pm.height, pm.samples


def ssim(a, b):
    # Mean SSIM over 8x8 windows of two equal-size 8-bit gray images.
    (w, h, sa), (_, _, sb) = a, b
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    scores = []
    for y in range(0, h - 7, 8):
        for x in range(0, w - 7, 8):
            pa = [sa[(y + j) * w + x + i] for j in range(8) for i in range(8)]
            pb = [sb[(y + j) * w + x + i] for j in range(8) for i in range(8)]
            ma, mb = sum(pa) / 64, sum(pb) / 64
            va = sum((p - ma) ** 2 for p in pa) / 63
            vb = sum((p - mb) ** 2 for p in pb) / 63
            cov = sum((p - ma) * (q - mb) for p, q in zip(pa, pb)) / 63
            scores.append(
                ((2 * ma * mb + c1) * (2 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2))
            )
    return sum(scores) / len(scores)


print(json.dumps({"ours": words(ours), "theirs": words(theirs), "ssim": ssim(gray(ours), gray(theirs))}))
"""


def _real_pymupdf_python() -> str | None:
    """An interpreter with the REAL PyMuPDF: the ``.venv-oracle`` next to the
    repo when present, else this one if a fresh process (no ``fitz`` shim
    installed) imports a ``pymupdf`` that is not pdfspine's."""
    root = os.path.join(os.path.dirname(__file__), "..", "..", ".venv-oracle")
    for candidate in (
        os.path.join(root, "bin", "python"),
        os.path.join(root, "Scripts", "python.exe"),
    ):
        if os.path.exists(candidate):
            return candidate
    # pdfspine's shim imports pdfspine; the real package never does.
    probe = "import pymupdf, sys; sys.exit('pdfspine' in sys.modules)"
    ok = subprocess.run([sys.executable, "-c", probe], capture_output=True)
    return sys.executable if ok.returncode == 0 else None


def _assert_words_match(got, ref, tol: float = 0.5) -> None:
    """Same words in the same order, every box edge within ``tol`` pt."""
    assert [w[0] for w in got] == [w[0] for w in ref]
    for g, r in zip(got, ref):
        for axis, a, b in zip(("x0", "y0", "x1", "y1"), g[1:], r[1:]):
            assert abs(a - b) <= tol, f"{r[0]} {axis}: {a} vs {b}"


def test_pym4_redact_003_quote_operators_match_real_pymupdf(tmp_path):
    # pdfspine redacts the `'` / `"` page; real PyMuPDF redacts the explicit
    # `T*` / `Tw` / `Tc` / `Tj` spelling of the same page (its own filter
    # mishandles `'` / `"`: MuPDF 1.27 drops the leading and collapses the
    # lines). Both results are read and rendered by real PyMuPDF so only the
    # rewrite differs. Before the fix pdfspine put lines two to four on line
    # one's baseline and lost the `"` spacing (words off by 14+ pt).
    oracle = _real_pymupdf_python()
    if oracle is None:
        pytest.skip("real PyMuPDF not available")
    src = tmp_path / "src.pdf"
    src.write_bytes(_quote_ops_doc(explicit=True))
    ours = tmp_path / "ours.pdf"
    theirs = tmp_path / "theirs.pdf"

    data = _quote_ops_doc()
    doc = pdfspine.open(stream=data)
    doc[0].add_redact_annot(_QUOTE_OPS_RECT, fill=(0, 0, 0))
    assert doc[0].apply_redactions() == 1
    doc.save(str(ours))

    out = subprocess.run(
        [
            oracle,
            "-c",
            _ORACLE_SCRIPT,
            str(src),
            json.dumps(_QUOTE_OPS_RECT),
            str(ours),
            str(theirs),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    result = json.loads(out)
    survivors = ["LINE", "ONE", "KEEP", "TAIL", "LINE", "THREE", "LINE", "FOUR"]
    assert [w[0] for w in result["theirs"]] == survivors
    _assert_words_match(result["ours"], result["theirs"])
    assert result["ssim"] >= 0.99, f"render SSIM {result['ssim']:.4f}"

    # pdfspine's own reader: the survivors keep the boxes they had before.
    def words(d):
        return [(w[4], *w[:4]) for w in d[0].get_text("words")]

    before = [w for w in words(pdfspine.open(stream=data)) if w[0] != "SECRET"]
    after = words(pdfspine.open(str(ours)))
    assert [w[0] for w in after] == survivors
    _assert_words_match(after, before)


# --- PYM4-WIDGET-* : forms + Widget ----------------------------------------


def test_pym4_widget_001_list_fields():
    doc = pdfspine.open(stream=acroform_doc())
    assert doc.is_form_pdf
    page = doc[0]
    widgets = page.widgets()
    assert len(widgets) == 1
    w = widgets[0]
    assert w.field_name == "tx1"
    assert w.field_type_string == "Text"
    assert w.field_value == "init"
    assert isinstance(page.first_widget, pdfspine.Widget)


def test_pym4_widget_002_update_value_persists():
    doc = pdfspine.open(stream=acroform_doc())
    w = doc[0].widgets()[0]
    w.update("changed")
    re = pdfspine.open(stream=doc.tobytes())
    assert re.is_form_pdf
    assert re[0].widgets()[0].field_value == "changed"


# --- PYM4-EMBFILE-* / PYM4-SCRUB-* -----------------------------------------


def test_pym4_embfile_001_roundtrip():
    doc = blank_doc()
    doc.embfile_add(
        "data.bin", b"\x00\x01payload\xff", filename="data.bin", desc="a blob"
    )
    assert doc.embfile_names() == ["data.bin"]
    assert doc.embfile_count() == 1
    assert doc.embfile_get("data.bin") == b"\x00\x01payload\xff"
    info = doc.embfile_info("data.bin")
    assert info["filename"] == "data.bin"
    assert info["size"] == len(b"\x00\x01payload\xff")
    # persists across save/reopen
    re = pdfspine.open(stream=doc.tobytes())
    assert re.embfile_names() == ["data.bin"]
    assert re.embfile_get("data.bin") == b"\x00\x01payload\xff"


def test_pym4_scrub_001_removes_metadata():
    doc = blank_doc()
    doc.set_metadata({"title": "Confidential", "author": "Spy"})
    assert doc.metadata.get("title") == "Confidential"
    doc.scrub(metadata=True)
    md = doc.metadata
    assert not md.get("title")
    assert not md.get("author")


# --- PYM4-FITZ-* : deprecated-alias parity ---------------------------------


def test_pym4_fitz_001_camelcase_aliases(tmp_path):
    import fitz

    # annotations / drawings / insert / shape via camelCase
    doc = fitz.open(stream=blank_doc().tobytes())
    page = doc[0]
    page.insertText((72, 100), "ALIASED")
    a = page.addHighlightAnnot([_HL_QUAD])
    assert a.type[1] == "Highlight"
    assert page.firstAnnot is not None
    shape = page.newShape()
    shape.draw_rect((10, 10, 50, 50))
    shape.finish(color=(0, 0, 0))
    shape.commit()
    assert isinstance(page.getDrawings(), list)
    re = fitz.open(stream=doc.tobytes())
    assert "ALIASED" in re[0].get_text()

    # redaction via applyRedactions
    data, rect = secret_doc("KEEP ", "HIDDEN")
    rdoc = fitz.open(stream=data)
    rdoc[0].addRedactAnnot(rect)
    assert rdoc[0].applyRedactions() == 1
    reopened = fitz.open(stream=rdoc.tobytes())
    assert "HIDDEN" not in reopened[0].get_text()


def test_pym4_fitz_002_classes_exposed():
    import fitz

    assert fitz.Annot is pdfspine.Annot
    assert fitz.Widget is pdfspine.Widget
    assert fitz.Shape is pdfspine.Shape
