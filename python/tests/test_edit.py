"""M3d Python edit/save surface tests (PRD §8.7 / §8.9 / §8.4).

`PYSAVE-*`/`PYMETA-*`/`PYTOC-*`/`PYMERGE-*`/`PYEDIT-*`/`PYLINK-*`/`PYLABEL-*`/
`PYENC-*` exercise the native ``oxide_pdf`` package; the deprecated PyMuPDF aliases
(`saveIncr`/`setMetadata`/`getToC`/`setToC`/`insertPDF`/`newPage`) are checked too.
All fixtures are self-generated in-test (PRD §10).
"""

from __future__ import annotations

import oxide_pdf
import pytest


def _build_pdf(objects: list[tuple[int, bytes]], root: int, extra_trailer: bytes = b"") -> bytes:
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
    out += b"trailer\n"
    out += f"<< /Size {size} /Root {root} 0 R {extra_trailer.decode()} >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


def multi_page_pdf(markers: list[str]) -> bytes:
    """An N-page doc; each page shows a one-word marker, sharing font object 3."""
    objects: list[tuple[int, bytes]] = [
        (3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]
    kids = []
    for i, marker in enumerate(markers):
        leaf = 4 + i * 2
        content = leaf + 1
        kids.append(f"{leaf} 0 R")
        body = f"BT /F1 12 Tf 20 100 Td ({marker}) Tj ET".encode()
        objects.append(
            (
                leaf,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                + f"/Contents {content} 0 R ".encode()
                + b"/Resources << /Font << /F1 3 0 R >> >> >>",
            )
        )
        objects.append(
            (content, f"<< /Length {len(body)} >>\nstream\n".encode() + body + b"\nendstream")
        )
    objects.append((1, b"<< /Type /Catalog /Pages 2 0 R >>"))
    objects.append(
        (
            2,
            b"<< /Type /Pages /Count "
            + str(len(markers)).encode()
            + b" /Kids ["
            + b" ".join(k.encode() for k in kids)
            + b"] >>",
        )
    )
    objects.sort(key=lambda o: o[0])
    return _build_pdf(objects, root=1)


def _open(markers: list[str]) -> "oxide_pdf.Document":
    return oxide_pdf.open(stream=multi_page_pdf(markers))


# --- save ----------------------------------------------------------------


def test_pysave_001_save_path_reopen(tmp_path):
    doc = _open(["AAA", "BBB"])
    p = tmp_path / "out.pdf"
    doc.save(str(p))
    re = oxide_pdf.open(str(p))
    assert re.page_count == 2
    assert "AAA" in re[0].get_text()


def test_pysave_002_tobytes_roundtrip():
    doc = _open(["AAA", "BBB", "CCC"])
    data = doc.tobytes()
    re = oxide_pdf.open(stream=data)
    assert re.page_count == 3
    assert "CCC" in re[2].get_text()


def test_pysave_003_incremental(tmp_path):
    doc = _open(["AAA"])
    p = tmp_path / "incr.pdf"
    doc.save(str(p))  # full save first (clean parse for incremental)
    re = oxide_pdf.open(str(p))
    re.set_metadata({"title": "Incremental"})
    orig = p.read_bytes()
    re.saveIncr(str(p))
    after = p.read_bytes()
    assert after[: len(orig)] == orig  # byte-exact prefix (append-only)
    re2 = oxide_pdf.open(str(p))
    assert re2.metadata["title"] == "Incremental"


def test_pysave_004_garbage_deflate():
    doc = _open(["AAA", "BBB"])
    data = doc.tobytes(garbage=3, deflate=True)
    re = oxide_pdf.open(stream=data)
    assert re.page_count == 2


# --- metadata ------------------------------------------------------------


def test_pymeta_001_roundtrip():
    doc = _open(["AAA"])
    doc.set_metadata({"title": "My Doc", "author": "Me", "subject": "S"})
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.metadata["title"] == "My Doc"
    assert re.metadata["author"] == "Me"
    assert re.metadata["subject"] == "S"


def test_pymeta_002_deprecated_alias():
    doc = _open(["AAA"])
    doc.setMetadata({"title": "Via Alias"})
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.metadata["title"] == "Via Alias"


def test_pymeta_003_xml():
    doc = _open(["AAA"])
    assert doc.get_xml_metadata() == ""
    xmp = "<?xpacket begin='﻿'?><x:xmpmeta>data</x:xmpmeta>"
    doc.set_xml_metadata(xmp)
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.get_xml_metadata() == xmp


# --- TOC -----------------------------------------------------------------


def test_pytoc_001_roundtrip():
    doc = _open(["AAA", "BBB", "CCC"])
    toc = [[1, "Chapter 1", 0], [2, "Section 1.1", 1], [1, "Chapter 2", 2]]
    doc.set_toc(toc)
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.get_toc() == toc


def test_pytoc_002_deprecated_aliases():
    doc = _open(["AAA", "BBB"])
    doc.setToC([[1, "A", 0], [1, "B", 1]])
    assert doc.getToC() == [[1, "A", 0], [1, "B", 1]]


def test_pytoc_003_level_jump_raises():
    doc = _open(["AAA", "BBB"])
    with pytest.raises(oxide_pdf.PdfError):
        doc.set_toc([[1, "A", 0], [3, "C", 1]])


# --- merge + page ops ----------------------------------------------------


def test_pymerge_001_insert_pdf():
    dst = _open(["AAA", "BBB"])
    src = _open(["XXX", "YYY"])
    dst.insert_pdf(src)
    assert dst.page_count == 4
    re = oxide_pdf.open(stream=dst.tobytes())
    assert re.page_count == 4
    texts = [re[i].get_text() for i in range(4)]
    joined = " ".join(texts)
    assert "AAA" in joined and "XXX" in joined and "YYY" in joined


def test_pymerge_002_deprecated_alias():
    dst = _open(["AAA"])
    src = _open(["ZZZ"])
    dst.insertPDF(src)
    assert dst.page_count == 2


def test_pyedit_001_delete_select_reopen():
    doc = _open(["AAA", "BBB", "CCC"])
    doc.delete_page(1)
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.page_count == 2
    assert "BBB" not in " ".join(re[i].get_text() for i in range(2))

    doc2 = _open(["AAA", "BBB", "CCC"])
    doc2.select([2, 0])
    re2 = oxide_pdf.open(stream=doc2.tobytes())
    assert re2.page_count == 2
    assert "CCC" in re2[0].get_text()
    assert "AAA" in re2[1].get_text()


def test_pyedit_002_new_page():
    doc = _open(["AAA"])
    doc.new_page(-1, 100, 100)
    assert doc.page_count == 2
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.page_count == 2

    doc2 = _open(["AAA"])
    doc2.newPage()  # deprecated alias
    assert doc2.page_count == 2


# --- links + labels ------------------------------------------------------


def test_pylink_001_get_and_insert():
    doc = _open(["AAA", "BBB"])
    page = doc[0]
    page.insert_link({"kind": 2, "from": (10, 10, 100, 30), "uri": "https://oxide_pdf.dev"})
    re = oxide_pdf.open(stream=doc.tobytes())
    links = re[0].get_links()
    assert len(links) == 1
    assert links[0]["kind"] == 2
    assert links[0]["uri"] == "https://oxide_pdf.dev"
    assert isinstance(links[0]["from"], oxide_pdf.Rect)
    assert tuple(links[0]["from"]) == (10.0, 10.0, 100.0, 30.0)


def test_pylabel_001_get_label():
    # A doc with /PageLabels: decimal style with prefix "A-".
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R /PageLabels 3 0 R >>"),
        (2, b"<< /Type /Pages /Count 2 /Kids [4 0 R 5 0 R] >>"),
        (3, b"<< /Nums [0 << /S /D /P (A-) >>] >>"),
        (4, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>"),
        (5, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>"),
    ]
    doc = oxide_pdf.open(stream=_build_pdf(objects, root=1))
    assert doc[0].get_label() == "A-1"
    assert doc[1].get_label() == "A-2"


# --- encryption ----------------------------------------------------------


def test_pyenc_001_aes256_roundtrip():
    doc = _open(["AAA", "BBB"])
    doc.set_metadata({"title": "Secret"})
    data = doc.tobytes(encryption=oxide_pdf.PDF_ENCRYPT_AES_256, user_pw="")
    re = oxide_pdf.open(stream=data)
    assert re.is_encrypted
    assert re.authenticate("") is True
    assert re.metadata["title"] == "Secret"
    assert "AAA" in re[0].get_text()


def test_pyenc_002_owner_wrong_user():
    doc = _open(["AAA"])
    data = doc.tobytes(
        encryption=oxide_pdf.PDF_ENCRYPT_RC4_128, user_pw="theuser", owner_pw="theowner"
    )
    re = oxide_pdf.open(stream=data)
    assert re.is_encrypted
    assert re.authenticate("wrong") is False
    re2 = oxide_pdf.open(stream=data)
    assert re2.authenticate("theowner") is True


# --- fitz shim parity ----------------------------------------------------


def test_pyfitz_edit_aliases():
    import fitz

    doc = fitz.open(stream=multi_page_pdf(["AAA", "BBB"]))
    doc.set_metadata({"title": "Shim"})
    doc.set_toc([[1, "Top", 0]])
    data = doc.tobytes()
    re = fitz.open(stream=data)
    assert re.metadata["title"] == "Shim"
    assert re.get_toc() == [[1, "Top", 0]]
    # Encryption constants surface through the shim.
    assert fitz.PDF_ENCRYPT_AES_256 == oxide_pdf.PDF_ENCRYPT_AES_256
