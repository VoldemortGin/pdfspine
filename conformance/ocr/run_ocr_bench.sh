#!/usr/bin/env bash
# One-line reproduce for the OCR accuracy benchmark (PaddleOCR vs Tesseract = fitz's OCR).
#
#   bash conformance/ocr/run_ocr_bench.sh
#
# Step 1 regenerates the deterministic synthetic CJK+Latin SCAN corpus in
# .venv-oracle (the only venv with Pillow). Step 2 runs BOTH engines through the
# pdfspine wheel (.venv) and scores per-script character accuracy vs ground truth.
# Run from the repo ROOT. Commits nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

# 1) (re)generate corpus + manifest (needs Pillow -> .venv-oracle)
source .venv-oracle/bin/activate
python conformance/ocr/gen_corpus.py
deactivate

# 2) run both engines via pdfspine and score (needs the wheel -> .venv)
source .venv/bin/activate
python conformance/ocr/run_ocr_bench.py
