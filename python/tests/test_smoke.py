"""M0 smoke tests for the oxide_pdf wheel.

These prove the abi3 extension imports and the geometry path is wired through
PyO3. Full API tests arrive with the corresponding milestones.
"""

import oxide_pdf


def test_version_is_string() -> None:
    assert isinstance(oxide_pdf.__version__, str)
    assert oxide_pdf.__version__ != ""


def test_version_function_matches_dunder() -> None:
    assert oxide_pdf.version() == oxide_pdf.__version__


def test_identity_matrix() -> None:
    assert oxide_pdf.identity_matrix() == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def test_fitz_shim_reexports_version() -> None:
    import fitz

    assert fitz.__version__ == oxide_pdf.__version__


def test_pymupdf_alias_reexports_version() -> None:
    import pymupdf

    assert pymupdf.__version__ == oxide_pdf.__version__
