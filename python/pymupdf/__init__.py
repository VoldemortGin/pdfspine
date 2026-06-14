"""``pymupdf`` alias package for oxipdf.

PyMuPDF can also be imported as ``import pymupdf``; this mirrors the :mod:`fitz`
shim. Full surface lands in M5 (PRD §9.5).
"""

from oxipdf import *  # noqa: F401,F403
from oxipdf import __version__  # noqa: F401
