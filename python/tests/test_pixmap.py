"""PIXMAP-* / PIXMAP-IMGONLY-* / PIXMAP-BUF-LIFETIME-* / EXTRACT-IMAGE-* —
``Pixmap``, ``page.get_pixmap``, the buffer-protocol COW lifetime contract, and
``doc.extract_image`` from Python (PRD §3.3 / §8.10 / §9.4).

All fixtures are self-generated in-test (raw PDF bytes via ``stream=``) — no
external / PyMuPDF files (PRD §10).
"""

from __future__ import annotations

import gc
import struct
import zlib

import oxide_pdf
import pytest


# --- self-generated image-only PDF fixtures -------------------------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    max_num = 0
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
        max_num = max(max_num, num)
    size = max_num + 1
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


def _rgb_samples(w: int, h: int) -> bytes:
    s = bytearray()
    for y in range(h):
        for x in range(w):
            s += bytes([(x * 17) & 0xFF, (y * 23) & 0xFF, ((x + y) * 5) & 0xFF])
    return bytes(s)


def image_only_pdf(w: int, h: int, samples: bytes, content: str) -> bytes:
    """A 1-page PDF drawing one Flate-encoded RGB image XObject (obj 4)."""
    img = zlib.compress(samples)
    img_obj = (
        b"<< /Type /XObject /Subtype /Image "
        + f"/Width {w} /Height {h} ".encode()
        + b"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode "
        + f"/Length {len(img)} ".encode()
        + b">>\nstream\n"
        + img
        + b"\nendstream"
    )
    content_b = content.encode()
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                b"/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>",
            ),
            (4, img_obj),
            (
                5,
                f"<< /Length {len(content_b)} >>\nstream\n".encode()
                + content_b
                + b"\nendstream",
            ),
        ],
        root=1,
    )


_DRAW = "q 200 0 0 200 0 0 cm /Im0 Do Q"


# --- PYPIXMAP-001: image-only page get_pixmap → correct w/h/samples --------


def test_pypixmap_001_image_only_get_pixmap():
    w, h = 8, 6
    samples = _rgb_samples(w, h)
    doc = oxide_pdf.open(stream=image_only_pdf(w, h, samples, _DRAW))
    page = doc[0]
    assert page.is_image_only
    pix = page.get_pixmap()
    assert (pix.width, pix.height, pix.n) == (w, h, 3)
    assert pix.colorspace == "DeviceRGB"
    assert len(pix.samples) == w * h * 3
    # Pixel-equality with the source raster.
    assert bytes(pix.samples) == samples


# --- PYPIXMAP-002: pix.save() reopens as that image -----------------------


def test_pypixmap_002_save_png_roundtrip(tmp_path):
    w, h = 8, 6
    samples = _rgb_samples(w, h)
    doc = oxide_pdf.open(stream=image_only_pdf(w, h, samples, _DRAW))
    pix = doc[0].get_pixmap()
    out = tmp_path / "out.png"
    pix.save(str(out))
    data = out.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    # Parse the PNG IHDR width/height to confirm geometry round-trips.
    assert data[12:16] == b"IHDR"
    pw, ph = struct.unpack(">II", data[16:24])
    assert (pw, ph) == (w, h)


# --- PYPIXMAP-003: memoryview(pix) works + np.frombuffer path -------------


def test_pypixmap_003_memoryview():
    w, h = 4, 4
    doc = oxide_pdf.open(stream=image_only_pdf(w, h, _rgb_samples(w, h), _DRAW))
    pix = doc[0].get_pixmap()
    mv = memoryview(pix)
    assert len(mv) == w * h * 3
    assert mv.readonly
    assert mv.format == "B"
    # samples_mv is the same zero-copy view.
    assert len(pix.samples_mv) == len(mv)
    mv.release()


# --- PIXMAP-BUF-LIFETIME: hold a view, drop the Pixmap, mutate, read -------


def test_pixmap_buf_lifetime():
    # Build a blank gray pixmap and write a known pattern.
    pix = oxide_pdf.Pixmap(1, (0, 0, 2, 2), False)  # n=1, 4 bytes
    for y in range(2):
        for x in range(2):
            pix.set_pixel(x, y, [10 * (y * 2 + x) + 1])
    mv = memoryview(pix)
    snapshot = bytes(mv)

    # (a) Drop the Pixmap while the view is live: the bytes survive (the buffer
    #     export keeps an Arc clone alive). No crash, no use-after-free.
    del pix
    gc.collect()
    assert bytes(mv) == snapshot

    # (b) Mutate via a *fresh* handle (a new Pixmap can't touch this buffer);
    #     and an in-place mutation on a pixmap with a live view copies-on-write.
    pix2 = oxide_pdf.Pixmap(1, (0, 0, 2, 2), False)
    mv2 = memoryview(pix2)
    base = bytes(mv2)
    pix2.clear_with(0xFF)  # in-place mutate under a live view → COW
    assert bytes(mv2) == base  # the live view is unchanged
    assert bytes(pix2.samples) == b"\xff\xff\xff\xff"  # the pixmap did change

    mv.release()
    mv2.release()


