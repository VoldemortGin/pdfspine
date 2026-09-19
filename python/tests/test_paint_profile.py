"""Strict paint-profile binding tests with self-generated PDF bytes."""

from __future__ import annotations

from types import MappingProxyType

import pdfspine
import pytest


def _pdf() -> bytes:
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] "
            b"/Resources << /ExtGState << /GS1 << /Type /ExtGState "
            b"/BM /Normal /CA 1 >> >> >> >>",
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R "
            b"/Group << /Type /Group /S /Transparency /CS /DeviceRGB >> >>",
        ),
        (4, b"<< /Length 24 >>\nstream\n/GS1 gs 10 10 20 20 re f\nendstream"),
    ]
    output = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    for number, body in objects:
        offsets[number] = len(output)
        output += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(output)
    output += b"xref\n0 5\n0000000000 65535 f \n"
    for number in range(1, 5):
        output += f"{offsets[number]:010} 00000 n \n".encode()
    output += (
        b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n"
        + str(xref).encode()
        + b"\n%%EOF\n"
    )
    return bytes(output)


def test_paint_profile_is_a_deep_read_only_snapshot() -> None:
    page = pdfspine.open(stream=_pdf())[0]
    profile = page.get_paint_profile()

    assert profile.version == "strict-paint-profile-v1"
    assert profile.complete
    assert isinstance(profile.resource_scopes, tuple)
    assert isinstance(profile.resource_scopes[0], MappingProxyType)
    assert isinstance(profile.resource_scopes[0]["entries"], tuple)
    assert isinstance(profile.resource_scopes[0]["entries"][0], MappingProxyType)
    assert isinstance(profile.resource_scopes[0]["group"], MappingProxyType)
    assert profile.resource_scopes[0]["group"]["color_space"] == "DeviceRGB"

    with pytest.raises(AttributeError):
        profile.complete = False
    with pytest.raises(TypeError):
        profile.resource_scopes[0]["scope_id"] = "forged"
    with pytest.raises(TypeError):
        profile.resource_scopes[0]["entries"][0]["selected"] = False
    with pytest.raises(TypeError):
        profile.resource_scopes[0]["group"]["isolated"] = True


def test_selected_extgstate_is_typed_without_exposing_mutable_dicts() -> None:
    profile = pdfspine.open(stream=_pdf())[0].get_paint_profile()
    state = profile.resource_scopes[0]["entries"][0]["ext_gstate"]

    assert isinstance(state, MappingProxyType)
    assert state["stroke_alpha"] == 1.0
    assert state["blend_mode"] == "Normal"
    with pytest.raises(TypeError):
        state["stroke_alpha"] = 1.0
