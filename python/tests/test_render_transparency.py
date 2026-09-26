"""Transparency rendering parity with PDFium on the USGS fact-sheet poster.

``usgs-fs20183024.pdf`` layers translucent boxes (``ca`` on transparency-group
forms), luminosity soft masks (feathered shadows, the washed-out background
photo), Multiply/Screen blend modes and a rounded-corner image clip. Before
these were rendered, page 1 differed from PDFium by a mean of ~67 levels with
~37% of pixels off by more than 64; after, the residual (~10-17 mean, <3% of
pixels over 64) is CMYK→RGB conversion and anti-aliasing. The thresholds sit
between the two with margin on both sides.

The fixture lives in the gitignored ``fixtures/corpus`` and PyPDFium2 is an
optional oracle, so the test skips when either is missing.
"""

from pathlib import Path

import pytest

import pdfspine

np = pytest.importorskip("numpy")
pdfium = pytest.importorskip("pypdfium2")

_PDF = (
    Path(__file__).resolve().parents[2] / "fixtures" / "corpus" / "usgs-fs20183024.pdf"
)

# Mean absolute difference (max over RGB channels) and share of pixels that
# differ by more than 64 levels, at 72 dpi.
_MAX_MEAN_DIFF = 20.0
_MAX_FAR_SHARE = 0.04


@pytest.mark.skipif(not _PDF.is_file(), reason="fixtures/corpus not fetched")
# Page 6's map is a DeviceN image with a PostScript (type 4) tint transform,
# which is a separate, known gap and not transparency.
@pytest.mark.parametrize("pno", range(5))
def test_usgs_transparency_matches_pdfium(pno: int) -> None:
    pix = pdfspine.open(str(_PDF))[pno].get_pixmap(dpi=72)
    ours = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.width, pix.n)[
        ..., :3
    ]
    oracle_doc = pdfium.PdfDocument(str(_PDF))
    try:
        oracle = np.asarray(oracle_doc[pno].render(scale=1).to_pil().convert("RGB"))
    finally:
        oracle_doc.close()
    assert ours.shape == oracle.shape
    diff = np.abs(ours.astype(np.int16) - oracle.astype(np.int16)).max(axis=2)
    assert diff.mean() <= _MAX_MEAN_DIFF, diff.mean()
    assert (diff > 64).mean() <= _MAX_FAR_SHARE, (diff > 64).mean()
