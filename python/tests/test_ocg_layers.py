"""PyMuPDF-compat optional-content: layer configurations, OCMDs, hidden render.

``PYOCG-004`` … ``PYOCG-038`` cover the seven ``Document`` methods added on top
of the M7 optional-content surface (``PYOCG-001``…``003`` live in
``test_m7.py``): ``get_layers`` / ``add_layer`` / ``switch_layer`` /
``set_layer_ui_config`` / ``get_oc`` / ``get_ocmd`` / ``set_ocmd``, plus the
parity fixes to ``layer_ui_configs`` (row-index ``number``, ``"radiobox"`` type,
locked label rows) and the in-memory "layer view" reflected by ``get_ocgs`` /
``layer_ui_configs`` / ``ocg_state``, and the Rust interpreter's handling of
hidden optional content (BDC marked content, XObject ``/OC``, OCMDs) during
render / text extraction.

Every expected value is captured from **REAL PyMuPDF 1.28.2** (the "PyMuPDF
1.28.2 oracle") and hardcoded. ``PYOCG-037``/``038`` additionally drive a real
PyMuPDF inside a subprocess (the in-process ``pymupdf`` is pdfspine's shim via
``conftest``); they skip when the child cannot import a real PyMuPDF.

All fixtures are hand-written PDF bytes (opened via ``stream=``) — no external
files (PRD §10).
"""

from __future__ import annotations

import json
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


def _real_pymupdf_available() -> bool:
    probe = "import pymupdf, sys; sys.exit(0 if hasattr(pymupdf, 'mupdf') else 1)"
    return (
        subprocess.run([sys.executable, "-c", probe], capture_output=True).returncode
        == 0
    )


