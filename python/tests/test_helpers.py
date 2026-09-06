"""Branch/edge coverage for the module-level helpers (`helpers.py`).

Extends the long-tail helper parity tests in ``test_longtail11.py`` by driving
the branches they skip: the rotated (quadrant 2/3/4 and small-glyph-height)
paths of ``recover_bbox_quad``; the ``None``-direction, bad-argument and
single-element paths of ``recover_char_quad`` / ``recover_span_quad`` /
``recover_line_quad``; the control-character escapes of ``get_pdf_str``; both
timezone signs of ``get_pdf_now``; the XHTML ``ConversionHeader``; the whole
``_make_output`` destination resolver (fd / path / path-append / stream /
pylogging / default / bad-prefix) with its ``_Out`` logging sink; and the two
failure fallbacks of ``log``. PyMuPDF 1.27 semantics are the spec.
"""

import io
import logging
import os
import sys

import pytest

from pdfspine import helpers
from pdfspine.document import TOOLS


_SPAN = {
    "bbox": (72.0, 50.5, 172.04, 77.98),
    "size": 20.0,
    "ascender": 1.075,
    "descender": -0.299,
}


def _q(q):
    return tuple(tuple(round(v, 3) for v in p) for p in (q.ul, q.ur, q.ll, q.lr))


# --------------------------------------------------------------------------- #
# recover_bbox_quad — rotated quadrants + small-glyph-heights
# --------------------------------------------------------------------------- #
class TestRecoverBboxQuad:
    def test_quadrant2_direction(self):
        # dir=(-1,0): hc<=0, hs<=0 -> the 180deg (quadrant 2) corner assignment.
        got = _q(helpers.recover_bbox_quad((-1.0, 0.0), _SPAN, _SPAN["bbox"]))
        assert got == (
            (172.04, 77.98),
            (72.0, 77.98),
            (172.04, 50.5),
            (72.0, 50.5),
        )

    def test_quadrant3_direction(self):
        # dir=(0,1): hc<=0, hs>=0 -> quadrant 3.
        got = _q(helpers.recover_bbox_quad((0.0, 1.0), _SPAN, _SPAN["bbox"]))
        assert got == (
            (172.04, 50.5),
            (99.48, 77.98),
            (144.56, 50.5),
            (72.0, 77.98),
        )

    def test_quadrant4_direction(self):
        # dir=(0.6,0.8): hc>=0, hs>=0 -> quadrant 4 (the else branch).
        got = _q(helpers.recover_bbox_quad((0.6, 0.8), _SPAN, _SPAN["bbox"]))
        assert got == (
            (93.984, 50.5),
            (172.04, 61.492),
            (72.0, 66.988),
            (150.056, 77.98),
        )

    def test_none_line_dir_reads_span_dir(self):
        span = dict(_SPAN, dir=(1.0, 0.0))
        got = _q(helpers.recover_bbox_quad(None, span, span["bbox"]))
        assert got == (
            (72.0, 50.5),
            (172.04, 50.5),
            (72.0, 77.98),
            (172.04, 77.98),
        )

    def test_small_glyph_heights_uses_unit_height(self):
        try:
            TOOLS.set_small_glyph_heights(True)
            got = _q(helpers.recover_bbox_quad((1.0, 0.0), _SPAN, _SPAN["bbox"]))
        finally:
            TOOLS.set_small_glyph_heights(False)
        # d == 1 -> height == size == 20, so the quad is a 20-unit-tall band.
        assert got == (
            (72.0, 57.98),
            (172.04, 50.5),
            (72.0, 77.98),
            (172.04, 70.5),
        )


