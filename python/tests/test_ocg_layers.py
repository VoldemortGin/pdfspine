"""PyMuPDF-compat optional-content: layer configurations, OCMDs, hidden render.

``PYOCG-004`` … ``PYOCG-038`` cover the seven ``Document`` methods added on top
of the M7 optional-content surface (``PYOCG-001``…``003`` live in
``test_m7.py``): ``get_layers`` / ``add_layer`` / ``switch_layer`` /
``set_layer_ui_config`` / ``get_oc`` / ``get_ocmd`` / ``set_ocmd``, plus the
parity fixes to ``layer_ui_configs`` (row-index ``number``, ``"radiobox"`` type,
locked label rows) and the in-memory "layer view" reflected by ``get_ocgs`` /
``layer_ui_configs`` / ``ocg_state``, and the Rust interpreter's handling of
hidden optional content (BDC marked content, XObject ``/OC``, OCMDs) during
render / text extraction. ``PYOCG-039`` … ``PYOCG-047`` cover the ``oc=``
parameter of the content writers (``insert_text`` / ``insert_textbox`` /
``insert_image`` / ``show_pdf_page`` / ``Shape.finish`` / ``draw_*`` /
``TextWriter.write_text``): marked-content wrapping, ``/Properties`` key reuse,
XObject ``/OC``, and the PyMuPDF error contract. ``PYOCG-048`` … ``PYOCG-056``
cover the ``/Usage /View /ViewState`` + configuration ``/AS`` visibility rows
(ISO 32000-1 §8.11.4.4) as seen through ``get_text`` / ``get_ocgs`` /
``layer_ui_configs``, including the deliberate ``/AS`` promotion divergence
from MuPDF.

Every expected value is captured from **REAL PyMuPDF 1.27.2** (the "PyMuPDF
1.27.2 oracle") and hardcoded. ``PYOCG-037``/``038``, ``046``/``047`` and
``056`` additionally drive a real PyMuPDF inside a subprocess (the in-process
``pymupdf`` is pdfspine's shim via ``conftest``); they skip when the child
cannot import a real PyMuPDF. The child interpreter is
``PDFSPINE_ORACLE_PYTHON`` when set, else the ``.venv-oracle`` next to the
repo when present (the same lookup as ``test_m4.py``), else this one.

All fixtures are hand-written PDF bytes (opened via ``stream=``) — no external
files (PRD §10).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys

import pytest

import pdfspine


# --- fixtures -------------------------------------------------------------

_BLANK_PDF = (
    b"%PDF-1.7\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n"
    b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n"
    b"trailer<</Root 1 0 R>>\n%%EOF"
)


def _blank() -> pdfspine.Document:
    return pdfspine.open(stream=_BLANK_PDF)


def _with_ab() -> tuple[pdfspine.Document, int, int]:
    """A blank doc with OCGs ``A`` (on) and ``B`` (off)."""
    doc = _blank()
    a = doc.add_ocg("A")
    b = doc.add_ocg("B", on=False)
    return doc, a, b


def _with_three_configs() -> tuple[pdfspine.Document, int, int]:
    """``A``/``B`` plus three ``/Configs`` entries (cfg1 on=[a], cfg2, cfg3)."""
    doc, a, b = _with_ab()
    doc.add_layer("cfg1", creator="me", on=[a])
    doc.add_layer("cfg2")
    doc.add_layer("cfg3", on=[b, 99999])  # 99999 is a bogus xref, silently dropped
    return doc, a, b


def _build_order_pdf(
    order_body: bytes, extra_d: bytes = b"", names=(b"A", b"B", b"C")
) -> bytes:
    """A PDF whose ``/OCProperties /D`` uses a hand-written ``/Order`` (and,
    optionally, extra ``/D`` keys such as ``/RBGroups``). OCGs live at xrefs 7…."""
    ocg_objs: dict[int, bytes] = {}
    refs = []
    for i, name in enumerate(names):
        xr = 7 + i
        ocg_objs[xr] = b"<</Type/OCG/Name(" + name + b")>>"
        refs.append(f"{xr} 0 R".encode())
    allrefs = b" ".join(refs)
    objs = {
        1: b"<</Type/Catalog/Pages 2 0 R/OCProperties 6 0 R>>",
        2: b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        3: b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>",
        6: b"<</OCGs["
        + allrefs
        + b"]/D<</ON["
        + allrefs
        + b"]/OFF[]/Order "
        + order_body
        + extra_d
        + b">>>>",
    }
    objs.update(ocg_objs)
    return _assemble(objs)


def _image_and_form_pdf() -> bytes:
    """A page referencing an image XObject (xref 4) and a form XObject (xref 5),
    neither carrying an ``/OC`` entry."""
    img = bytes([0, 0, 0] * 16)  # 4x4 black RGB
    objs = {
        1: b"<</Type/Catalog/Pages 2 0 R>>",
        2: b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        3: b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]"
        b"/Resources<</XObject<</Im1 4 0 R/Fm1 5 0 R>>>>>>",
        4: b"<</Type/XObject/Subtype/Image/Width 4/Height 4/ColorSpace/DeviceRGB"
        b"/BitsPerComponent 8/Length "
        + str(len(img)).encode()
        + b">>stream\n"
        + img
        + b"\nendstream",
        5: b"<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Length 3>>stream\n0 0\nendstream",
    }
    return _assemble(objs)


def _layered_pdf() -> bytes:
    """A hand-written layered page (the ``build_pdf`` from the render smoke probe).

    Xrefs: 7 = OCG ``A`` (on), 8 = OCG ``B`` (off), 9 = OCMD ``AllOn[A,B]``,
    10 = OCMD ``VE[/Not B]``, 11 = image XObject ``/OC B``, 12 = form XObject
    ``/OC A``. ``/D`` turns A on and B off, and a ``/Configs`` entry "only B"
    turns only B on. Text lives in ``/OC … BDC`` sections: AAAA under A, BBBB
    under B, CCCC under the AllOn OCMD, DDDD under the Not-B OCMD, EEEE with no
    ``/OC``; FFFF is inside the form XObject, and a blue rect is inside a
    hidden (``B``) section.
    """
    content = (
        b"/OC /MC0 BDC BT /F1 24 Tf 50 700 Td (AAAA) Tj ET EMC\n"
        b"/OC /MC1 BDC BT /F1 24 Tf 50 600 Td (BBBB) Tj ET EMC\n"
        b"/OC /MC2 BDC BT /F1 24 Tf 50 500 Td (CCCC) Tj ET EMC\n"
        b"/OC /MC3 BDC BT /F1 24 Tf 50 400 Td (DDDD) Tj ET EMC\n"
        b"BT /F1 24 Tf 50 300 Td (EEEE) Tj ET\n"
        b"q 100 0 0 100 300 600 cm /Im1 Do Q\n"
        b"q 1 0 0 1 300 300 cm /Fm1 Do Q\n"
        b"/OC /MC1 BDC q 0 0 1 rg 300 100 100 50 re f Q EMC\n"
    )
    form = b"BT /F1 24 Tf 0 0 Td (FFFF) Tj ET"
    img = bytes([0, 0, 0] * 16)  # 4x4 black RGB
    objs = {
        1: b"<</Type/Catalog/Pages 2 0 R/OCProperties 6 0 R>>",
        2: b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        3: b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Contents 4 0 R"
        b"/Resources<</Font<</F1 5 0 R>>"
        b"/Properties<</MC0 7 0 R/MC1 8 0 R/MC2 9 0 R/MC3 10 0 R>>"
        b"/XObject<</Im1 11 0 R/Fm1 12 0 R>>>>>>",
        4: b"<</Length "
        + str(len(content)).encode()
        + b">>stream\n"
        + content
        + b"\nendstream",
        5: b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>",
        6: b"<</OCGs[7 0 R 8 0 R]/D<</ON[7 0 R]/OFF[8 0 R]/Order[7 0 R 8 0 R]>>"
        b"/Configs[<</Name(only B)/BaseState/OFF/ON[8 0 R]>>]>>",
        7: b"<</Type/OCG/Name(A)>>",
        8: b"<</Type/OCG/Name(B)>>",
        9: b"<</Type/OCMD/OCGs[7 0 R 8 0 R]/P/AllOn>>",
        10: b"<</Type/OCMD/VE[/Not 8 0 R]>>",
        11: b"<</Type/XObject/Subtype/Image/Width 4/Height 4/ColorSpace/DeviceRGB"
        b"/BitsPerComponent 8/OC 8 0 R/Length "
        + str(len(img)).encode()
        + b">>stream\n"
        + img
        + b"\nendstream",
        12: b"<</Type/XObject/Subtype/Form/BBox[0 0 200 50]/OC 7 0 R"
        b"/Resources<</Font<</F1 5 0 R>>>>/Length "
        + str(len(form)).encode()
        + b">>stream\n"
        + form
        + b"\nendstream",
    }
    return _assemble(objs)


def _assemble(objs: dict[int, bytes]) -> bytes:
    """Serialises ``{xref: body}`` into a minimal classic-xref PDF."""
    out = bytearray(b"%PDF-1.7\n")
    offsets: dict[int, int] = {}
    for num in sorted(objs):
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + objs[num] + b"\nendobj\n"
    xref_pos = len(out)
    size = max(objs) + 1
    out += f"xref\n0 {size}\n".encode() + b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010d} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += (
        f"trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref_pos}\n%%EOF\n".encode()
    )
    return bytes(out)


# --- small helpers --------------------------------------------------------


def _ocprops_xref(doc: pdfspine.Document) -> int:
    ref = doc.xref_get_key(doc.pdf_catalog(), "OCProperties")
    return int(ref.split()[0])


def _on_states(doc: pdfspine.Document, *xrefs: int) -> dict[int, bool]:
    ocgs = doc.get_ocgs()
    return {x: ocgs[x]["on"] for x in xrefs}


def _ui_on(doc: pdfspine.Document) -> dict[str, bool]:
    return {u["text"]: u["on"] for u in doc.layer_ui_configs()}


def _text_words(doc: pdfspine.Document) -> list[str]:
    return doc[0].get_text().split()


def _nonwhite(doc: pdfspine.Document) -> int:
    pm = doc[0].get_pixmap()
    return sum(1 for i in range(0, len(pm.samples), pm.n) if pm.samples[i] != 255)


def _image_blocks(doc: pdfspine.Document) -> int:
    return sum(1 for blk in doc[0].get_text("dict")["blocks"] if blk["type"] == 1)


def _oracle_python() -> str:
    """The interpreter that runs the live-oracle children (real PyMuPDF must be
    importable there): ``PDFSPINE_ORACLE_PYTHON`` when set, else the
    ``.venv-oracle`` next to the repo when present, else the current one."""
    env = os.environ.get("PDFSPINE_ORACLE_PYTHON")
    if env:
        return env
    root = os.path.join(os.path.dirname(__file__), "..", "..", ".venv-oracle")
    for candidate in (
        os.path.join(root, "bin", "python"),
        os.path.join(root, "Scripts", "python.exe"),
    ):
        if os.path.exists(candidate):
            return candidate
    return sys.executable


_ORACLE_PYTHON = _oracle_python()


def _real_pymupdf_available() -> bool:
    probe = "import pymupdf, sys; sys.exit(0 if hasattr(pymupdf, 'mupdf') else 1)"
    return (
        subprocess.run([_ORACLE_PYTHON, "-c", probe], capture_output=True).returncode
        == 0
    )


def _run_child(code: str) -> dict:
    result = subprocess.run(
        [_ORACLE_PYTHON, "-c", code], capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


# === PYOCG-004..010 — get_layers / add_layer / switch_layer ================


def test_pyocg_004_fresh_doc_has_no_layers():
    # PyMuPDF 1.27.2 oracle: a fresh doc has no /Configs; negative/zero switch is a no-op.
    doc = _blank()
    assert doc.get_layers() == []
    assert doc.switch_layer(0) is None
    assert doc.switch_layer(-1) is None


def test_pyocg_005_switch_layer_out_of_range_no_configs():
    # PyMuPDF 1.27.2 oracle: with no /Configs, config >= 1 raises "bad layer number".
    doc = _blank()
    with pytest.raises(ValueError, match="bad layer number"):
        doc.switch_layer(1)


def test_pyocg_006_add_ocg_yields_no_configs():
    # PyMuPDF 1.27.2 oracle: OCGs alone are not layer configurations.
    doc, _a, _b = _with_ab()
    assert doc.get_layers() == []


def test_pyocg_007_add_layer_get_layers_roundtrip():
    # PyMuPDF 1.27.2 oracle (pdfspine returns this immediately, no reopen needed).
    doc, _a, _b = _with_three_configs()
    assert doc.get_layers() == [
        {"number": 0, "name": "cfg1", "creator": "me"},
        {"number": 1, "name": "cfg2", "creator": ""},
        {"number": 2, "name": "cfg3", "creator": ""},
    ]


def test_pyocg_008_configs_stored_as_direct_dicts():
    # PyMuPDF 1.27.2 oracle: /Configs holds inline dicts with Name/Creator/BaseState/ON.
    doc, a, _b = _with_three_configs()
    obj = doc.xref_object(_ocprops_xref(doc))
    assert "/Configs [<<" in obj  # direct dicts, not indirect "N 0 R" refs
    assert "/Name (cfg1)" in obj
    assert "/Creator (me)" in obj
    assert "/BaseState /OFF" in obj
    assert f"/ON [{a} 0 R]" in obj


@pytest.mark.parametrize(
    ("config", "a_on", "b_on"),
    [(0, True, False), (1, False, False), (2, False, True)],
)
def test_pyocg_009_switch_layer_selects_config_view(config, a_on, b_on):
    # PyMuPDF 1.27.2 oracle: switch_layer selects the config's ON/OFF as the view.
    doc, a, b = _with_three_configs()
    doc.switch_layer(config)
    assert _ui_on(doc) == {"A": a_on, "B": b_on}
    assert _on_states(doc, a, b) == {a: a_on, b: b_on}


def test_pyocg_010_switch_layer_out_of_range_raises():
    # PyMuPDF 1.27.2 oracle: past the last config raises "Illegal Layer config".
    doc, _a, _b = _with_three_configs()
    with pytest.raises(ValueError, match="Illegal Layer config"):
        doc.switch_layer(5)


def test_pyocg_011_switch_layer_is_in_memory_only():
    # PyMuPDF 1.27.2 oracle: switching leaves the persisted /D unchanged.
    doc, a, b = _with_three_configs()
    doc.switch_layer(2)  # A off, B on in the in-memory view
    assert _on_states(doc, a, b) == {a: False, b: True}
    reopened = pdfspine.open(stream=doc.tobytes())
    assert reopened.get_layers() == doc.get_layers()
    assert _on_states(reopened, a, b) == {a: True, b: False}  # /D default is preserved


def test_pyocg_012_switch_layer_as_default_rewrites_d():
    # PyMuPDF 1.27.2 oracle: as_default rewrites /D from the config and deletes /Configs.
    doc, a, _b = _with_three_configs()
    doc.switch_layer(0, as_default=True)
    reopened = pdfspine.open(stream=doc.tobytes())
    assert reopened.get_layers() == []
    ocp = _ocprops_xref(reopened)
    assert reopened.xref_get_key(ocp, "Configs") is None
    d_obj = reopened.xref_get_key(ocp, "D")
    assert "/BaseState /OFF" in d_obj
    assert "/Intent /View" in d_obj
    assert f"/ON [{a} 0 R]" in d_obj
    assert "/OFF [" not in d_obj  # no explicit /OFF key remains


# === PYOCG-013..016 — set_layer_ui_config ==================================


def test_pyocg_013_set_layer_ui_config_set_toggle_clear():
    # PyMuPDF 1.27.2 oracle: action 2 clears, 0 sets, 1 toggles a panel row.
    doc, a, b = _with_ab()
    assert _on_states(doc, a, b) == {a: True, b: False}
    doc.set_layer_ui_config(0, 2)  # clear A
    assert _ui_on(doc) == {"A": False, "B": False}
    assert _on_states(doc, a, b) == {a: False, b: False}
    doc.set_layer_ui_config(1, 0)  # set B
    assert _on_states(doc, a, b) == {a: False, b: True}
    doc.set_layer_ui_config(0, 1)  # toggle A on
    assert _on_states(doc, a, b) == {a: True, b: True}


def test_pyocg_014_set_layer_ui_config_by_row_text():
    # PyMuPDF 1.27.2 oracle: a str `number` addresses the row by its text.
    doc, a, b = _with_ab()
    doc.set_layer_ui_config("A", 1)  # toggle A (currently on) off
    assert _on_states(doc, a, b) == {a: False, b: False}


def test_pyocg_015_set_layer_ui_config_persisted_d_unchanged():
    # PyMuPDF 1.27.2 oracle: panel overrides are in-memory; /D stays put.
    doc, a, b = _with_ab()
    doc.set_layer_ui_config(0, 2)  # A off in the view only
    reopened = pdfspine.open(stream=doc.tobytes())
    assert _on_states(reopened, a, b) == {a: True, b: False}


@pytest.mark.parametrize(
    ("number", "match"),
    [(5, r"."), ("nosuch", r"bad OCG 'nosuch'\.")],
)
def test_pyocg_016_set_layer_ui_config_errors(number, match):
    # PyMuPDF 1.27.2 oracle: out-of-range index or unknown row text raises ValueError.
    doc, _a, _b = _with_ab()
    with pytest.raises(ValueError, match=match):
        doc.set_layer_ui_config(number, 0)


# === PYOCG-017..019 — layer_ui_configs parity ==============================


def test_pyocg_017_layer_ui_configs_nested_order_depth():
    # PyMuPDF 1.27.2 oracle: /Order [A [B C]] nests B,C one level under A.
    doc = pdfspine.open(stream=_build_order_pdf(b"[7 0 R [8 0 R 9 0 R]]"))
    rows = doc.layer_ui_configs()
    assert [(r["number"], r["text"], r["depth"], r["type"]) for r in rows] == [
        (0, "A", 0, "checkbox"),
        (1, "B", 1, "checkbox"),
        (2, "C", 1, "checkbox"),
    ]


def test_pyocg_018_layer_ui_configs_label_group_locked():
    # PyMuPDF 1.27.2 oracle: a leading string in a nested array is a locked label row.
    doc = pdfspine.open(
        stream=_build_order_pdf(b"[[(Group1) 7 0 R 8 0 R]]", names=(b"A", b"B"))
    )
    rows = doc.layer_ui_configs()
    assert rows[0] == {
        "number": 0,
        "text": "Group1",
        "depth": 0,
        "type": "label",
        "on": False,
        "locked": True,
    }
    assert rows[0]["on"] == 0 and rows[0]["locked"] == 1  # PyMuPDF ints compare equal
    assert [(r["number"], r["text"], r["depth"]) for r in rows[1:]] == [
        (1, "A", 1),
        (2, "B", 1),
    ]


def test_pyocg_019_layer_ui_configs_rbgroups_radiobox():
    # PyMuPDF 1.27.2 oracle: /RBGroups members render as "radiobox" rows.
    doc = pdfspine.open(
        stream=_build_order_pdf(
            b"[7 0 R 8 0 R]", extra_d=b"/RBGroups[[7 0 R 8 0 R]]", names=(b"A", b"B")
        )
    )
    rows = doc.layer_ui_configs()
    assert [(r["number"], r["text"], r["type"]) for r in rows] == [
        (0, "A", "radiobox"),
        (1, "B", "radiobox"),
    ]


# === PYOCG-020..023 — get_oc / set_oc ======================================


@pytest.mark.parametrize("xref", [4, 5])  # image XObject, form XObject
def test_pyocg_020_get_oc_without_oc_is_zero(xref):
    # PyMuPDF 1.27.2 oracle: an XObject without /OC yields 0.
    doc = pdfspine.open(stream=_image_and_form_pdf())
    assert doc.get_oc(xref) == 0


def test_pyocg_021_get_oc_after_set_oc():
    # PyMuPDF 1.27.2 oracle: set_oc binds /OC to an OCG then an OCMD; get_ocgs stays OCG-only.
    doc = pdfspine.open(stream=_image_and_form_pdf())
    a = doc.add_ocg("A")
    doc.set_oc(4, a)
    assert doc.get_oc(4) == a
    ocmd = doc.set_ocmd(ocgs=[a], policy="AnyOn")
    doc.set_oc(4, ocmd)
    assert doc.get_oc(4) == ocmd
    assert sorted(doc.get_ocgs().keys()) == [a]  # the OCMD is not an OCG


@pytest.mark.parametrize("xref", [1, 3, 7])  # catalog, page, OCG
def test_pyocg_022_get_oc_bad_object_type(xref):
    # PyMuPDF 1.27.2 oracle: a non-XObject xref raises "bad object type at xref N".
    doc = pdfspine.open(stream=_layered_pdf())
    with pytest.raises(ValueError, match=f"bad object type at xref {xref}"):
        doc.get_oc(xref)


def test_pyocg_023_get_oc_bad_xref():
    # PyMuPDF 1.27.2 oracle: xref 0 or >= xref_length raises "bad xref".
    doc = pdfspine.open(stream=_layered_pdf())
    with pytest.raises(ValueError, match="bad xref"):
        doc.get_oc(0)
    with pytest.raises(ValueError, match="bad xref"):
        doc.get_oc(doc.xref_length())


# === PYOCG-024..029 — set_ocmd / get_ocmd ==================================


@pytest.mark.parametrize(
    ("policy", "expected"),
    [("AnyOn", "AnyOn"), ("alloff", "AllOff"), (None, None)],
)
def test_pyocg_024_set_ocmd_policy_roundtrip(policy, expected):
    # PyMuPDF 1.27.2 oracle: policy normalises to AnyOn/AllOn/AnyOff/AllOff (or None).
    doc, a, b = _with_ab()
    xref = doc.set_ocmd(ocgs=[a, b], policy=policy)
    assert doc.get_ocmd(xref) == {
        "xref": xref,
        "ocgs": [a, b],
        "policy": expected,
        "ve": None,
    }
    assert sorted(doc.get_ocgs().keys()) == [a, b]  # OCMD does not register as an OCG


def test_pyocg_025_set_ocmd_ve_roundtrip():
    # PyMuPDF 1.27.2 oracle: visibility expressions round-trip as nested lists.
    doc, a, b = _with_ab()
    for ve in (["not", a], ["and", a, ["not", b]], ["or", a, b]):
        xref = doc.set_ocmd(ve=ve)
        assert doc.get_ocmd(xref) == {
            "xref": xref,
            "ocgs": None,
            "policy": None,
            "ve": ve,
        }


def test_pyocg_026_set_ocmd_all_fields():
    # PyMuPDF 1.27.2 oracle: ocgs + policy + ve can coexist in one OCMD.
    doc, a, b = _with_ab()
    xref = doc.set_ocmd(ocgs=[a], policy="AnyOff", ve=["or", a, b])
    assert doc.get_ocmd(xref) == {
        "xref": xref,
        "ocgs": [a],
        "policy": "AnyOff",
        "ve": ["or", a, b],
    }


def test_pyocg_027_set_ocmd_replace_and_ignore():
    # PyMuPDF 1.27.2 oracle: a replace rewrites the whole dict; a bare int ocgs is ignored.
    doc, a, b = _with_ab()
    xref = doc.set_ocmd(ocgs=[a, b], policy="AnyOn")
    doc.set_ocmd(xref=xref, ocgs=[b])  # drops policy/ve
    assert doc.get_ocmd(xref) == {"xref": xref, "ocgs": [b], "policy": None, "ve": None}
    doc.set_ocmd(xref=xref)  # empty replace clears everything
    assert doc.get_ocmd(xref) == {
        "xref": xref,
        "ocgs": None,
        "policy": None,
        "ve": None,
    }
    ignored = doc.set_ocmd(ocgs=5)  # int, not a list -> silently ignored
    assert doc.get_ocmd(ignored)["ocgs"] is None


@pytest.mark.parametrize("kind", ["policy", "ocgs", "ve_not3", "ve_xor", "ve_not_bad"])
def test_pyocg_028_set_ocmd_errors(kind):
    # PyMuPDF 1.27.2 oracle: malformed policy / OCGs / ve raise ValueError (the ve
    # messages quote the resolved OCG xrefs).
    doc, a, b = _with_ab()
    cases = {
        "policy": ({"policy": "bogus"}, "bad policy: bogus"),
        "ocgs": ({"ocgs": [999999]}, r"bad OCGs: \{999999\}"),
        "ve_not3": (
            {"ve": ["not", a, b]},
            re.escape(f"bad 've' format: ['not', {a}, {b}]"),
        ),
        "ve_xor": ({"ve": ["xor", a]}, "bad operand: xor"),
        "ve_not_bad": ({"ve": ["not", 999999]}, "bad OCG 999999"),
    }
    kwargs, match = cases[kind]
    with pytest.raises(ValueError, match=match):
        doc.set_ocmd(**kwargs)


def test_pyocg_028b_set_ocmd_replace_non_ocmd():
    # PyMuPDF 1.27.2 oracle: replacing a non-OCMD xref raises "bad xref or not an OCMD".
    doc, _a, _b = _with_ab()
    with pytest.raises(ValueError, match="bad xref or not an OCMD"):
        doc.set_ocmd(xref=doc.pdf_catalog())


@pytest.mark.parametrize("bad", ["catalog", "ocg"])
def test_pyocg_029_get_ocmd_bad_object_type(bad):
    # PyMuPDF 1.27.2 oracle: get_ocmd on a non-OCMD object raises "bad object type".
    doc, a, _b = _with_ab()
    xref = doc.pdf_catalog() if bad == "catalog" else a
    with pytest.raises(ValueError, match="bad object type"):
        doc.get_ocmd(xref)


def test_pyocg_029b_get_ocmd_bad_xref():
    # PyMuPDF 1.27.2 oracle: an out-of-range xref raises "bad xref".
    doc, _a, _b = _with_ab()
    with pytest.raises(ValueError, match="bad xref"):
        doc.get_ocmd(999999)


# === PYOCG-030 — closed document ===========================================


@pytest.mark.parametrize(
    ("method", "args"),
    [
        ("get_layers", ()),
        ("add_layer", ("x",)),
        ("switch_layer", (0,)),
        ("set_layer_ui_config", (0,)),
        ("get_ocmd", (1,)),
        ("set_ocmd", ()),
    ],
)
def test_pyocg_030_closed_document_errors(method, args):
    # PyMuPDF 1.27.2 oracle: layer/OCMD methods on a closed doc raise "document closed".
    doc = _blank()
    doc.close()
    with pytest.raises(ValueError, match="document closed"):
        getattr(doc, method)(*args)


def test_pyocg_030b_get_oc_closed_document():
    # PyMuPDF 1.27.2 oracle: get_oc has its own wording (sic) on a closed doc.
    doc = _blank()
    doc.close()
    with pytest.raises(ValueError, match="document close or encrypted"):
        doc.get_oc(1)


# === PYOCG-031..036 — hidden optional content (render / extract) ===========


def test_pyocg_031_hidden_default_view():
    # PyMuPDF 1.27.2 oracle: /D (A on, B off) hides BBBB/CCCC, the /OC B image and the rect.
    doc = pdfspine.open(stream=_layered_pdf())
    assert _text_words(doc) == ["AAAA", "DDDD", "EEEE", "FFFF"]
    assert _image_blocks(doc) == 0
    assert doc[0].get_drawings() == []


def test_pyocg_032_all_layers_visible():
    # PyMuPDF 1.27.2 oracle: turning B on reveals every word, the image and the rect.
    doc = pdfspine.open(stream=_layered_pdf())
    hidden_pixels = _nonwhite(doc)
    doc.set_layer_ui_config(1, 0)  # B on
    assert _text_words(doc) == ["AAAA", "BBBB", "CCCC", "DDDD", "EEEE", "FFFF"]
    assert _image_blocks(doc) == 1
    assert len(doc[0].get_drawings()) == 1
    assert _nonwhite(doc) > hidden_pixels  # relative: more paint once B is visible


def test_pyocg_033_layer_a_off():
    # PyMuPDF 1.27.2 oracle: with A off and B on only the B-gated BBBB and the
    # un-gated EEEE survive (AAAA/FFFF need A; CCCC needs AllOn; DDDD needs Not-B).
    doc = pdfspine.open(stream=_layered_pdf())
    doc.set_layer_ui_config(1, 0)  # B on
    doc.set_layer_ui_config(0, 2)  # A off
    assert _text_words(doc) == ["BBBB", "EEEE"]


def test_pyocg_034_switch_config_only_b():
    # PyMuPDF 1.27.2 oracle: config "only B" shows BBBB/EEEE; a negative switch is a no-op.
    doc = pdfspine.open(stream=_layered_pdf())
    doc.switch_layer(0)  # config "only B" (BaseState OFF, ON=[B])
    assert _text_words(doc) == ["BBBB", "EEEE"]
    doc.switch_layer(-1)  # no-op
    assert _text_words(doc) == ["BBBB", "EEEE"]


def test_pyocg_035_set_layer_direct():
    # PyMuPDF 1.27.2 oracle: set_layer writes /D (B on, A off) and resets the view.
    doc = pdfspine.open(stream=_layered_pdf())
    doc.set_layer(-1, on=[8], off=[7])
    assert _text_words(doc) == ["BBBB", "EEEE"]


def test_pyocg_036_layered_get_oc_and_get_ocmd():
    # PyMuPDF 1.27.2 oracle: /OC bindings and OCMD definitions read back on the layered file.
    doc = pdfspine.open(stream=_layered_pdf())
    assert doc.get_oc(11) == 8  # image XObject bound to OCG B
    assert doc.get_oc(12) == 7  # form XObject bound to OCG A
    assert doc.get_ocmd(9) == {"xref": 9, "ocgs": [7, 8], "policy": "AllOn", "ve": None}
    assert doc.get_ocmd(10) == {
        "xref": 10,
        "ocgs": None,
        "policy": None,
        "ve": ["not", 8],
    }


# === PYOCG-037..038 — live PyMuPDF 1.27.2 oracle (subprocess) ==============


def test_pyocg_037_live_oracle_forward(tmp_path):
    """pdfspine authors a layered/OCMD file; real PyMuPDF must read it identically."""
    if not _real_pymupdf_available():
        pytest.skip("no real PyMuPDF in the subprocess")
    doc = _blank()
    a = doc.add_ocg("A")
    b = doc.add_ocg("B", on=False)
    doc.add_layer("cfg1", creator="me", on=[a])
    doc.add_layer("cfg2")
    doc.add_layer("cfg3", on=[b])
    o1 = doc.set_ocmd(ocgs=[a, b], policy="AnyOff")
    o2 = doc.set_ocmd(ocgs=[a, b], policy="AllOff")
    o3 = doc.set_ocmd(ve=["not", a])
    path = tmp_path / "forward.pdf"
    doc.save(path)

    child = (
        "import json, pymupdf\n"
        "d = pymupdf.open(__PATH__)\n"
        "out = {}\n"
        "out['get_layers'] = d.get_layers()\n"
        "out['get_layer'] = [d.get_layer(i) for i in range(3)]\n"
        "out['ocmd'] = [d.get_ocmd(__O1__), d.get_ocmd(__O2__), d.get_ocmd(__O3__)]\n"
        "out['ui'] = [(u['text'], int(u['on'])) for u in d.layer_ui_configs()]\n"
        "print(json.dumps(out))\n"
    )
    code = (
        child.replace("__PATH__", repr(str(path)))
        .replace("__O1__", str(o1))
        .replace("__O2__", str(o2))
        .replace("__O3__", str(o3))
    )
    real = _run_child(code)

    ps = pdfspine.open(path)
    # parity: real PyMuPDF agrees with pdfspine on the same file
    assert real["get_layers"] == ps.get_layers()
    assert real["ocmd"] == [ps.get_ocmd(o1), ps.get_ocmd(o2), ps.get_ocmd(o3)]
    assert [tuple(t) for t in real["ui"]] == [
        (u["text"], int(u["on"])) for u in ps.layer_ui_configs()
    ]
    # hardcoded PyMuPDF 1.27.2 get_layer(n) expectations
    assert real["get_layer"] == [
        {"on": [a], "basestate": "OFF"},
        {"basestate": "OFF"},
        {"on": [b], "basestate": "OFF"},
    ]


def test_pyocg_038_live_oracle_reverse(tmp_path):
    """Real PyMuPDF authors /OC BDC sections; pdfspine must render/extract them identically."""
    if not _real_pymupdf_available():
        pytest.skip("no real PyMuPDF in the subprocess")
    path = tmp_path / "reverse.pdf"
    author = (
        "import json, pymupdf\n"
        "d = pymupdf.open()\n"
        "page = d.new_page()\n"
        "a = d.add_ocg('A')\n"
        "b = d.add_ocg('B', on=False)\n"
        "page.insert_text((50, 100), 'AAAA', oc=a)\n"
        "page.insert_text((50, 200), 'BBBB', oc=b)\n"
        "d.add_layer('only B', on=[b])\n"
        "x = d.set_ocmd(ocgs=[a, b], policy='AnyOff')\n"
        "d.save(__PATH__)\n"
        "d2 = pymupdf.open(__PATH__)\n"
        "out = {}\n"
        "out['a'] = a\n"
        "out['b'] = b\n"
        "out['x'] = x\n"
        "out['get_layers'] = d2.get_layers()\n"
        "out['get_ocmd'] = d2.get_ocmd(x)\n"
        "out['default'] = d2[0].get_text().split()\n"
        "d2.set_layer_ui_config(1, 0)\n"
        "out['ui10'] = d2[0].get_text().split()\n"
        "d2.switch_layer(0)\n"
        "out['switch0'] = d2[0].get_text().split()\n"
        "print(json.dumps(out))\n"
    ).replace("__PATH__", repr(str(path)))
    real = _run_child(author)

    # hardcoded PyMuPDF 1.27.2 expectations for the three view states
    assert real["default"] == ["AAAA"]
    assert real["ui10"] == ["AAAA", "BBBB"]
    assert real["switch0"] == ["BBBB"]

    # pdfspine reads the very same file (identical xrefs) and must match
    doc = pdfspine.open(path)
    assert doc.get_layers() == real["get_layers"]
    assert doc.get_ocmd(real["x"]) == real["get_ocmd"]
    assert _text_words(doc) == real["default"]
    doc.set_layer_ui_config(1, 0)
    assert _text_words(doc) == real["ui10"]
    doc.switch_layer(0)
    assert _text_words(doc) == real["switch0"]


# === PYOCG-039..045 — `oc=` writers ========================================

# The exact chunk PyMuPDF-style writers emit: `/OC /MCn BDC` right after the
# opening `q`, `EMC` right before the closing `Q`.
_TEXT_OC_RE = re.compile(
    rb"\Aq\n/OC /(MC\d+) BDC\nBT\n(?:.*\n)*?ET\nEMC\nQ\n\Z", re.DOTALL
)


def _properties(doc: pdfspine.Document) -> dict[str, int]:
    """``{key: xref}`` of the page's ``/Resources /Properties`` dict."""
    obj = doc.xref_object(doc[0].xref)
    m = re.search(r"/Properties\s*<<(.*?)>>", obj)
    if not m:
        return {}
    return {k: int(x) for k, x in re.findall(r"/(\w+)\s+(\d+) 0 R", m.group(1))}


