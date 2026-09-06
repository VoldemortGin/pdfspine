"""OCR-BENCH-* — the CJK+Latin accuracy benchmark's scoring contract, and the
PaddleOCR Latin/CJK regression pins on its clean synthetic scan.

The scoring functions live in ``conformance/ocr/run_ocr_bench.py`` and are
applied identically to both engines (PaddleOCR and Tesseract); these tests pin
that contract so a future "PaddleOCR beats/loses to Tesseract" claim cannot be a
scoring artifact. The engine tests run the same ``Page.insert_image`` →
``get_textpage_ocr(dpi=150, engine="paddle")`` pipeline as the benchmark on
``conformance/ocr/images/scan_00.png`` (no blur, no noise) and are skipped on a
lean build without the ``ocr`` feature.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest

from python.tests.test_ocr_paddle import _requires_paddle

_ROOT = Path(__file__).resolve().parents[2]
_OCR_DIR = _ROOT / "conformance" / "ocr"
_CLEAN_SCAN = "images/scan_00.png"


def _bench():
    spec = importlib.util.spec_from_file_location(
        "_pdfspine_ocr_bench_test", _OCR_DIR / "run_ocr_bench.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _clean_entry() -> dict:
    manifest = json.loads((_OCR_DIR / "manifest.json").read_text(encoding="utf-8"))
    (entry,) = [d for d in manifest["docs"] if d["image"] == _CLEAN_SCAN]
    assert entry["blur"] == 0 and entry["noise"] == 0
    return entry


# --- OCR-BENCH-001: Latin tokenization is script-blind and case-folded -----


def test_latin_tokens_split_on_cjk_and_punctuation_and_lowercase():
    bench = _bench()
    assert bench._latin_tokens("大街Beijing 100190, PaddleOCR v4!") == [
        "beijing",
        "100190",
        "paddleocr",
        "v4",
    ]
    assert bench._latin_tokens("纯中文") == []


# --- OCR-BENCH-002: the Latin metric credits found tokens, ignores noise ----


def test_latin_acc_ignores_extra_tokens_regardless_of_their_script():
    bench = _bench()
    truth = "Beijing 100190"
    # Tesseract-style ASCII garbage for CJK glyphs and PaddleOCR-style real CJK
    # text score identically: neither is charged against the Latin tokens.
    assert bench._latin_acc("RUSTSCEM Beijing 100190 RZEMETASARATTZ", truth) == 1.0
    assert bench._latin_acc("北京市海淀区中关村大街 Beijing 100190", truth) == 1.0
    assert bench._latin_acc("", truth) == 0.0
    assert bench._latin_acc("anything", "中文") == 1.0


def test_latin_misses_record_best_match_behind_the_score():
    bench = _bench()
    truth = "invoice no. A1938"
    pred = "invoice no.A19w"
    assert bench._latin_misses(pred, truth) == [{"truth": "a1938", "best": "a19w", "sim": 0.6}]
    matches = bench._latin_matches(pred, truth)
    assert [t for t, _, _ in matches] == ["invoice", "no", "a1938"]
    assert bench._latin_acc(pred, truth) == pytest.approx(sum(s for _, _, s in matches) / 3)


# --- OCR-BENCH-003: CJK is character accuracy over the pure-CJK streams ----


def test_char_acc_over_cjk_streams():
    bench = _bench()
    truth = bench._cjk_only("机器学习与深度神经网络 PaddleOCR v4 model")
    assert truth == "机器学习与深度神经网络"
    assert bench._char_acc(bench._cjk_only("机器学习与深度神经络"), truth) == pytest.approx(1 - 1 / 11)
    assert bench._char_acc("", "") == 1.0


# --- OCR-BENCH-004: PaddleOCR on the clean synthetic scan -------------------


def _paddle_text_for_clean_scan(bench) -> str:
    doc = bench._scanned_pdf((_OCR_DIR / _CLEAN_SCAN).read_bytes())
    return bench._ocr_text(doc, "paddle")


@_requires_paddle
def test_paddle_clean_scan_cjk_is_exact():
    bench = _bench()
    text = _paddle_text_for_clean_scan(bench)
    assert bench._cjk_only(text) == bench._cjk_only(_clean_entry()["cjk_text"])


@_requires_paddle
@pytest.mark.xfail(
    strict=True,
    reason=(
        "ocrspine 732975f right-pads recognition crops with BLACK (-1.0 after "
        "normalization) instead of PaddleOCR's 0.0 (mid-gray); the black pad reads "
        "as ink and garbles the tail of Latin lines (docs/BENCHMARKS.md §6). Drop "
        "this marker once pdf-ocr pins an ocrspine rev with a gray pad."
    ),
)
def test_paddle_clean_scan_latin_is_exact():
    bench = _bench()
    text = _paddle_text_for_clean_scan(bench)
    misses = bench._latin_misses(text, _clean_entry()["latin_text"])
    assert misses == [], f"Latin misses on the clean scan: {misses!r}\n{text!r}"
