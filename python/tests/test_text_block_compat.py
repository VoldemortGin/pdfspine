"""PyMuPDF-compatible text-block granularity regressions."""

from __future__ import annotations

from pathlib import Path

import pdfspine


_TYPESET_FIXTURES = Path(__file__).resolve().parents[2] / "fixtures" / "typeset"


def _text_block_stats(filename: str) -> list[tuple[int, float, int]]:
    doc = pdfspine.open(str(_TYPESET_FIXTURES / filename))
    stats = []
    for page in doc:
        blocks = [
            block
            for block in page.get_text("blocks")
            if block[6] == 0 and block[4].strip()
        ]
        stats.append(
            (
                len(blocks),
                max((block[3] - block[1] for block in blocks), default=0.0),
                max((len(block[4]) for block in blocks), default=0),
            )
        )
    return stats


def test_compat_block_007_typeset_fixture_counts() -> None:
    # COMPAT-BLOCK-007: counts captured from PyMuPDF 1.28.0. Together these
    # fixtures exercise paragraph leading, same-row cells, indentation, and
    # mixed font sizes without relying on the private customer corpus.
    expected_counts = {
        "typeset-box.pdf": [5],
        "typeset-flow.pdf": [9, 3],
        "typeset-lo-doc.pdf": [4],
        "typeset-lo-slide.pdf": [5],
    }
    limits = {
        "typeset-box.pdf": [(100.0, 50)],
        "typeset-flow.pdf": [(55.0, 200), (20.0, 50)],
        "typeset-lo-doc.pdf": [(50.0, 300)],
        "typeset-lo-slide.pdf": [(40.0, 70)],
    }

    for name, expected in expected_counts.items():
        stats = _text_block_stats(name)
        assert [count for count, _, _ in stats] == expected
        for (_, max_height, max_chars), (height_limit, chars_limit) in zip(
            stats, limits[name], strict=True
        ):
            assert max_height <= height_limit
            assert max_chars <= chars_limit
