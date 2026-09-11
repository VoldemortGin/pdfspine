"""Initial independent acceptance cases for the TextPage replay adapter."""

import pdfspine as p
import pytest


def page(text):
    doc = p.open()
    doc.new_page(width=200, height=120).insert_text((20, 40), text)
    return doc


def test_identity_repeated_append_and_existing_blocks():
    source, old = page("NEW"), page("OLD")
    target = old[0].get_displaylist().get_textpage(flags=0)
    identity, rect = id(target), tuple(target.rect)
    original = target.extractDICT()["blocks"]
    device = p.ReplayDevice.for_textpage(target, flags=0)
    record = source[0].get_displaylist()
    source.close()
    for _ in range(2):
        assert record.run(device, None, None) is None
    assert id(target) == identity and tuple(target.rect) == rect
    assert target.extractText().split() == ["OLD", "NEW", "NEW"]
    assert target.extractDICT()["blocks"][: len(original)] == original
    assert [b["number"] for b in target.extractDICT()["blocks"]] == [0, 1, 2]
    assert len(target.search("NEW")) == 2


def test_no_area_matches_existing_extend_textpage():
    source, old = page("NEW"), page("OLD")
    expected = old[0].get_displaylist().get_textpage(flags=0)
    actual = old[0].get_displaylist().get_textpage(flags=0)
    matrix = p.Matrix(1.2, 0.1, 0.2, 0.8, 5, 10)
    source[0].extend_textpage(expected, flags=0, matrix=matrix)
    source[0].get_displaylist().run(
        p.ReplayDevice.for_textpage(actual, flags=0), matrix, None
    )
    assert actual.extractRAWDICT() == expected.extractRAWDICT()


def test_empty_area_and_invalid_geometry_leave_target_core_unchanged():
    source, old = page("NEW"), page("OLD")
    target = old[0].get_textpage()
    original = target._tp
    record = source[0].get_displaylist()
    device = p.ReplayDevice.for_textpage(target)
    record.run(device, None, (0, 0, 0, 0))
    assert target._tp is original
    with pytest.raises(ValueError):
        record.run(device, (1, 0, 0, 1, float("nan"), 0), None)
    assert target._tp is original
    record.run(device, None, None)
    assert target.extractText().split() == ["OLD", "NEW"]


@pytest.mark.parametrize("mode", [0, 3, 7])
def test_semantic_text_modes_match_b_even_when_not_painted(mode):
    from python.tests.test_rawdict_serialization import _document

    source = _document(f"BT /F1 12 Tf {mode} Tr 40 700 Td (SEMANTIC) Tj ET".encode())
    old = _document(b"")
    actual = old[0].get_displaylist().get_textpage(flags=0)
    expected = old[0].get_displaylist().get_textpage(flags=0)
    source[0].extend_textpage(expected, flags=0)
    source[0].run(p.ReplayDevice.for_textpage(actual), None)
    assert actual.extractRAWDICT() == expected.extractRAWDICT()
    assert actual.extractText() == "SEMANTIC\n"


@pytest.mark.parametrize("hidden", [False, True])
def test_form_and_annotation_global_ranges_preserve_semantic_fonts(hidden):
    from python.tests.test_displaylist_textpage import _annotation_document
    from python.tests.test_rawdict_serialization import _spans

    source = _annotation_document()
    # Prepend real page content, then recurse into both forms, then append APs.
    source.update_stream(4, b"q 1 0 0 1 10 50 cm /A Do Q")
    if hidden:
        oc = source.add_ocg("hidden first annotation", on=False)
        source.xref_set_key(11, "OC", f"{oc} 0 R")
    old = p.open()
    old.new_page(width=300, height=200)
    actual = old[0].get_textpage()
    expected = old[0].get_textpage()
    source[0].extend_textpage(expected, flags=4)
    record = source[0].get_displaylist()
    source.close()
    record.run(p.ReplayDevice.for_textpage(actual, flags=4), None, None)
    assert actual.extractRAWDICT() == expected.extractRAWDICT()
    assert {s["font"] for s in _spans(actual.extractDICT())} == {"Helvetica", "Courier"}
    chars = [
        c
        for span in _spans(actual.extractRAWDICT())
        for c in span["chars"]
        if not c["synthetic"]
    ]
    assert len(chars) == (9 if hidden else 14)