def _reopen(doc: pdfspine.Document, path) -> pdfspine.Document:
    doc.save(path)
    return pdfspine.open(path)


def _bdc_names(doc: pdfspine.Document) -> list[str]:
    return re.findall(r"/OC /(MC\d+) BDC", doc[0].read_contents().decode("latin-1"))


def test_pyocg_039_insert_text_oc_marked_content(tmp_path):
    # PyMuPDF 1.27.2 oracle: `q / BDC / BT … ET / EMC / Q`, /Properties key MC0
    # reused for the same OCG, MC1 for the next one; OFF hides the text.
    doc, a, b = _with_ab()
    page = doc[0]
    page.insert_text((50, 100), "AAAA", oc=a)
    body = page.read_contents()
    assert _TEXT_OC_RE.match(body), body
    assert body == (
        b"q\n/OC /MC0 BDC\nBT\n/F0 11 Tf\n0 0 0 rg\n13.2 TL\n"
        b"1 0 0 1 50 100 Tm\n(AAAA) Tj\nET\nEMC\nQ\n"
    )
    assert _properties(doc) == {"MC0": a}

    page.insert_text((50, 120), "AAA2", oc=a)
    page.insert_text((50, 140), "BBBB", oc=b)
    assert _bdc_names(doc) == ["MC0", "MC0", "MC1"]
    assert _properties(doc) == {"MC0": a, "MC1": b}

    re_doc = _reopen(doc, tmp_path / "text_oc.pdf")
    assert _properties(re_doc) == {"MC0": a, "MC1": b}
    assert _text_words(re_doc) == ["AAAA", "AAA2"]
    re_doc.set_layer_ui_config(1, 0)  # B ON
    assert _text_words(re_doc) == ["AAAA", "AAA2", "BBBB"]


