#!/usr/bin/env python3
"""Diff-oracle text extractor — run ONLY inside the local ``.venv-oracle``.

This script is invoked as a subprocess by ``run_validation.py`` because PyMuPDF
(AGPL) and pdfminer.six must live in a separate, gitignored venv and must never
be imported into the same interpreter as our Apache-2.0 build. It reads a single
PDF path and prints a JSON object with per-page text from each available oracle::

    {"pymupdf": {"ok": true, "pages": ["...", ...]},
     "pdfminer": {"ok": true, "pages": ["...", ...]}}

PyMuPDF is the primary "fitz" reference; pdfminer.six (MIT) is the secondary.
Oracle output is used only for live, local similarity scoring — it is NEVER
committed, bundled, shipped, or stored as a golden.

Usage::

    .venv-oracle/bin/python conformance/oracle_extract.py <path-to-pdf>
"""

from __future__ import annotations

import json
import sys


def _pymupdf_pages(path: str) -> dict:
    try:
        import fitz  # PyMuPDF
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"import: {type(exc).__name__}: {exc}", "pages": []}
    try:
        pages: list[str] = []
        with fitz.open(path) as doc:
            for page in doc:
                pages.append(page.get_text("text"))
        return {"ok": True, "pages": pages, "version": getattr(fitz, "VersionBind", "")}
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}", "pages": []}


def _pdfminer_pages(path: str) -> dict:
    try:
        from pdfminer.high_level import extract_pages
        from pdfminer.layout import LTTextContainer
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"import: {type(exc).__name__}: {exc}", "pages": []}
    try:
        pages: list[str] = []
        for layout in extract_pages(path):
            parts: list[str] = []
            for element in layout:
                if isinstance(element, LTTextContainer):
                    parts.append(element.get_text())
            pages.append("".join(parts))
        return {"ok": True, "pages": pages}
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}", "pages": []}


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(json.dumps({"error": "usage: oracle_extract.py <pdf>"}))
        return 2
    path = argv[1]
    result = {"pymupdf": _pymupdf_pages(path), "pdfminer": _pdfminer_pages(path)}
    sys.stdout.write(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