@pytest.mark.parametrize("initial,added", [(0, 0), (0, 4), (4, 0), (4, 4)])
def test_flags_resources_colliding_names_and_source_close(initial, added):
    from python.tests.test_displaylist_textpage import _resource_pdf

    source = p.open(stream=_resource_pdf())
    old = p.open(stream=_resource_pdf())
    source.update_stream(9, b"\x00\xff\x00")
    actual = old[0].get_displaylist().get_textpage(flags=initial)
    expected = old[0].get_displaylist().get_textpage(flags=initial)
    source[0].extend_textpage(expected, flags=added)
    record = source[0].get_displaylist()
    source.close()
    old.close()
    record.run(p.ReplayDevice.for_textpage(actual, flags=added), None, None)
    assert actual.extractRAWDICT() == expected.extractRAWDICT()
    assert len([b for b in actual.extractDICT()["blocks"] if b["type"] == 1]) == (
        2 * bool(initial) + 2 * bool(added)
    )


def test_area_selects_full_unknown_text_run_not_individual_glyphs():
    from python.tests.test_rawdict_serialization import _document

    source = _document(b"BT /F1 12 Tf 40 700 Td (ALPHA BETA) Tj ET")
    old = _document(b"")
    for area in [(40, 85, 42, 94), (500, 500, 501, 501)]:
        target = old[0].get_textpage()
        source[0].get_displaylist().run(p.ReplayDevice.for_textpage(target), None, area)
        assert target.extractText() == "ALPHA BETA\n"


def test_image_area_selection_keeps_whole_placement_and_clip_stack():
    from python.tests.test_displaylist_textpage import _resource_pdf

    source = p.open(stream=_resource_pdf())
    old = p.open()
    old.new_page(width=300, height=200)
    target = old[0].get_textpage()
    source[0].get_displaylist().run(
        p.ReplayDevice.for_textpage(target, flags=4), None, (165, 125, 166, 126)
    )
    # Unknown text bounds retain both runs; only the second image intersects area.
    blocks = target.extractDICT()["blocks"]
    assert target.extractText() == "Alpha\nBeta\n"
    images = [b for b in blocks if b["type"] == 1]
    assert len(images) == 1 and images[0]["bbox"] == (160.0, 120.0, 190.0, 150.0)


def test_target_promotion_failure_and_invalid_factory_are_atomic():
    from python.tests.test_extend_textpage import document, add_image

    old, old_page = document("OLD")
    source, source_page = document("NEW")
    add_image(old_page, (255, 0, 0))
    target = old_page.get_textpage()
    old.update_stream(old_page.get_images()[0][0], b"")
    original = target._tp
    with pytest.raises(p.PdfDecodeError, match="promote target image"):
        source_page.run(p.ReplayDevice.for_textpage(target), None)
    assert target._tp is original
    for flags in [-1, 2**32]:
        with pytest.raises(OverflowError):
            p.ReplayDevice.for_textpage(target, flags)
    with pytest.raises(TypeError):
        p.ReplayDevice.for_textpage(target, 1.2)
    with pytest.raises(TypeError):
        p.ReplayDevice.for_textpage(object())


def test_concurrent_target_change_is_not_overwritten_and_device_recovers():
    from pdfspine.replay import _run

    source, old, other = page("NEW"), page("OLD"), page("CONCURRENT")
    target = old[0].get_textpage()
    record = source[0].get_displaylist()
    device = p.ReplayDevice.for_textpage(target)

    class InterleavedRecord:
        def _replay_textpage(self, original, flags, matrix, area):
            replacement = record._replay_textpage(original, flags, matrix, area)
            # Deterministically model an append committing while native staging
            # has released the GIL, rather than rely on a timing-sensitive race.
            other[0].extend_textpage(target)
            return replacement

    with pytest.raises(RuntimeError, match="changed during replay"):
        _run(InterleavedRecord(), device, None, None)
    assert target.extractText().split() == ["OLD", "CONCURRENT"]
    record.run(device, None, None)
    assert target.extractText().split() == ["OLD", "CONCURRENT", "NEW"]


def test_explicit_clip_filters_text_run_and_restore_keeps_later_semantics():
    from python.tests.test_rawdict_serialization import _document

    source = _document(
        b"q 0 650 100 100 re W n BT /F1 12 Tf 40 700 Td (FIRST) Tj ET Q BT /F1 12 Tf 200 700 Td (SECOND) Tj ET"
    )
    old = _document(b"")
    target = old[0].get_textpage()
    source[0].get_displaylist().run(
        p.ReplayDevice.for_textpage(target), None, (210, 90, 211, 91)
    )
    assert target.extractText() == "SECOND\n"


