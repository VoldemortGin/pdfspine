"""Pytest session setup for the pdfspine in-repo suite.

The PyMuPDF compatibility shim is opt-in: a default install no longer claims the
global ``fitz`` / ``pymupdf`` import names (so it stays collision-safe alongside a
real PyMuPDF). Many tests here still do ``import fitz`` / ``import pymupdf`` to
exercise the drop-in surface, so we explicitly opt in once at session start.

This registers :mod:`pdfspine.fitz` / :mod:`pdfspine.pymupdf` under the global
names via :func:`pdfspine.install_fitz_shim`. It uses ``setdefault``, so it never
clobbers a real PyMuPDF that happened to be imported first.
"""

from __future__ import annotations

import pdfspine

pdfspine.install_fitz_shim()
