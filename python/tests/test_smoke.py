"""M0 smoke tests for the oxipdf wheel.

These prove the abi3 extension imports and the geometry path is wired through
PyO3. Full API tests arrive with the corresponding milestones.
"""

import oxipdf


def test_version_is_string() -> None:
    assert isinstance(oxipdf.__version__, str)
    assert oxipdf.__version__ != ""


def test_version_function_matches_dunder() -> None:
    assert oxipdf.version() == oxipdf.__version__


def test_identity_matrix() -> None:
    assert oxipdf.identity_matrix() == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def test_fitz_shim_reexports_version() -> None:
    import fitz

    assert fitz.__version__ == oxipdf.__version__


def test_pymupdf_alias_reexports_version() -> None:
    import pymupdf

    assert pymupdf.__version__ == oxipdf.__version__