# --- PYPIXMAP-VECTOR: a vector page now RENDERS (M6d) ----------------------


def test_pypixmap_vector_page_renders():
    w, h = 4, 4
    samples = _rgb_samples(w, h)
    # Content paints a red path (re/f) → a vector page. In M6d this renders
    # full-page (no PdfUnsupportedError); the page MediaBox is 200x200.
    doc = oxide_pdf.open(stream=image_only_pdf(w, h, samples, "1 0 0 rg 0 0 20 20 re f"))
    page = doc[0]
    assert not page.is_image_only
    pix = page.get_pixmap()
    assert (pix.width, pix.height) == (200, 200)
    assert pix.colorspace == "DeviceRGB"
    # The bottom-left 20x20 user-space square is red; device y is flipped, so it
    # lands at the bottom of the raster. Sample inside it.
    red = pix.pixel(5, 195)
    assert red[0] > 200 and red[1] < 60 and red[2] < 60
    # A non-empty raster.
    assert any(b != 255 for b in pix.samples)


# --- PYPIXMAP-UNDECODABLE: bad image but get_text still works --------------


def test_pypixmap_undecodable_image_text_independent():
    # A page whose image is a DCTDecode stream of garbage (not a real JPEG),
    # PLUS a text layer so get_text returns content. The page is a *vector*
    # page (text present) for Pixmap purposes, but text extraction works.
    junk = b"not a real jpeg payload"
    img_obj = (
        b"<< /Type /XObject /Subtype /Image /Width 4 /Height 4 "
        b"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode "
        + f"/Length {len(junk)} ".encode()
        + b">>\nstream\n"
        + junk
        + b"\nendstream"
    )
    content = b"q 100 0 0 100 0 0 cm /Im0 Do Q BT /F1 12 Tf 10 10 Td (hello) Tj ET"
    pdf = _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                b"/Resources << /XObject << /Im0 4 0 R >> "
                b"/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> "
                b"/Contents 5 0 R >>",
            ),
            (4, img_obj),
            (
                5,
                f"<< /Length {len(content)} >>\nstream\n".encode()
                + content
                + b"\nendstream",
            ),
        ],
        root=1,
    )
    doc = oxide_pdf.open(stream=pdf)
    page = doc[0]
    # get_text works regardless of the broken image.
    assert "hello" in page.get_text("text")
    # get_pixmap now RENDERS the page (M6d): the undecodable image is skipped
    # (the §8.4.1 degradation contract — a broken image never aborts the render),
    # and the rest of the page (here a text layer) still rasterizes.
    pix = page.get_pixmap()
    assert (pix.width, pix.height) == (200, 200)
    assert pix.colorspace == "DeviceRGB"


# --- PYEXTRACT-IMAGE-001: doc.extract_image → dict shape ------------------


def test_pyextract_image_001_dict():
    w, h = 8, 6
    samples = _rgb_samples(w, h)
    doc = oxide_pdf.open(stream=image_only_pdf(w, h, samples, _DRAW))
    info = doc.extract_image(4)
    assert info["ext"] == "png"
    assert info["width"] == w
    assert info["height"] == h
    assert info["bpc"] == 8
    assert info["colorspace"] == "DeviceRGB"
    assert info["n"] == 3
    assert info["image"][:8] == b"\x89PNG\r\n\x1a\n"


# --- PYFITZ-PIXMAP: fitz shim parity --------------------------------------


def test_pyfitz_pixmap_parity(tmp_path):
    import fitz

    assert fitz.Pixmap is oxide_pdf.Pixmap
    w, h = 4, 4
    samples = _rgb_samples(w, h)
    doc = fitz.open(stream=image_only_pdf(w, h, samples, _DRAW))
    page = doc[0]
    # snake_case + camelCase both resolve.
    pix = page.get_pixmap()
    pix2 = page.getPixmap()
    assert pix.width == pix2.width == w
    assert bytes(pix.samples) == samples
    # extract_image / extractImage parity.
    a = doc.extract_image(4)
    b = doc.extractImage(4)
    assert a["width"] == b["width"] == w


# --- PYPIXMAP-SCALE / ALPHA -----------------------------------------------


def test_pypixmap_scale_and_alpha():
    w, h = 8, 6
    samples = _rgb_samples(w, h)
    doc = oxide_pdf.open(stream=image_only_pdf(w, h, samples, _DRAW))
    page = doc[0]
    # dpi=144 → 2x scale.
    pix = page.get_pixmap(dpi=144)
    assert (pix.width, pix.height) == (2 * w, 2 * h)
    # Matrix scale of 2.
    pix2 = page.get_pixmap(matrix=(2, 0, 0, 2, 0, 0))
    assert (pix2.width, pix2.height) == (2 * w, 2 * h)
    # alpha=True → 4 components, opaque.
    pix3 = page.get_pixmap(alpha=True)
    assert pix3.n == 4 and pix3.alpha
    assert all(px == 255 for px in bytes(pix3.samples)[3::4])


