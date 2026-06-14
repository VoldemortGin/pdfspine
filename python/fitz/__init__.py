"""``fitz`` compatibility shim for oxipdf.

PyMuPDF is imported as ``import fitz``; this package lets existing code keep that
import while running on oxipdf. In M0 it simply re-exports everything from the
native :mod:`oxipdf` package. The full PyMuPDF-compatible surface (geometry
value types, exception aliases, constants) is built out in M5 (PRD §9.5).
"""

from oxipdf import *  # noqa: F401,F403
from oxipdf import __version__  # noqa: F401
