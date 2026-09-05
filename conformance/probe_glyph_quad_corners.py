#!/usr/bin/env python3
"""Compare pdfspine and PyMuPDF glyph-quad corner conventions.

The two engines use different fallback Helvetica vertical metrics, so their
quad coordinates are not expected to be numerically identical.  This probe
compares the direction of the ``ul -> ur`` and ``ul -> ll`` edges instead:
those two vectors identify whether corners were named in the glyph frame
before transformation or relabelled by their final visual position.

The probe uses only in-memory, self-generated PDFs.  A PyMuPDF 1.28.2 oracle
can be prepared with::

    uv venv .venv-oracle --python 3.12
    uv pip install --python .venv-oracle/bin/python pymupdf==1.28.2

Then run::

    .venv/bin/python conformance/probe_glyph_quad_corners.py
"""

import argparse
import json
import math
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
CASES = (
    ("upright", b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj ET", None),
    ("shear", b"BT /F1 1 Tf 12 0 6 12 100 700 Tm (A) Tj ET", None),
    ("run_rotate_90", b"BT /F1 1 Tf 0 12 -12 0 100 700 Tm (A) Tj ET", None),
    (
        "page_rotate_90_upright_run",
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj ET",
        90,
    ),
    (
        "page_rotate_90_rotated_run",
        b"BT /F1 1 Tf 0 12 -12 0 100 700 Tm (A) Tj ET",
        90,
    ),
)


def _build_pdf(content: bytes, rotate: int | None) -> bytes:
    page_extra = b"" if rotate is None else f" /Rotate {rotate}".encode()
    widths = b"[" + b" ".join(b"500" for _ in range(94)) + b"]"
    font = (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 125 /Widths "
        + widths
        + b" >>"
    )
    objects = (
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]"
            + page_extra
            + b" /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (
            4,
            b"<< /Length "
            + str(len(content)).encode()
            + b" >>\nstream\n"
            + content
            + b"\nendstream",
        ),
        (5, font),
    )

    output = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    for number, body in objects:
        offsets[number] = len(output)
        output += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"

    startxref = len(output)
    output += b"xref\n0 6\n0000000000 65535 f \n"
    for number in range(1, 6):
        output += f"{offsets[number]:010} 00000 n \n".encode()
    output += (
        b"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n"
        + str(startxref).encode()
        + b"\n%%EOF\n"
    )
    return bytes(output)


def _worker(engine: str) -> int:
    if engine == "pdfspine":
        import pdfspine as pdf_engine

        version = pdf_engine.VersionBind
    else:
        import pymupdf as pdf_engine

        version = pdf_engine.VersionBind

    results = []
    for name, content, rotate in CASES:
        kwargs = {"stream": _build_pdf(content, rotate)}
        if engine == "pymupdf":
            kwargs["filetype"] = "pdf"
        document = pdf_engine.open(**kwargs)
        page = document[0]
        char = ET.fromstring(page.get_text("xml")).find(".//char")
        if char is None:
            raise RuntimeError(f"{engine} emitted no XML char for {name}")
        results.append(
            {
                "name": name,
                "page_rotation": page.rotation,
                "quad": [float(value) for value in char.attrib["quad"].split()],
                "origin": [float(char.attrib["x"]), float(char.attrib["y"])],
            }
        )
    print(json.dumps({"engine": engine, "version": version, "cases": results}))
    return 0


def _vector(quad: list[float], start: int, end: int) -> tuple[float, float]:
    return quad[end] - quad[start], quad[end + 1] - quad[start + 1]


def _unit(vector: tuple[float, float]) -> tuple[float, float]:
    length = math.hypot(*vector)
    if length == 0:
        raise AssertionError("degenerate quad edge")
    return vector[0] / length, vector[1] / length


def _dot(left: tuple[float, float], right: tuple[float, float]) -> float:
    return left[0] * right[0] + left[1] * right[1]


def _run(python: Path, engine: str) -> dict:
    process = subprocess.run(
        [str(python), str(Path(__file__).resolve()), "--worker", engine],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(process.stdout)


def _case_map(payload: dict) -> dict[str, dict]:
    return {case["name"]: case for case in payload["cases"]}


def _compare(pdfspine_payload: dict, oracle_payload: dict) -> None:
    pdfspine_cases = _case_map(pdfspine_payload)
    oracle_cases = _case_map(oracle_payload)

    for name in ("upright", "shear", "run_rotate_90"):
        ours = pdfspine_cases[name]["quad"]
        oracle = oracle_cases[name]["quad"]
        for start, end, label in ((0, 2, "ul->ur"), (0, 4, "ul->ll")):
            ours_direction = _unit(_vector(ours, start, end))
            oracle_direction = _unit(_vector(oracle, start, end))
            agreement = _dot(ours_direction, oracle_direction)
            if agreement < 0.999:
                raise AssertionError(
                    f"{name} {label} corner direction differs: dot={agreement}"
                )

    rotated = oracle_cases["run_rotate_90"]["quad"]
    if not (_vector(rotated, 0, 2)[1] < 0 and _vector(rotated, 0, 4)[0] > 0):
        raise AssertionError("oracle relabelled the rotated quad by visual position")

    for plain, page_rotated in (
        ("upright", "page_rotate_90_upright_run"),
        ("run_rotate_90", "page_rotate_90_rotated_run"),
    ):
        if oracle_cases[plain]["quad"] != oracle_cases[page_rotated]["quad"]:
            raise AssertionError("PyMuPDF XML unexpectedly applied page /Rotate")
        if pdfspine_cases[plain]["quad"] == pdfspine_cases[page_rotated]["quad"]:
            raise AssertionError("pdfspine unexpectedly ignored page /Rotate")
        if oracle_cases[page_rotated]["page_rotation"] != 90:
            raise AssertionError("PyMuPDF did not preserve the page /Rotate metadata")
        if pdfspine_cases[page_rotated]["page_rotation"] != 90:
            raise AssertionError("pdfspine did not preserve the page /Rotate metadata")

    print(
        f"corner convention: MATCH "
        f"(pdfspine {pdfspine_payload['version']} vs PyMuPDF {oracle_payload['version']})"
    )
    print(
        "note: coordinates differ because the engines use different Helvetica "
        "vertical metrics; only normalized edge directions are compared"
    )
    print(
        "page /Rotate coordinate basis: DIFFERENT "
        "(PyMuPDF XML stays unrotated; pdfspine applies its page transform)"
    )
    for payload in (pdfspine_payload, oracle_payload):
        print(f"\n{payload['engine']} {payload['version']}")
        for case in payload["cases"]:
            print(f"  {case['name']}: quad={case['quad']} origin={case['origin']}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", choices=("pdfspine", "pymupdf"))
    parser.add_argument(
        "--pdfspine-python",
        type=Path,
        default=REPO_ROOT / ".venv" / "bin" / "python",
    )
    parser.add_argument(
        "--oracle-python",
        type=Path,
        default=REPO_ROOT / ".venv-oracle" / "bin" / "python",
    )
    args = parser.parse_args()
    if args.worker:
        return _worker(args.worker)
    _compare(
        _run(args.pdfspine_python, "pdfspine"),
        _run(args.oracle_python, "pymupdf"),
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