def test_pyocg_040_insert_textbox_and_shape_text_oc(tmp_path):
    # PyMuPDF 1.27.2 oracle: insert_textbox / Shape.insert_text / Shape.insert_textbox
    # wrap the text object the same way (oc=0 leaves the stream untouched).
    doc, a, b = _with_ab()
    page = doc[0]
    page.insert_textbox((10, 10, 190, 60), "BOXED", oc=b)
    assert _TEXT_OC_RE.match(page.read_contents())
    assert _bdc_names(doc) == ["MC0"]

    shape = page.new_shape()
    shape.insert_text((20, 100), "SHAPETEXT", oc=a)
    shape.insert_textbox((10, 120, 190, 180), "SHAPEBOX", oc=b)
    shape.commit()
    assert _bdc_names(doc) == ["MC0", "MC1", "MC0"]
    assert _properties(doc) == {"MC0": b, "MC1": a}

    page.insert_text((20, 190), "PLAIN")  # oc=0 (default): no marked content
    assert _bdc_names(doc) == ["MC0", "MC1", "MC0"]
    assert re.search(
        rb"q\nBT\n/F\d+ 11 Tf\n0 0 0 rg\n13.2 TL\n1 0 0 1 20 10 Tm\n\(PLAIN\) Tj\nET\nQ\n\Z",
        page.read_contents(),
    )

    re_doc = _reopen(doc, tmp_path / "textbox_oc.pdf")
    assert _text_words(re_doc) == ["SHAPETEXT", "PLAIN"]