# --------------------------------------------------------------------------- #
# recover_char_quad / recover_span_quad / recover_line_quad — guards & branches
# --------------------------------------------------------------------------- #
class TestRecoverCharSpanLine:
    def test_char_quad_none_dir_reads_span_dir(self):
        span = dict(_SPAN, dir=(1.0, 0.0))
        char = {"bbox": (72.0, 50.5, 86.44, 77.98)}
        got = _q(helpers.recover_char_quad(None, span, char))
        assert got[0] == (72.0, 50.5) and got[3] == (86.44, 77.98)

    def test_char_quad_bad_line_dir(self):
        with pytest.raises(ValueError):
            helpers.recover_char_quad([1, 0], _SPAN, {"bbox": (0, 0, 1, 1)})

    def test_char_quad_bad_span(self):
        with pytest.raises(ValueError):
            helpers.recover_char_quad((1.0, 0.0), [1, 2], {"bbox": (0, 0, 1, 1)})

    def test_char_quad_bad_char_type(self):
        with pytest.raises(ValueError):
            helpers.recover_char_quad((1.0, 0.0), _SPAN, 123)

    def test_span_quad_none_dir_with_chars(self):
        span = dict(_SPAN, dir=(1.0, 0.0))
        span["chars"] = [{"bbox": (72.0, 50.5, 86.44, 77.98)}]
        got = _q(helpers.recover_span_quad(None, span, span["chars"]))
        assert got[0] == (72.0, 50.5) and got[3] == (86.44, 77.98)

    def test_span_quad_missing_chars_key_raises(self):
        # chars requested but the span was not produced with the rawdict option.
        with pytest.raises(ValueError):
            helpers.recover_span_quad((1.0, 0.0), dict(_SPAN), [{"bbox": (0, 0, 1, 1)}])

    def test_span_quad_single_char(self):
        span = dict(_SPAN)
        span["chars"] = [{"bbox": (72.0, 50.5, 86.44, 77.98)}]
        got = _q(helpers.recover_span_quad((1.0, 0.0), span, span["chars"]))
        assert got == (
            (72.0, 50.5),
            (86.44, 50.5),
            (72.0, 77.98),
            (86.44, 77.98),
        )

    def test_line_quad_multi_span(self):
        line = {
            "dir": (1.0, 0.0),
            "spans": [
                dict(_SPAN),
                dict(_SPAN, bbox=(180.0, 50.5, 280.0, 77.98)),
            ],
        }
        got = _q(helpers.recover_line_quad(line))
        # spans[0].ll .. spans[-1].lr span the whole line.
        assert got[0] == (72.0, 50.5) and got[3] == (280.0, 77.98)

    def test_line_quad_empty_spans_raises(self):
        with pytest.raises(ValueError):
            helpers.recover_line_quad({"dir": (1.0, 0.0), "spans": []})


# --------------------------------------------------------------------------- #
# get_pdf_str control-character escapes
# --------------------------------------------------------------------------- #
class TestGetPdfStr:
    def test_control_char_escapes(self):
        assert helpers.get_pdf_str("\x08") == "(\\b)"
        assert helpers.get_pdf_str("\x0c") == "(\\f)"
        assert helpers.get_pdf_str("\x0d") == "(\\r)"
        # any other control byte collapses to the fixed \267 escape.
        assert helpers.get_pdf_str("\x01") == "(\\267)"
        assert helpers.get_pdf_str("\x08\x0c\x0d\x01") == "(\\b\\f\\r\\267)"


# --------------------------------------------------------------------------- #
# get_pdf_now timezone-sign branches
# --------------------------------------------------------------------------- #
class TestGetPdfNow:
    def test_west_of_utc_uses_minus(self, monkeypatch):
        monkeypatch.setattr(helpers.time, "altzone", 3600)
        assert helpers.get_pdf_now().endswith("-01'00'")

    def test_east_of_utc_uses_plus(self, monkeypatch):
        monkeypatch.setattr(helpers.time, "altzone", -3600)
        assert helpers.get_pdf_now().endswith("+01'00'")

    def test_utc_has_no_suffix(self, monkeypatch):
        monkeypatch.setattr(helpers.time, "altzone", 0)
        now = helpers.get_pdf_now()
        assert now.startswith("D:") and len(now) == 16


