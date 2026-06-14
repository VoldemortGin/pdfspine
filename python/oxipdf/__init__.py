"""oxipdf — an MIT-licensed, pure-Rust reimplementation of PyMuPDF (``fitz``).

This is the native, idiomatic-Python package backed by the Rust ``_core``
extension module. In M0 the public surface is intentionally minimal (version
probe + geometry path proof); the full document/page/text API lands in M1+.
"""

from . import _core
from ._core import identity_matrix, version

__version__: str = _core.__version__

__all__ = ["__version__", "version", "identity_matrix"]