def test_pyocg_041_insert_image_oc_sets_xobject_oc(tmp_path):
    # PyMuPDF 1.27.2 oracle: no BDC — `/OC` lands on the image XObject (get_oc reads it).
    doc, a, b = _with_ab()
    page = doc[0]
    page.insert_image(
        (10, 10, 60, 60), stream=bytes([0, 0, 0] * 4), width=2, height=2, oc=b
    )
    xref = page.get_images()[0][0]
    assert doc.get_oc(xref) == b
    assert doc.xref_get_key(xref, "OC") == f"{b} 0 R"
    assert b"BDC" not in page.read_contents()
    assert _properties(doc) == {}

    re_doc = _reopen(doc, tmp_path / "image_oc.pdf")
    assert re_doc.get_oc(xref) == b
    assert _image_blocks(re_doc) == 0  # B is OFF
    re_doc.set_layer_ui_config(1, 0)
    assert _image_blocks(re_doc) == 1


def test_pyocg_042_shape_finish_and_draw_oc(tmp_path):
    # PyMuPDF 1.27.2 oracle: every finish(oc=) block is `q / BDC / … / EMC / Q`;
    # page.draw_*(oc=) one-shots take the same path; hidden drawings vanish.
    doc, a, b = _with_ab()
    page = doc[0]
    shape = page.new_shape()
    shape.draw_rect((10, 10, 50, 50))
    shape.finish(color=None, fill=(1, 0, 0), oc=b)
    shape.draw_line((0, 0), (100, 100))
    shape.finish(color=(0, 0, 1), width=2, oc=a)
    shape.draw_line((0, 100), (100, 0))
    shape.finish(color=(0, 0, 1))
    shape.commit()
    assert page.read_contents() == (
        b"q\n/OC /MC0 BDC\n1 w\n1 0 0 rg\n10 150 40 40 re\nf\nEMC\nQ\n"
        b"q\n/OC /MC1 BDC\n2 w\n0 0 1 RG\n0 200 m\n100 100 l\nS\nEMC\nQ\n"
        b"q\n1 w\n0 0 1 RG\n0 100 m\n100 200 l\nS\nQ\n"
    )
    assert _properties(doc) == {"MC0": b, "MC1": a}

    page.draw_rect((60, 60, 90, 90), color=(0, 1, 0), fill=(0, 1, 0), oc=b)
    page.draw_circle((100, 100), 5, color=(0, 1, 0), oc=a)
    page.draw_line((0, 0), (10, 10), oc=b)
    assert _bdc_names(doc) == ["MC0", "MC1", "MC0", "MC1", "MC0"]
    assert page.read_contents().count(b"EMC") == 5

    re_doc = _reopen(doc, tmp_path / "shape_oc.pdf")
    # B OFF hides the red rect, the green rect and the last line: 3 remain.
    assert len(re_doc[0].get_drawings()) == 3
    re_doc.set_layer_ui_config(1, 0)
    assert len(re_doc[0].get_drawings()) == 6


