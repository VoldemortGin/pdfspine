"""M0 smoke tests for the pdfspine wheel.

These prove the abi3 extension imports and the geometry path is wired through
PyO3. Full API tests arrive with the corresponding milestones.
"""

import pdfspine


def test_version_is_string() -> None:
    assert isinstance(pdfspine.__version__, str)
    assert pdfspine.__version__ != ""


def test_version_function_matches_dunder() -> None:
    # _core.version() is the engine version string; pdfspine.version is the
    # fitz-shaped (VersionBind, VersionFitz, timestamp) tuple.
    assert pdfspine._core.version() == pdfspine.__version__
    assert pdfspine.version == (pdfspine.__version__, pdfspine.__version__, None)


def test_identity_matrix() -> None:
    assert pdfspine.identity_matrix() == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def test_fitz_shim_reexports_version() -> None:
    import fitz

    assert fitz.__version__ == pdfspine.__version__


def test_pymupdf_alias_reexports_version() -> None:
    import pymupdf

    assert pymupdf.__version__ == pdfspine.__version__