def _run_child(code: str) -> dict:
    result = subprocess.run(
        [sys.executable, "-c", code], capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


# === PYOCG-004..010 — get_layers / add_layer / switch_layer ================


def test_pyocg_004_fresh_doc_has_no_layers():
    # PyMuPDF 1.28.2 oracle: a fresh doc has no /Configs; negative/zero switch is a no-op.
    doc = _blank()
    assert doc.get_layers() == []
    assert doc.switch_layer(0) is None
    assert doc.switch_layer(-1) is None


def test_pyocg_005_switch_layer_out_of_range_no_configs():
    # PyMuPDF 1.28.2 oracle: with no /Configs, config >= 1 raises "bad layer number".
    doc = _blank()
    with pytest.raises(ValueError, match="bad layer number"):
        doc.switch_layer(1)


def test_pyocg_006_add_ocg_yields_no_configs():
    # PyMuPDF 1.28.2 oracle: OCGs alone are not layer configurations.
    doc, _a, _b = _with_ab()
    assert doc.get_layers() == []


def test_pyocg_007_add_layer_get_layers_roundtrip():
    # PyMuPDF 1.28.2 oracle (pdfspine returns this immediately, no reopen needed).
    doc, _a, _b = _with_three_configs()
    assert doc.get_layers() == [
        {"number": 0, "name": "cfg1", "creator": "me"},
        {"number": 1, "name": "cfg2", "creator": ""},
        {"number": 2, "name": "cfg3", "creator": ""},
    ]


def test_pyocg_008_configs_stored_as_direct_dicts():
    # PyMuPDF 1.28.2 oracle: /Configs holds inline dicts with Name/Creator/BaseState/ON.
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
    # PyMuPDF 1.28.2 oracle: switch_layer selects the config's ON/OFF as the view.
    doc, a, b = _with_three_configs()
    doc.switch_layer(config)
    assert _ui_on(doc) == {"A": a_on, "B": b_on}
    assert _on_states(doc, a, b) == {a: a_on, b: b_on}


def test_pyocg_010_switch_layer_out_of_range_raises():
    # PyMuPDF 1.28.2 oracle: past the last config raises "Illegal Layer config".
    doc, _a, _b = _with_three_configs()
    with pytest.raises(ValueError, match="Illegal Layer config"):
        doc.switch_layer(5)


def test_pyocg_011_switch_layer_is_in_memory_only():
    # PyMuPDF 1.28.2 oracle: switching leaves the persisted /D unchanged.
    doc, a, b = _with_three_configs()
    doc.switch_layer(2)  # A off, B on in the in-memory view
    assert _on_states(doc, a, b) == {a: False, b: True}
    reopened = pdfspine.open(stream=doc.tobytes())
    assert reopened.get_layers() == doc.get_layers()
    assert _on_states(reopened, a, b) == {a: True, b: False}  # /D default is preserved


def test_pyocg_012_switch_layer_as_default_rewrites_d():
    # PyMuPDF 1.28.2 oracle: as_default rewrites /D from the config and deletes /Configs.
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
    # PyMuPDF 1.28.2 oracle: action 2 clears, 0 sets, 1 toggles a panel row.
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
    # PyMuPDF 1.28.2 oracle: a str `number` addresses the row by its text.
    doc, a, b = _with_ab()
    doc.set_layer_ui_config("A", 1)  # toggle A (currently on) off
    assert _on_states(doc, a, b) == {a: False, b: False}


def test_pyocg_015_set_layer_ui_config_persisted_d_unchanged():
    # PyMuPDF 1.28.2 oracle: panel overrides are in-memory; /D stays put.
    doc, a, b = _with_ab()
    doc.set_layer_ui_config(0, 2)  # A off in the view only
    reopened = pdfspine.open(stream=doc.tobytes())
    assert _on_states(reopened, a, b) == {a: True, b: False}


@pytest.mark.parametrize(
    ("number", "match"),
    [(5, r"."), ("nosuch", r"bad OCG 'nosuch'\.")],
)
def test_pyocg_016_set_layer_ui_config_errors(number, match):
    # PyMuPDF 1.28.2 oracle: out-of-range index or unknown row text raises ValueError.
    doc, _a, _b = _with_ab()
    with pytest.raises(ValueError, match=match):
        doc.set_layer_ui_config(number, 0)


# === PYOCG-017..019 — layer_ui_configs parity ==============================


def test_pyocg_017_layer_ui_configs_nested_order_depth():
    # PyMuPDF 1.28.2 oracle: /Order [A [B C]] nests B,C one level under A.
    doc = pdfspine.open(stream=_build_order_pdf(b"[7 0 R [8 0 R 9 0 R]]"))
    rows = doc.layer_ui_configs()
    assert [(r["number"], r["text"], r["depth"], r["type"]) for r in rows] == [
        (0, "A", 0, "checkbox"),
        (1, "B", 1, "checkbox"),
        (2, "C", 1, "checkbox"),
    ]


def test_pyocg_018_layer_ui_configs_label_group_locked():
    # PyMuPDF 1.28.2 oracle: a leading string in a nested array is a locked label row.
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
    # PyMuPDF 1.28.2 oracle: /RBGroups members render as "radiobox" rows.
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
    # PyMuPDF 1.28.2 oracle: an XObject without /OC yields 0.
    doc = pdfspine.open(stream=_image_and_form_pdf())
    assert doc.get_oc(xref) == 0


def test_pyocg_021_get_oc_after_set_oc():
    # PyMuPDF 1.28.2 oracle: set_oc binds /OC to an OCG then an OCMD; get_ocgs stays OCG-only.
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
    # PyMuPDF 1.28.2 oracle: a non-XObject xref raises "bad object type at xref N".
    doc = pdfspine.open(stream=_layered_pdf())
    with pytest.raises(ValueError, match=f"bad object type at xref {xref}"):
        doc.get_oc(xref)


def test_pyocg_023_get_oc_bad_xref():
    # PyMuPDF 1.28.2 oracle: xref 0 or >= xref_length raises "bad xref".
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
    # PyMuPDF 1.28.2 oracle: policy normalises to AnyOn/AllOn/AnyOff/AllOff (or None).
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
    # PyMuPDF 1.28.2 oracle: visibility expressions round-trip as nested lists.
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
    # PyMuPDF 1.28.2 oracle: ocgs + policy + ve can coexist in one OCMD.
    doc, a, b = _with_ab()
    xref = doc.set_ocmd(ocgs=[a], policy="AnyOff", ve=["or", a, b])
    assert doc.get_ocmd(xref) == {
        "xref": xref,
        "ocgs": [a],
        "policy": "AnyOff",
        "ve": ["or", a, b],
    }


def test_pyocg_027_set_ocmd_replace_and_ignore():
    # PyMuPDF 1.28.2 oracle: a replace rewrites the whole dict; a bare int ocgs is ignored.
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
    # PyMuPDF 1.28.2 oracle: malformed policy / OCGs / ve raise ValueError (the ve
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
    # PyMuPDF 1.28.2 oracle: replacing a non-OCMD xref raises "bad xref or not an OCMD".
    doc, _a, _b = _with_ab()
    with pytest.raises(ValueError, match="bad xref or not an OCMD"):
        doc.set_ocmd(xref=doc.pdf_catalog())


@pytest.mark.parametrize("bad", ["catalog", "ocg"])
def test_pyocg_029_get_ocmd_bad_object_type(bad):
    # PyMuPDF 1.28.2 oracle: get_ocmd on a non-OCMD object raises "bad object type".
    doc, a, _b = _with_ab()
    xref = doc.pdf_catalog() if bad == "catalog" else a
    with pytest.raises(ValueError, match="bad object type"):
        doc.get_ocmd(xref)


def test_pyocg_029b_get_ocmd_bad_xref():
    # PyMuPDF 1.28.2 oracle: an out-of-range xref raises "bad xref".
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
    # PyMuPDF 1.28.2 oracle: layer/OCMD methods on a closed doc raise "document closed".
    doc = _blank()
    doc.close()
    with pytest.raises(ValueError, match="document closed"):
        getattr(doc, method)(*args)


def test_pyocg_030b_get_oc_closed_document():
    # PyMuPDF 1.28.2 oracle: get_oc has its own wording (sic) on a closed doc.
    doc = _blank()
    doc.close()
    with pytest.raises(ValueError, match="document close or encrypted"):
        doc.get_oc(1)


# === PYOCG-031..036 — hidden optional content (render / extract) ===========


def test_pyocg_031_hidden_default_view():
    # PyMuPDF 1.28.2 oracle: /D (A on, B off) hides BBBB/CCCC, the /OC B image and the rect.
    doc = pdfspine.open(stream=_layered_pdf())
    assert _text_words(doc) == ["AAAA", "DDDD", "EEEE", "FFFF"]
    assert _image_blocks(doc) == 0
    assert doc[0].get_drawings() == []


def test_pyocg_032_all_layers_visible():
    # PyMuPDF 1.28.2 oracle: turning B on reveals every word, the image and the rect.
    doc = pdfspine.open(stream=_layered_pdf())
    hidden_pixels = _nonwhite(doc)
    doc.set_layer_ui_config(1, 0)  # B on
    assert _text_words(doc) == ["AAAA", "BBBB", "CCCC", "DDDD", "EEEE", "FFFF"]
    assert _image_blocks(doc) == 1
    assert len(doc[0].get_drawings()) == 1
    assert _nonwhite(doc) > hidden_pixels  # relative: more paint once B is visible


def test_pyocg_033_layer_a_off():
    # PyMuPDF 1.28.2 oracle: with A off and B on only the B-gated BBBB and the
    # un-gated EEEE survive (AAAA/FFFF need A; CCCC needs AllOn; DDDD needs Not-B).
    doc = pdfspine.open(stream=_layered_pdf())
    doc.set_layer_ui_config(1, 0)  # B on
    doc.set_layer_ui_config(0, 2)  # A off
    assert _text_words(doc) == ["BBBB", "EEEE"]


def test_pyocg_034_switch_config_only_b():
    # PyMuPDF 1.28.2 oracle: config "only B" shows BBBB/EEEE; a negative switch is a no-op.
    doc = pdfspine.open(stream=_layered_pdf())
    doc.switch_layer(0)  # config "only B" (BaseState OFF, ON=[B])
    assert _text_words(doc) == ["BBBB", "EEEE"]
    doc.switch_layer(-1)  # no-op
    assert _text_words(doc) == ["BBBB", "EEEE"]


def test_pyocg_035_set_layer_direct():
    # PyMuPDF 1.28.2 oracle: set_layer writes /D (B on, A off) and resets the view.
    doc = pdfspine.open(stream=_layered_pdf())
    doc.set_layer(-1, on=[8], off=[7])
    assert _text_words(doc) == ["BBBB", "EEEE"]


def test_pyocg_036_layered_get_oc_and_get_ocmd():
    # PyMuPDF 1.28.2 oracle: /OC bindings and OCMD definitions read back on the layered file.
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


# === PYOCG-037..038 — live PyMuPDF 1.28.2 oracle (subprocess) ==============


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
    # hardcoded PyMuPDF 1.28.2 get_layer(n) expectations
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

    # hardcoded PyMuPDF 1.28.2 expectations for the three view states
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