def test_pyocg_043_bad_oc_raises():
    # PyMuPDF 1.27.2 oracle: not an OCG/OCMD → ValueError("bad optional content: 'oc'"),
    # nonexistent xref → RuntimeError("bad xref"); nothing is written on failure.
    doc, a, _b = _with_ab()
    page = doc[0]
    src = _blank()
    bad = page.xref  # a /Type /Page dictionary
    cases = [
        lambda oc: page.insert_text((10, 10), "x", oc=oc),
        lambda oc: page.insert_textbox((0, 0, 100, 100), "x", oc=oc),
        lambda oc: page.insert_image(
            (0, 0, 10, 10), stream=bytes([0, 0, 0]), width=1, height=1, oc=oc
        ),
        lambda oc: page.show_pdf_page((0, 0, 50, 50), src, 0, oc=oc),
        lambda oc: page.draw_rect((0, 0, 10, 10), oc=oc),
    ]
    for call in cases:
        with pytest.raises(ValueError, match="bad optional content: 'oc'"):
            call(bad)
        with pytest.raises(RuntimeError, match="bad xref"):
            call(99999)
    shape = page.new_shape()
    shape.draw_line((0, 0), (1, 1))
    shape.finish(oc=bad)  # validated at commit, like PyMuPDF's deferred write
    with pytest.raises(ValueError, match="bad optional content: 'oc'"):
        shape.commit()
    assert page.read_contents() == b""
    assert _properties(doc) == {}
    assert page.get_images() == []

    # An OCMD is accepted like an OCG.
    ocmd = doc.set_ocmd(ocgs=[a], policy="AnyOn")
    page.insert_text((10, 10), "OCMD", oc=ocmd)
    assert _properties(doc) == {"MC0": ocmd}


