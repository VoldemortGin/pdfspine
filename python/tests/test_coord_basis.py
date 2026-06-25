"""Coordinate-basis unification (MediaBox → CropBox) cross-channel tests.

PRD: ``docs/PRD-coordinate-basis-unification.md``. On pages where
``CropBox != MediaBox`` the digital-text / vector / drawing device coordinates
must share a single origin (the **CropBox**) with the already-CropBox-based
render / svg / ocr channels, eliminating the old MediaBox-vs-CropBox
cross-channel spatial offset. ``CropBox == MediaBox`` pages are unchanged.

Catalog IDs: ``COORD-BASIS-*``. All fixtures are self-generated raw PDFs (no
PyMuPDF files), placing text at known user-space coordinates.
"""

from __future__ import annotations

import pdfspine
from pdfspine.geometry import Matrix, Point


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
    """A minimal cross-reference-table PDF from ``(num, body)`` objects."""
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


def _page_pdf(mediabox, cropbox, text, tx, ty, *, size=40, rotate=0) -> bytes:
    """A one-page PDF: ``text`` (Helvetica) with its origin at user ``(tx, ty)``,
    an explicit ``/MediaBox`` and optional ``/CropBox`` + ``/Rotate``."""
    content = f"BT /F1 {size} Tf {tx} {ty} Td ({text}) Tj ET".encode()
    page = b"<< /Type /Page /Parent 2 0 R " + (
        f"/MediaBox [{mediabox[0]} {mediabox[1]} {mediabox[2]} {mediabox[3]}] ".encode()
    )
    if cropbox is not None:
        page += (
            f"/CropBox [{cropbox[0]} {cropbox[1]} {cropbox[2]} {cropbox[3]}] ".encode()
        )
    if rotate:
        page += f"/Rotate {rotate} ".encode()
    page += b"/Contents 4 0 R /Resources << /Font << /F1 3 0 R >> >> >>"
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [5 0 R] >>"),
        (3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        (
            4,
            f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream",
        ),
        (5, page),
    ]
    return _build_pdf(objects, root=1)


def _content_page_pdf(mediabox, cropbox, content: bytes, *, with_font=False) -> bytes:
    """A one-page PDF with a raw ``content`` stream, an explicit ``/MediaBox`` and
    optional ``/CropBox``. Includes a Helvetica ``/F1`` when ``with_font`` (so the
    content may place real words for table detection)."""
    page = b"<< /Type /Page /Parent 2 0 R " + (
        f"/MediaBox [{mediabox[0]} {mediabox[1]} {mediabox[2]} {mediabox[3]}] ".encode()
    )
    if cropbox is not None:
        page += (
            f"/CropBox [{cropbox[0]} {cropbox[1]} {cropbox[2]} {cropbox[3]}] ".encode()
        )
    page += b"/Contents 4 0 R "
    if with_font:
        page += b"/Resources << /Font << /F1 3 0 R >> >> >>"
    else:
        page += b"/Resources << >> >>"
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [5 0 R] >>"),
        (3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        (
            4,
            f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream",
        ),
        (5, page),
    ]
    return _build_pdf(objects, root=1)


def _ink_bbox(pix):
    """Bounding box ``(x0, y0, x1, y1)`` of the dark (inked) pixels in a Pixmap,
    or ``None`` when the page rendered blank."""
    w, h, n, stride = pix.width, pix.height, pix.n, pix.stride
    s = pix.samples
    x0 = y0 = 1 << 30
    x1 = y1 = -1
    chans = min(n, 3)  # ignore the alpha channel if present
    for y in range(h):
        base_row = y * stride
        for x in range(w):
            base = base_row + x * n
            if any(s[base + c] < 200 for c in range(chans)):
                x0, x1 = min(x0, x), max(x1, x)
                y0, y1 = min(y0, y), max(y1, y)
    if x1 < 0:
        return None
    return (x0, y0, x1 + 1, y1 + 1)


# --- COORD-BASIS-001: digital text shares the CropBox origin with render ----