# --- PYPIXMAP-BLANK: constructor + pixel ----------------------------------


def test_pypixmap_blank_and_pixel():
    pix = oxide_pdf.Pixmap(3, (0, 0, 2, 2), False)
    assert (pix.width, pix.height, pix.n) == (2, 2, 3)
    assert pix.pixel(0, 0) == (0, 0, 0)
    pix.set_pixel(1, 1, [9, 8, 7])
    assert pix.pixel(1, 1) == (9, 8, 7)


# ===========================================================================
# M6d — full-page render + DisplayList from Python (PYRENDER-*).
# ===========================================================================


def _vector_page_pdf(content: str, media: str = "[0 0 200 200]") -> bytes:
    """A 1-page PDF with no resources, drawing `content` (a self-built page)."""
    body = content.encode()
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox "
                + media.encode()
                + b" /Resources << >> /Contents 4 0 R >>",
            ),
            (
                4,
                f"<< /Length {len(body)} >>\nstream\n".encode() + body + b"\nendstream",
            ),
        ],
        root=1,
    )


# --- PYRENDER-001: a self-built page renders to a non-blank sized Pixmap ----


def test_pyrender_001_page_renders_non_blank():
    # A green rect over most of the page: a self-built page → non-blank raster.
    doc = oxide_pdf.open(stream=_vector_page_pdf("0 1 0 rg 20 20 160 160 re f"))
    page = doc[0]
    pix = page.get_pixmap()
    assert (pix.width, pix.height, pix.n) == (200, 200, 3)
    assert pix.colorspace == "DeviceRGB"
    # The rect center is green (device center maps inside the rect).
    cx = pix.pixel(100, 100)
    assert cx[1] > 200 and cx[0] < 60 and cx[2] < 60
    assert any(b != 255 for b in pix.samples)


# --- PYRENDER-002: a vector page renders (no raise) + pix.save(png) works ---


def test_pyrender_002_vector_page_save(tmp_path):
    doc = oxide_pdf.open(stream=_vector_page_pdf("0 0 1 rg 0 0 200 200 re f"))
    pix = doc[0].get_pixmap()  # no PdfUnsupportedError
    out = tmp_path / "page.png"
    pix.save(str(out))
    data = out.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    assert data[12:16] == b"IHDR"
    pw, ph = struct.unpack(">II", data[16:24])
    assert (pw, ph) == (200, 200)


# --- PYRENDER-003: dpi scales the rendered dimensions ----------------------


def test_pyrender_003_dpi_scales():
    doc = oxide_pdf.open(stream=_vector_page_pdf("1 0 0 rg 0 0 200 200 re f"))
    page = doc[0]
    base = page.get_pixmap()
    assert (base.width, base.height) == (200, 200)
    hi = page.get_pixmap(dpi=144)
    assert (hi.width, hi.height) == (400, 400)
    m2 = page.get_pixmap(matrix=(2, 0, 0, 2, 0, 0))
    assert (m2.width, m2.height) == (400, 400)


# --- PYRENDER-004: DisplayList replay matches get_pixmap -------------------


def test_pyrender_004_displaylist_replay_matches():
    doc = oxide_pdf.open(stream=_vector_page_pdf("0 0 1 rg 30 30 140 140 re f"))
    page = doc[0]
    direct = page.get_pixmap()
    dl = page.get_displaylist()
    assert dl.rect == (0.0, 0.0, 200.0, 200.0)
    replay = dl.get_pixmap()
    assert (replay.width, replay.height) == (direct.width, direct.height)
    assert bytes(replay.samples) == bytes(direct.samples)
    # DisplayList is exported on the package + fitz shim.
    assert isinstance(dl, oxide_pdf.DisplayList)


# --- PYRENDER-005: fitz shim parity on a rendered page --------------------


def test_pyrender_005_fitz_parity():
    import fitz

    pdf = _vector_page_pdf("1 0 0 rg 10 10 180 180 re f")
    doc = fitz.open(stream=pdf)
    page = doc.load_page(0)
    pix = page.get_pixmap()
    assert (pix.width, pix.height) == (200, 200)
    # camelCase alias + DisplayList exported on fitz.
    pix2 = page.getPixmap()
    assert (pix2.width, pix2.height) == (200, 200)
    assert fitz.DisplayList is oxide_pdf.DisplayList
    dl = page.get_displaylist()
    assert bytes(dl.get_pixmap().samples) == bytes(pix.samples)