def test_pyocg_044_show_pdf_page_oc(tmp_path):
    # PyMuPDF 1.27.2 oracle: `/OC` on the form XObject, no BDC; OFF hides the form.
    src = _blank()
    src[0].insert_text((20, 40), "STAMP")
    doc, a, b = _with_ab()
    page = doc[0]
    name = page.show_pdf_page((0, 0, 200, 200), src, 0, oc=b)
    assert name == "Fm0"
    xref = next(x[0] for x in page.get_xobjects() if x[1] == name)
    assert doc.get_oc(xref) == b
    assert b"BDC" not in page.read_contents()
    assert _properties(doc) == {}

    re_doc = _reopen(doc, tmp_path / "form_oc.pdf")
    assert re_doc.get_oc(xref) == b
    assert _text_words(re_doc) == []
    re_doc.set_layer_ui_config(1, 0)
    assert _text_words(re_doc) == ["STAMP"]


def test_pyocg_045_textwriter_write_text_oc(tmp_path):
    # PyMuPDF 1.27.2 oracle: TextWriter.write_text(oc=) / page.write_text(oc=) wrap
    # every segment; the same OCG reuses /MC0.
    doc, a, b = _with_ab()
    page = doc[0]
    tw = pdfspine.TextWriter(page.rect)
    tw.append((20, 40), "ONE")
    tw.append((20, 60), "TWO")
    tw.write_text(page, oc=b)
    assert _bdc_names(doc) == ["MC0", "MC0"]
    assert _properties(doc) == {"MC0": b}

    tw2 = pdfspine.TextWriter(page.rect)
    tw2.append((20, 80), "THREE")
    page.write_text(writers=tw2, oc=a)  # fast path: single writer, no rect/rotate
    assert _bdc_names(doc) == ["MC0", "MC0", "MC1"]
    assert _properties(doc) == {"MC0": b, "MC1": a}

    re_doc = _reopen(doc, tmp_path / "tw_oc.pdf")
    assert _text_words(re_doc) == ["THREE"]
    re_doc.set_layer_ui_config(1, 0)
    assert _text_words(re_doc) == ["ONE", "TWO", "THREE"]


# === PYOCG-046..047 — live PyMuPDF oracle for the `oc=` writers ============


def _oc_blocks(body: bytes) -> list[bytes]:
    """Every ``q`` / ``/OC /MCn BDC`` … ``EMC`` / ``Q`` chunk, in stream order."""
    return re.findall(rb"q\n/OC /MC\d+ BDC\n.*?\nEMC\nQ\n", body, re.DOTALL)


def _skeleton(body: bytes) -> list[bytes]:
    """The structural operators of a content stream (q / BDC / BT / ET / EMC / Q)."""
    keep = (b"q", b"Q", b"BT", b"ET", b"EMC")
    out = []
    for line in body.split(b"\n"):
        line = line.strip()
        if line in keep or line.endswith(b" BDC"):
            out.append(line)
    return out