def test_coord_basis_001_text_aligns_with_render_on_cropped_page():
    # A page with real crop margins (MediaBox != CropBox), and an equivalent
    # un-cropped page whose MediaBox is exactly the visible crop region. The text
    # sits at the SAME position relative to the CropBox origin on both.
    mb = (0, 0, 400, 400)
    cb = (100, 80, 300, 360)  # cw=200, ch=280; asymmetric x/y margins
    doc_crop = pdfspine.open(stream=_page_pdf(mb, cb, "H", 100 + 30, 80 + 200))
    doc_plain = pdfspine.open(stream=_page_pdf((0, 0, 200, 280), None, "H", 30, 200))
    pc, pp = doc_crop[0], doc_plain[0]

    wc = pc.get_text("words")
    wp = pp.get_text("words")
    assert wc and wp, "text must be extracted on both pages"
    # CropBox is the basis with ZERO crop offset → identical device bboxes.
    bc, bp = wc[0][:4], wp[0][:4]
    for a, b in zip(bc, bp):
        assert abs(a - b) < 1e-3, f"text bbox differs crop vs plain: {bc} vs {bp}"

    # Render is already CropBox-based: same visible size and identical pixels.
    pixc, pixp = pc.get_pixmap(), pp.get_pixmap()
    assert (pixc.width, pixc.height) == (200, 280) == (pixp.width, pixp.height)
    assert bytes(pixc.samples) == bytes(pixp.samples)

    # Cross-layer: the rendered ink overlaps the reported text device bbox (one
    # origin). Under the OLD MediaBox basis the text bbox would be shifted by the
    # crop margin (~100 px in x, ~40 px in y) and would NOT overlap the ink.
    ink = _ink_bbox(pixc)
    assert ink is not None, "text must render to ink"
    tx0, ty0, tx1, ty1 = bc
    ix0, iy0, ix1, iy1 = ink
    overlap_x = min(tx1, ix1) - max(tx0, ix0)
    overlap_y = min(ty1, iy1) - max(ty0, iy0)
    assert overlap_x > 0 and overlap_y > 0, (
        f"text bbox {bc} does not overlap rendered ink {ink} (cross-channel offset)"
    )


# --- COORD-BASIS-002: CropBox == MediaBox is unchanged (regression) ---------


def test_coord_basis_002_crop_equals_media_unchanged():
    mb = (0, 0, 200, 280)
    doc_none = pdfspine.open(stream=_page_pdf(mb, None, "H", 30, 200))
    doc_full = pdfspine.open(stream=_page_pdf(mb, (0, 0, 200, 280), "H", 30, 200))
    bn = doc_none[0].get_text("words")[0][:4]
    bf = doc_full[0].get_text("words")[0][:4]
    assert bn == bf, f"explicit full CropBox changed coords: {bn} vs {bf}"


# --- COORD-BASIS-003: derotation inverts the text bbox on a cropped page -----


def test_coord_basis_003_derotation_inverts_text_bbox_with_crop():
    mb = (0, 0, 400, 400)
    cb = (100, 80, 300, 360)
    tx, ty = 100 + 30, 80 + 200
    p0 = pdfspine.open(stream=_page_pdf(mb, cb, "H", tx, ty, rotate=0))[0]
    p90 = pdfspine.open(stream=_page_pdf(mb, cb, "H", tx, ty, rotate=90))[0]

    # rotation_matrix and derotation_matrix compose to the identity on a
    # CropBox != MediaBox page (the crop offset is carried consistently).
    ident = Matrix(*p90.rotation_matrix) * Matrix(*p90.derotation_matrix)
    for a, b in zip(tuple(ident), (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)):
        assert abs(a - b) < 1e-6

    bb0 = p0.get_text("words")[0][:4]
    bb90 = p90.get_text("words")[0][:4]
    dm = p90.derotation_matrix
    # De-rotating the rotated text bbox returns the unrotated CropBox-based bbox:
    # text bbox and derotation_matrix now share one CropBox basis, so the
    # round-trip is exact (before the fix the two bases differed).
    c1 = Point(bb90[0], bb90[1]).transform(dm)
    c2 = Point(bb90[2], bb90[3]).transform(dm)
    rx0, rx1 = sorted((c1.x, c2.x))
    ry0, ry1 = sorted((c1.y, c2.y))
    for a, b in zip((rx0, ry0, rx1, ry1), bb0):
        assert abs(a - b) < 1e-3, (
            f"derotated rotated bbox {(rx0, ry0, rx1, ry1)} != unrotated {bb0}"
        )


# --- COORD-BASIS-004: get_drawings shares the CropBox origin (EDIT3) ---------


