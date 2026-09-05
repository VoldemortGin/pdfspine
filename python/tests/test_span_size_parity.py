"""Structured size parity probes, including format and legacy-consumer scope."""

from __future__ import annotations

import json
import math

import pytest

from conformance.probe_span_size_parity import unique_matches
from python.tests.test_rawdict_serialization import _document, _spans


@pytest.mark.parametrize(
    ("content", "declared", "rendered"),
    [
        (b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj ET", 1.0, 12.0),
        (b"q 2 0 0 2 0 0 cm BT /F1 12 Tf 50 350 Td (A) Tj ET Q", 12.0, 24.0),
        (b"BT /F1 12 Tf 50 Tz 100 700 Td (A) Tj ET", 12.0, math.sqrt(72)),
        (b"BT /F1 1 Tf 20 0 0 10 100 700 Tm (A) Tj ET", 1.0, math.sqrt(200)),
        (b"BT /F1 1 Tf 12 0 6 12 100 700 Tm (A) Tj ET", 1.0, 12.0),
        (b"BT /F1 -12 Tf 100 700 Td (A) Tj ET", -12.0, 12.0),
        (b"BT /F1 1 Tf 12 0 0 0 100 700 Tm (A) Tj ET", 1.0, 0.0),
    ],
)
def test_structured_size_reports_rendered_and_preserves_declared(
    content: bytes, declared: float, rendered: float
) -> None:
    """PYSIZE-001: pinned affine probes match the measured PyMuPDF size rule."""
    with _document(content) as doc:
        for mode in ("dict", "rawdict", "json", "rawjson"):
            result = doc[0].get_text(mode)
            if isinstance(result, str):
                result = json.loads(result)
            span = _spans(result)[0]
            assert math.isclose(span["size"], rendered, rel_tol=1e-6, abs_tol=1e-6)
            assert span["size"] == span["rendered_size"]
            assert span["declared_size"] == declared
            if mode in ("dict", "rawdict"):
                assert type(span["size"]) is float


def test_structured_size_change_does_not_redefine_other_size_consumers() -> None:
    """PYSIZE-002: this experiment does not silently alter markup or trace."""
    with _document(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj ET") as doc:
        page = doc[0]
        assert _spans(page.get_text("dict"))[0]["size"] == 12.0
        assert 'size="1"' in page.get_text("xml")
        assert "font-size:1pt" in page.get_text("html")
        assert "font-size:1pt" in page.get_text("xhtml")
        assert page.get_texttrace()[0]["size"] == 1.0


def test_size_oracle_matching_uses_geometry_and_rejects_ambiguous_duplicates() -> None:
    """PYSIZE-003: the measurement must not silently pair by extraction order."""
    ours = [["A", 9.9999, 20.0], ["B", 11.0, 20.0]]
    reference = [["B", 11.004, 20.0], ["A", 10.0001, 20.0]]
    assert unique_matches(ours, reference, 0.01) == ([(0, 1), (1, 0)], 0)
    assert unique_matches([["A", 10.0, 20.0]], [["B", 10.0, 20.0]], 0.01) == ([], 0)
    duplicate = [["A", 10.0, 20.0], ["A", 10.0, 20.0]]
    assert unique_matches(duplicate, duplicate[:1], 0.01) == ([], 2)
    assert unique_matches(duplicate[:1], duplicate, 0.01) == ([], 1)