def test_pyocg_046_live_oracle_oc_forward(tmp_path):
    """pdfspine writes with ``oc=``; real PyMuPDF must hide / show the same content
    and emit the same marked-content skeleton for the same call."""
    if not _real_pymupdf_available():
        pytest.skip("no real PyMuPDF in the subprocess")
    doc, a, b = _with_ab()
    page = doc[0]
    page.insert_text((50, 100), "AAAA", oc=a)
    page.insert_text((50, 120), "BBBB", oc=b)
    page.insert_textbox((10, 130, 190, 160), "BOXB", oc=b)
    page.insert_image(
        (10, 10, 60, 60), stream=bytes([0, 0, 0] * 4), width=2, height=2, oc=b
    )
    img_xref = page.get_images()[0][0]
    shape = page.new_shape()
    shape.draw_rect((100, 100, 150, 150))
    shape.finish(fill=(1, 0, 0), oc=b)
    shape.commit()
    path = tmp_path / "forward_oc.pdf"
    doc.save(path)
    ours = page.read_contents()

    child = (
        "import json, pymupdf\n"
        "d = pymupdf.open(__PATH__)\n"
        "out = {}\n"
        "p = d[0]\n"
        "out['default'] = p.get_text().split()\n"
        "out['drawings'] = len(p.get_drawings())\n"
        "out['images'] = len(p.get_image_info())\n"
        "out['get_oc'] = d.get_oc(__IMG__)\n"
        # off=[] is required: PyMuPDF's set_layer leaves /OFF untouched when
        # `off` is None, and MuPDF lets /OFF win over /ON.
        "d.set_layer(-1, on=[__A__, __B__], off=[])\n"
        "d.save(__PATH2__)\n"
        "d = pymupdf.open(__PATH2__)\n"
        "p = d[0]\n"
        "out['all_on'] = p.get_text().split()\n"
        "out['drawings_on'] = len(p.get_drawings())\n"
        "out['images_on'] = len(p.get_image_info())\n"
        # The same writer calls on a fresh PyMuPDF doc: the marked-content
        # skeleton (q / BDC / BT … ET / EMC / Q) must match pdfspine's.
        "e = pymupdf.open()\n"
        "ep = e.new_page(width=200, height=200)\n"
        "ea = e.add_ocg('A')\n"
        "eb = e.add_ocg('B', on=False)\n"
        "ep.insert_text((50, 100), 'AAAA', oc=ea)\n"
        "ep.insert_text((50, 120), 'BBBB', oc=eb)\n"
        "ep.insert_textbox((10, 130, 190, 160), 'BOXB', oc=eb)\n"
        "out['skeleton'] = ep.read_contents().decode('latin-1')\n"
        "out['props'] = e.xref_get_key(ep.xref, 'Resources/Properties')\n"
        "print(json.dumps(out))\n"
    )
    code = (
        child.replace("__PATH__", repr(str(path)))
        .replace("__PATH2__", repr(str(tmp_path / "forward_oc_on.pdf")))
        .replace("__IMG__", str(img_xref))
        .replace("__A__", str(a))
        .replace("__B__", str(b))
    )
    real = _run_child(code)

    assert real["default"] == ["AAAA"]
    assert real["drawings"] == 0
    assert real["images"] == 0
    assert real["get_oc"] == b
    assert real["all_on"] == ["AAAA", "BBBB", "BOXB"]
    assert real["drawings_on"] == 1
    assert real["images_on"] == 1
    # pdfspine agrees with itself on the same file.
    ps = pdfspine.open(path)
    assert _text_words(ps) == real["default"]
    assert len(ps[0].get_drawings()) == real["drawings"]
    assert _image_blocks(ps) == real["images"]
    # Same skeleton as PyMuPDF's own writers, and the same /MCn key reuse.
    # The three text chunks come first; the image chunk (`q` + `… cm` + `Do`)
    # and the shape chunk follow, so cut at the first `q` opening a `cm` line.
    # pdfspine re-wraps the prior content in `q` / `Q` on every insert, so the
    # comparison is per marked-content block rather than over the whole stream.
    image_start = re.search(rb"^q\n[-\d. ]+ cm\n", ours, re.MULTILINE)
    assert image_start is not None
    text_part = ours[: image_start.start()]
    ours_blocks = _oc_blocks(text_part)
    real_blocks = _oc_blocks(real["skeleton"].encode("latin-1"))
    assert len(ours_blocks) == len(real_blocks) == 3
    assert [_skeleton(x) for x in ours_blocks] == [_skeleton(x) for x in real_blocks]
    assert re.findall(r"/OC /(MC\d+) BDC", real["skeleton"]) == ["MC0", "MC1", "MC1"]
    assert _bdc_names(doc) == ["MC0", "MC1", "MC1", "MC1"]  # + the shape block


def test_pyocg_047_live_oracle_oc_reverse(tmp_path):
    """Real PyMuPDF writes with ``oc=`` (text, image, form, shape); pdfspine reads
    the bindings and the hidden state identically."""
    if not _real_pymupdf_available():
        pytest.skip("no real PyMuPDF in the subprocess")
    path = tmp_path / "reverse_oc.pdf"
    author = (
        "import json, pymupdf\n"
        "d = pymupdf.open()\n"
        "p = d.new_page(width=200, height=200)\n"
        "a = d.add_ocg('A')\n"
        "b = d.add_ocg('B', on=False)\n"
        "p.insert_text((50, 100), 'AAAA', oc=a)\n"
        "p.insert_text((50, 120), 'BBBB', oc=b)\n"
        "pm = pymupdf.Pixmap(pymupdf.csRGB, pymupdf.IRect(0, 0, 4, 4), False)\n"
        "pm.clear_with(0)\n"
        "p.insert_image((10, 10, 60, 60), pixmap=pm, oc=b)\n"
        "s = pymupdf.open()\n"
        "sp = s.new_page(width=100, height=100)\n"
        "sp.insert_text((20, 40), 'STAMP')\n"
        "p.show_pdf_page(pymupdf.Rect(100, 0, 200, 100), s, 0, oc=b)\n"
        "sh = p.new_shape()\n"
        "sh.draw_rect((100, 100, 150, 150))\n"
        "sh.finish(fill=(1, 0, 0), oc=b)\n"
        "sh.draw_rect((150, 150, 190, 190))\n"
        "sh.finish(fill=(0, 1, 0), oc=a)\n"
        "sh.commit()\n"
        "d.save(__PATH__)\n"
        "d2 = pymupdf.open(__PATH__)\n"
        "out = {'a': a, 'b': b}\n"
        "p2 = d2[0]\n"
        "out['default'] = p2.get_text().split()\n"
        "out['drawings'] = len(p2.get_drawings())\n"
        "out['images'] = len(p2.get_image_info())\n"
        # page-level XObjects only (invoker 0): show_pdf_page also nests a
        # 'fullpage' form inside the placed one.
        "out['oc'] = {x[1]: d2.get_oc(x[0]) for x in p2.get_xobjects() if x[2] == 0}\n"
        "out['oc'].update({x[7]: d2.get_oc(x[0]) for x in p2.get_images()})\n"
        "d2.set_layer_ui_config(1, 0)\n"
        "out['ui10'] = p2.get_text().split()\n"
        "out['drawings_on'] = len(p2.get_drawings())\n"
        "out['images_on'] = len(p2.get_image_info())\n"
        "print(json.dumps(out))\n"
    ).replace("__PATH__", repr(str(path)))
    real = _run_child(author)

    # hardcoded PyMuPDF expectations
    assert real["default"] == ["AAAA"]
    assert real["drawings"] == 1
    assert real["images"] == 0
    assert real["ui10"] == ["AAAA", "BBBB", "STAMP"]
    assert real["drawings_on"] == 2
    assert real["images_on"] == 1

    doc = pdfspine.open(path)
    page = doc[0]
    ocs = {x[1]: doc.get_oc(x[0]) for x in page.get_xobjects()}
    ocs.update({x[7]: doc.get_oc(x[0]) for x in page.get_images()})
    assert ocs == real["oc"]
    assert set(ocs.values()) == {real["b"]}
    assert _text_words(doc) == real["default"]
    assert len(page.get_drawings()) == real["drawings"]
    assert _image_blocks(doc) == real["images"]
    doc.set_layer_ui_config(1, 0)
    assert _text_words(doc) == real["ui10"]
    assert len(page.get_drawings()) == real["drawings_on"]
    assert _image_blocks(doc) == real["images_on"]


# === PYOCG-048..056 — /Usage /View /ViewState + config /AS visibility =======

# One `BDC`-gated word (GATED, under `/MC0`) plus an ungated one (FREE). The
# OCG is xref 7 (its `/Usage` is spliced in), the OCMD over it is xref 8, and
# `/D` carries the ON/OFF state plus an optional `/AS` array. `/AS` cannot be
# written through `xref_set_key`, hence the hand-assembled bytes.
_VISIBLE = ["GATED", "FREE"]
_HIDDEN = ["FREE"]
_VIEW_OFF = b"/Usage<</View<</ViewState/OFF>>>>"
_VIEW_ON = b"/Usage<</View<</ViewState/ON>>>>"
_PRINT_OFF = b"/Usage<</Print<</PrintState/OFF>>>>"
_AS_VIEW = b"/AS[<</Event/View/Category[/View]/OCGs[7 0 R]>>]"
_AS_PRINT = b"/AS[<</Event/Print/Category[/Print]/OCGs[7 0 R]>>]"


def _usage_pdf(
    usage: bytes = b"",
    *,
    on: bool = True,
    as_entry: bytes = b"",
    via_ocmd: bool = False,
) -> bytes:
    content = (
        b"/OC /MC0 BDC BT /F1 24 Tf 50 100 Td (GATED) Tj ET EMC\n"
        b"BT /F1 24 Tf 50 150 Td (FREE) Tj ET\n"
    )
    state = b"/ON[7 0 R]/OFF[]" if on else b"/ON[]/OFF[7 0 R]"
    objs = {
        1: b"<</Type/Catalog/Pages 2 0 R/OCProperties 6 0 R>>",
        2: b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        3: b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Contents 4 0 R"
        b"/Resources<</Font<</F1 5 0 R>>/Properties<</MC0 "
        + (b"8" if via_ocmd else b"7")
        + b" 0 R>>>>>>",
        4: b"<</Length "
        + str(len(content)).encode()
        + b">>stream\n"
        + content
        + b"\nendstream",
        5: b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>",
        6: b"<</OCGs[7 0 R]/D<<" + state + b"/Order[7 0 R]" + as_entry + b">>>>",
        7: b"<</Type/OCG/Name(A)" + usage + b">>",
        8: b"<</Type/OCMD/OCGs[7 0 R]>>",
    }
    return _assemble(objs)