# --------------------------------------------------------------------------- #
# ConversionHeader XHTML
# --------------------------------------------------------------------------- #
class TestConversionHeader:
    def test_xhtml_header(self):
        h = helpers.ConversionHeader("xhtml")
        assert h == helpers._XHTML_HEADER
        assert "xhtml" in h and h.lstrip().startswith("<?xml")


# --------------------------------------------------------------------------- #
# _make_output destination resolver
# --------------------------------------------------------------------------- #
class TestMakeOutput:
    def test_stream_passthrough(self):
        buf = io.StringIO()
        assert helpers._make_output(stream=buf) is buf

    def test_default_passthrough_when_nothing_set(self):
        sentinel = io.StringIO()
        assert helpers._make_output(default=sentinel) is sentinel

    def test_path_write_and_append(self, tmp_path):
        p = tmp_path / "msg.txt"
        out = helpers._make_output(text=f"path:{p}")
        out.write("first\n")
        out.close()
        out2 = helpers._make_output(text=f"path+{p}")
        out2.write("second\n")
        out2.close()
        assert p.read_text() == "first\nsecond\n"

    def test_fd_target(self, tmp_path):
        p = tmp_path / "fd.txt"
        fd = os.open(str(p), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
        try:
            out = helpers._make_output(text=f"fd:{fd}")
            out.write("viafd\n")
            out.close()  # closefd=False -> the raw fd stays open
        finally:
            os.close(fd)
        assert p.read_text() == "viafd\n"

    def test_bad_text_prefix_raises(self):
        with pytest.raises(AssertionError):
            helpers._make_output(text="nonsense")

    def test_pylogging_logger_kwarg(self, caplog):
        logger = logging.getLogger("pdfspine_helpers_probe_a")
        out = helpers._make_output(
            pylogging_logger=logger, pylogging_level=logging.WARNING
        )
        with caplog.at_level(logging.WARNING, logger="pdfspine_helpers_probe_a"):
            out.write("hello\n")  # trailing newline is stripped before logging
            out.write("\n")  # blank after strip -> not emitted
            out.flush()
        assert [r.getMessage() for r in caplog.records] == ["hello"]

    def test_text_logging_prefix_with_level(self, caplog):
        out = helpers._make_output(
            text=f"logging:level={logging.INFO},name=pdfspine_helpers_probe_b"
        )
        with caplog.at_level(logging.INFO, logger="pdfspine_helpers_probe_b"):
            out.write("world\n")
        assert "world" in caplog.text

    def test_text_logging_prefix_without_level(self, caplog):
        # no level -> the logger's effective level is resolved; the trailing
        # comma yields an empty item that is skipped.
        out = helpers._make_output(text="logging:name=pdfspine_helpers_probe_c,")
        with caplog.at_level(logging.WARNING, logger="pdfspine_helpers_probe_c"):
            out.write("effective\n")
        assert "effective" in caplog.text


# --------------------------------------------------------------------------- #
# log() failure fallbacks
# --------------------------------------------------------------------------- #
class TestLogFallbacks:
    def test_stack_stopiteration_leaves_text_unprefixed(self, monkeypatch):
        buf = io.StringIO()
        helpers.set_log(stream=buf)
        try:

            def _raise(*a, **k):
                raise StopIteration

            monkeypatch.setattr(helpers.inspect, "stack", _raise)
            helpers.log("plain")
        finally:
            helpers.set_log(stream=sys.stdout)
        assert buf.getvalue() == "plain\n"

    def test_relpath_failure_falls_back_to_absolute(self, monkeypatch):
        buf = io.StringIO()
        helpers.set_log(stream=buf)
        try:

            def _raise(path):
                raise ValueError("no relpath across drives")

            monkeypatch.setattr(helpers.os.path, "relpath", _raise)
            helpers.log("diag")
        finally:
            helpers.set_log(stream=sys.stdout)
        out = buf.getvalue()
        assert out.endswith("diag\n")
        assert "test_helpers.py" in out