@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
def test_crop_rotation_caller_affine_and_image_geometry_match_b(rotation):
    from python.tests.test_displaylist_textpage import _resource_pdf

    source = p.open(stream=_resource_pdf())
    source[0].set_cropbox(p.Rect(5, 10, 295, 190))
    source[0].set_rotation(rotation)
    old = p.open()
    old.new_page(width=600, height=600)
    expected, actual = old[0].get_textpage(), old[0].get_textpage()
    matrix = p.Matrix(1.2, 0.1, 0.2, 0.8, 15, 20)
    source[0].extend_textpage(expected, flags=4, matrix=matrix)
    source[0].get_displaylist().run(
        p.ReplayDevice.for_textpage(actual, 4), matrix, None
    )
    assert actual.extractRAWDICT() == expected.extractRAWDICT()
    assert len([b for b in actual.extractDICT()["blocks"] if b["type"] == 1]) == 2


def test_target_origin_clip_is_distinct_from_operation_area():
    source, old = page("NEW"), page("OLD")
    actual, expected = old[0].get_textpage(), old[0].get_textpage()
    matrix = p.Matrix(1, 0, 0, 1, 175, 0)
    source[0].extend_textpage(expected, matrix=matrix)
    source[0].get_displaylist().run(
        p.ReplayDevice.for_textpage(actual), matrix, (195, 35, 196, 40)
    )
    assert actual.extractRAWDICT() == expected.extractRAWDICT()
    assert actual.extractText().split() == ["OLD", "N"]


def test_subset_names_remain_dynamic_after_replay_and_close():
    from python.tests.test_subset_fontnames import subset_document
    from python.tests.test_rawdict_serialization import _document, _spans

    source, old = subset_document(), _document(b"")
    target = old[0].get_textpage()
    source[0].run(p.ReplayDevice.for_textpage(target), None)
    source.close()
    old.close()
    try:
        p.TOOLS.set_subset_fontnames(True)
        assert [s["font"] for s in _spans(target.extractDICT())] == [
            "ABCDEF+Helvetica",
            "UVWXYZ+Helvetica",
        ]
        p.TOOLS.set_subset_fontnames(False)
        assert [s["font"] for s in _spans(target.extractDICT())] == ["Helvetica"]
        assert target.extractText() == "AB\n"
    finally:
        p.TOOLS.set_subset_fontnames(False)


def test_hidden_marked_text_rollback_does_not_shift_later_glyph_ranges():
    from python.tests.test_rawdict_serialization import _document, _spans

    source = _document(
        b"BT /F1 12 Tf 40 700 Td (A) Tj /OC /Hidden BDC (HIDDEN) Tj EMC (B) Tj ET"
    )
    oc = source.add_ocg("hidden marked text", on=False)
    source.xref_set_key(
        3,
        "Resources",
        f"<< /Font << /F1 5 0 R >> /Properties << /Hidden {oc} 0 R >> >>",
    )
    old = _document(b"")
    target, expected = old[0].get_textpage(), old[0].get_textpage()
    source[0].extend_textpage(expected)
    source[0].run(p.ReplayDevice.for_textpage(target), None)
    assert target.extractRAWDICT() == expected.extractRAWDICT()
    assert (
        "".join(
            c["c"]
            for s in _spans(target.extractRAWDICT())
            for c in s["chars"]
            if not c["synthetic"]
        )
        == "AB"
    )


@pytest.mark.parametrize("typed", [False, True])
def test_closed_page_source_fails_before_callbacks_or_target_commit(typed):
    source, old = page("NEW"), page("OLD")
    source_page = source[0]
    record = source_page.get_displaylist()
    target = old[0].get_textpage()
    original = target._tp
    events = []
    device = (
        p.ReplayDevice.for_textpage(target) if typed else p.ReplayDevice(events.append)
    )
    source.close()
    with pytest.raises(ValueError, match="closed"):
        source_page.run(device, None)
    assert target._tp is original and events == []
    record.run(device, None, None)
    assert target.extractText().split() == (["OLD", "NEW"] if typed else ["OLD"])
    if not typed:
        assert events[0].kind == "begin" and events[-1].kind == "end"