# name -> (pdf, words pdfspine extracts, words real PyMuPDF 1.27.2 extracts).
# The two columns differ only for the deliberate `/AS` promotion divergence.
_USAGE_CASES: dict[str, tuple[bytes, list[str], list[str]]] = {
    "on_no_usage": (_usage_pdf(), _VISIBLE, _VISIBLE),
    "on_view_off": (_usage_pdf(_VIEW_OFF), _HIDDEN, _HIDDEN),
    "on_view_off_as_view": (_usage_pdf(_VIEW_OFF, as_entry=_AS_VIEW), _HIDDEN, _HIDDEN),
    "on_view_off_ocmd": (_usage_pdf(_VIEW_OFF, via_ocmd=True), _HIDDEN, _HIDDEN),
    "on_view_on": (_usage_pdf(_VIEW_ON), _VISIBLE, _VISIBLE),
    "off_view_on": (_usage_pdf(_VIEW_ON, on=False), _HIDDEN, _HIDDEN),
    "off_view_on_as_view": (
        _usage_pdf(_VIEW_ON, on=False, as_entry=_AS_VIEW),
        _VISIBLE,  # pdfspine honours the View usage-application entry …
        _HIDDEN,  # … MuPDF / PyMuPDF ignore `/AS` altogether
    ),
    "off_view_on_as_print": (
        _usage_pdf(_VIEW_ON, on=False, as_entry=_AS_PRINT),
        _HIDDEN,
        _HIDDEN,
    ),
    "off_no_usage_as_view": (_usage_pdf(on=False, as_entry=_AS_VIEW), _HIDDEN, _HIDDEN),
    "on_print_off": (_usage_pdf(_PRINT_OFF), _VISIBLE, _VISIBLE),
    "on_bogus_state": (
        _usage_pdf(b"/Usage<</View<</ViewState/Bogus>>>>"),
        _VISIBLE,
        _VISIBLE,
    ),
    "off_bogus_state": (
        _usage_pdf(b"/Usage<</View<</ViewState/Bogus>>>>", on=False),
        _HIDDEN,
        _HIDDEN,
    ),
    "on_empty_view": (_usage_pdf(b"/Usage<</View<<>>>>"), _VISIBLE, _VISIBLE),
    "on_no_view": (_usage_pdf(b"/Usage<<>>"), _VISIBLE, _VISIBLE),
}


def _usage_words(name: str) -> list[str]:
    return _text_words(pdfspine.open(stream=_USAGE_CASES[name][0]))


def test_pyocg_048_viewstate_off_hides_on_ocg():
    # PyMuPDF 1.27.2 oracle: `/ViewState /OFF` hides an OCG the configuration
    # turns ON, and an `/AS` View entry listing it does not rescue it.
    assert _usage_words("on_no_usage") == _VISIBLE
    assert _usage_words("on_view_off") == _HIDDEN
    assert _usage_words("on_view_off_as_view") == _HIDDEN


def test_pyocg_049_viewstate_off_through_ocmd():
    # PyMuPDF 1.27.2 oracle: the OCMD `/OCGs [ocg]` inherits the hidden usage.
    assert _usage_words("on_view_off_ocmd") == _HIDDEN


def test_pyocg_050_viewstate_on_does_not_override_config_off():
    # PyMuPDF 1.27.2 oracle: ViewState ON is not a promotion by itself.
    assert _usage_words("on_view_on") == _VISIBLE
    assert _usage_words("off_view_on") == _HIDDEN


def test_pyocg_051_printstate_off_leaves_view_visible():
    # PyMuPDF 1.27.2 oracle: only the View usage is evaluated.
    assert _usage_words("on_print_off") == _VISIBLE


@pytest.mark.parametrize(
    ("name", "words"),
    [
        ("on_bogus_state", _VISIBLE),
        ("off_bogus_state", _HIDDEN),
        ("on_empty_view", _VISIBLE),
        ("on_no_view", _VISIBLE),
    ],
)
def test_pyocg_052_malformed_usage_follows_config(name, words):
    # PyMuPDF 1.27.2 oracle: a bogus `/ViewState`, `/View <<>>` or `/Usage <<>>`
    # falls through to the configuration state.
    assert _usage_words(name) == words


def test_pyocg_053_as_view_entry_promotes_off_ocg():
    """Deliberate divergence (ISO 32000-1 §8.11.4.4): an OCG that is OFF in the
    configuration but carries ``/ViewState /ON`` and is listed by an ``/AS``
    entry with ``/Event /View`` is **visible** in pdfspine. MuPDF ignores
    ``/AS`` and hides it, so real PyMuPDF 1.27.2 extracts only ``FREE`` here
    (pinned in ``PYOCG-056``); never assert equality on this cell."""
    assert _usage_words("off_view_on_as_view") == _VISIBLE


def test_pyocg_054_as_entry_without_view_does_not_promote():
    # PyMuPDF 1.27.2 oracle: an `/AS` entry for `/Event /Print` only, or an
    # `/AS` View entry without `/ViewState /ON`, leaves an OFF OCG hidden.
    assert _usage_words("off_view_on_as_print") == _HIDDEN
    assert _usage_words("off_no_usage_as_view") == _HIDDEN


@pytest.mark.parametrize(
    ("name", "on"),
    [("on_view_off", True), ("off_view_on_as_view", False), ("on_no_usage", True)],
)
def test_pyocg_055_reporting_ignores_usage(name, on):
    # PyMuPDF 1.27.2 oracle (layer_ui_configs): the reporting API mirrors the
    # configuration state only — `/Usage` neither hides an ON row nor promotes
    # an OFF one, even where `get_text` disagrees.
    doc = pdfspine.open(stream=_USAGE_CASES[name][0])
    assert _on_states(doc, 7) == {7: on}
    assert _ui_on(doc) == {"A": on}


def test_pyocg_056_live_oracle_usage_matrix(tmp_path):
    """Every ``/Usage`` / ``/AS`` fixture read by real PyMuPDF: ``get_text`` and
    the ``layer_ui_configs`` ON states agree with pdfspine on every row except
    the ``/AS`` promotion cell, where pdfspine shows ``GATED`` and PyMuPDF hides
    it (deliberate divergence, see ``PYOCG-053``). PyMuPDF 1.27.2's
    ``get_ocgs()["on"]`` is not compared: it reports ``True`` for every OCG of
    these files, OFF ones included."""
    if not _real_pymupdf_available():
        pytest.skip("no real PyMuPDF in the subprocess")
    paths: dict[str, str] = {}
    for name, (pdf, _ours, _theirs) in _USAGE_CASES.items():
        path = tmp_path / f"{name}.pdf"
        path.write_bytes(pdf)
        paths[name] = str(path)
    child = (
        "import json, pymupdf\n"
        "out = {}\n"
        "for name, path in json.loads(__PATHS__).items():\n"
        "    d = pymupdf.open(path)\n"
        "    out[name] = {'text': d[0].get_text().split(),\n"
        "                 'ui': [int(u['on']) for u in d.layer_ui_configs()]}\n"
        "print(json.dumps(out))\n"
    ).replace("__PATHS__", repr(json.dumps(paths)))
    real = _run_child(child)

    for name, (pdf, ours_words, theirs_words) in _USAGE_CASES.items():
        doc = pdfspine.open(stream=pdf)
        assert _text_words(doc) == ours_words, name
        assert real[name]["text"] == theirs_words, name
        if name != "off_view_on_as_view":
            assert real[name]["text"] == _text_words(doc), name
        assert real[name]["ui"] == [int(u["on"]) for u in doc.layer_ui_configs()], name
    assert real["off_view_on_as_view"]["text"] == _HIDDEN  # MuPDF ignores /AS
    assert _text_words(
        pdfspine.open(stream=_USAGE_CASES["off_view_on_as_view"][0])
    ) == (_VISIBLE)