def test_coord_basis_004_drawings_share_cropbox_origin():
    # A stroked rectangle placed at KNOWN PDF user-space coordinates, fully inside
    # the CropBox, on a MediaBox != CropBox page. `get_drawings` reports device
    # geometry; under the CropBox basis the device coords are CropBox-relative
    # (zero crop offset). `crates/pdf-edit/src/drawings.rs` get_drawings.
    mb = (0, 0, 400, 400)
    cb = (100, 80, 300, 360)  # cw=200, ch=280; asymmetric x/y crop margins
    # `x y w h re`: user-space rectangle (150,120)-(250,200).
    content = b"q 1 0 0 RG 2 w 150 120 100 80 re S Q"
    doc = pdfspine.open(stream=_content_page_pdf(mb, cb, content))
    drawings = doc[0].get_drawings()
    assert drawings, "a vector drawing must be extracted"
    d = drawings[0]
    assert d["type"] == "s", "stroked rect → type 's'"

    # CropBox basis, rotate 0: device(px,py) = (px - x0_crop, y1_crop - py)
    #   = (px - 100, 360 - py).  (150,120)->(50,240) ; (250,200)->(150,160).
    # The crop offset is ZERO relative to the CropBox origin — this is the whole
    # point of the basis unification. Under the OLD MediaBox basis the rect would
    # be (150,200,250,280) (offset by the crop margin) and these would FAIL.
    exp = (50.0, 160.0, 150.0, 240.0)
    r = d["rect"]
    got = (r.x0, r.y0, r.x1, r.y1)
    for a, b in zip(got, exp):
        assert abs(a - b) < 1e-3, f"drawing rect {got} != CropBox-relative {exp}"
    # The `re` item carries the same CropBox-relative device rect.
    op, ir = d["items"][0]
    assert op == "re"
    for a, b in zip((ir.x0, ir.y0, ir.x1, ir.y1), exp):
        assert abs(a - b) < 1e-3, f"re item {tuple(ir)} != CropBox-relative {exp}"

    # Cross-check: an equivalent UN-cropped page (MediaBox == the crop region, the
    # rectangle translated by the crop origin (-100,-80)) yields the SAME device
    # rect — i.e. cropped geometry lands at the same place as if it were never
    # cropped, confirming the single shared CropBox origin.
    content2 = b"q 1 0 0 RG 2 w 50 40 100 80 re S Q"  # (50,40)-(150,120)
    doc2 = pdfspine.open(stream=_content_page_pdf((0, 0, 200, 280), None, content2))
    r2 = doc2[0].get_drawings()[0]["rect"]
    for a, b in zip((r2.x0, r2.y0, r2.x1, r2.y1), exp):
        assert abs(a - b) < 1e-3, f"un-cropped rect {tuple(r2)} != cropped {exp}"


# --- COORD-BASIS-005: find_tables cells share the CropBox origin (EDIT2) -----


def test_coord_basis_005_find_tables_cell_shares_cropbox_origin():
    # A 2x2 vector-ruled table (grid lines + a word per cell) at KNOWN PDF
    # user-space coordinates, fully inside the CropBox, on a MediaBox != CropBox
    # page. `find_tables` maps the page drawings (the rulings) into device space
    # via `crates/pdf-api/src/tables.rs` page_transform(cropbox, ...). Under the
    # CropBox basis the detected cell/table device geometry is CropBox-relative
    # (zero crop offset). The textpage words already share that basis (EDIT1), so
    # words land in their cells and extraction works.
    mb = (0, 0, 400, 400)
    cb = (100, 80, 300, 360)
    # Grid (PDF user space, y-up): vertical rulings x∈{120,200,280};
    # horizontal rulings y∈{200,250,300}. → 2 rows × 2 cols, all inside CropBox.
    grid = [
        (120, 300, 280, 300),
        (120, 250, 280, 250),
        (120, 200, 280, 200),
        (120, 200, 120, 300),
        (200, 200, 200, 300),
        (280, 200, 280, 300),
    ]
    c = "1 w\n"
    for x0, y0, x1, y1 in grid:
        c += f"{x0} {y0} m {x1} {y1} l S\n"
    c += "BT /F1 10 Tf\n"
    for x, y, t in [(135, 270, "A1"), (215, 270, "B1"), (135, 220, "A2"), (215, 220, "B2")]:
        c += f"1 0 0 1 {x} {y} Tm ({t}) Tj\n"
    c += "ET\n"
    doc = pdfspine.open(stream=_content_page_pdf(mb, cb, c.encode(), with_font=True))
    finder = doc[0].find_tables()
    assert len(finder) >= 1, "a ruled vector table must be detected on the crop page"
    table = finder.tables[0]
    assert (table.row_count, table.col_count) == (2, 2), "expected a 2x2 grid"

    # CropBox basis, rotate 0: device(px,py) = (px - 100, 360 - py).
    # Table bbox: user x∈[120,280], y∈[200,300] → device (20,60,180,160).
    # Under the OLD MediaBox basis the rulings map to (120,100,280,200) — i.e.
    # offset by the crop margin — so these pins FAIL after reverting EDIT2.
    bb = table.bbox
    for a, b in zip((bb.x0, bb.y0, bb.x1, bb.y1), (20.0, 60.0, 180.0, 160.0)):
        assert abs(a - b) < 1.0, f"table bbox {tuple(bb)} != CropBox-relative"

    # Top-left cell (device row 0 / col 0): user x∈[120,200], y∈[250,300] →
    # device (20,60,100,110). Pinned to the CropBox-relative device origin.
    cell = table.cells[0][0]
    assert cell is not None, "top-left cell must exist"
    for a, b in zip((cell.x0, cell.y0, cell.x1, cell.y1), (20.0, 60.0, 100.0, 110.0)):
        assert abs(a - b) < 1.0, f"cell[0][0] {tuple(cell)} != CropBox-relative"

    # The words (CropBox basis already) land in their cells → extraction works.
    assert table.extract() == [["A1", "B1"], ["A2", "B2"]]
