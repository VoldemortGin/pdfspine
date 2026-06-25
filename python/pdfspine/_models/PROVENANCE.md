# `pdf-ocr/models` — bundled OCR model provenance

This directory holds the *permissively-licensed* PaddleOCR **PP-OCRv5** model
weights (converted to ONNX, then made `tract`-parseable) and the recognition
character dictionary that the pure-Rust PaddleOCR engine uses. The OCR inference
runs in the sibling `ocrspine` crate: the recognition dictionary is embedded into
the binary at compile time via `include_str!` (see `ocrspine/src/paddle/model.rs`);
the three multi-MB `*.onnx` weights ship as data files (loaded from disk at
runtime — see the model-resolution note in that module). The project thesis is
**license cleanliness**: every bundled byte
must have a recorded, affirmatively-permissive license and a recorded upstream
source. License-uncertain data is **never** bundled.

All four files originate from the **PaddleOCR** project
(<https://github.com/PaddlePaddle/PaddleOCR>), which — including its published
PP-OCR model weights — is distributed under the **Apache License, Version 2.0**
(SPDX: `Apache-2.0`), compatible with this project's Apache-2.0 license. The
required attribution that must accompany binary distributions is carried in the
top-level [`NOTICE`](../../../NOTICE).

> **Conversion + strip note.** The upstream PaddleOCR weights are published in
> PaddlePaddle's native inference format. The bundled `*.onnx` files were first
> converted to ONNX with [Paddle2ONNX](https://github.com/PaddlePaddle/Paddle2ONNX)
> (a mechanical format transcode — it does not change the weights or licensing),
> then post-processed by this repo's deterministic
> [`scripts/strip_onnx_dims.py`](../../../scripts/strip_onnx_dims.py): paddle2onnx
> emits illegal dynamic-dimension names (`DynamicDimension.N`, containing a `.`)
> and `floor(...)` shape-hint expressions in `value_info` that the pure-Rust
> `tract` runtime cannot parse. The strip script renames those dims to legal
> identifiers, clears `value_info`, and clears the output shape hints. **It
> changes no weights** — only graph metadata — so the model is byte-for-byte
> equivalent in behavior; tract 0.21 can then `model_for_read → into_optimized
> → into_runnable → run` with det/rec keeping dynamic input dims. The same
> script also bakes the recognition dictionary (its `dict` sub-command).

The pre-conversion sources (all Apache-2.0):

* det / rec: <https://huggingface.co/ilaylow/PP_OCRv5_mobile_onnx>
  (`ppocrv5_det.onnx`, `ppocrv5_rec.onnx` — the PP-OCRv5 *mobile* models).
* cls (text-line orientation): <https://huggingface.co/monkt/paddleocr-onnx>
  (`preprocessing/textline-orientation/PP-LCNet_x1_0_textline_ori.onnx`).
* rec dictionary: <https://huggingface.co/monkt/paddleocr-onnx>
  (`languages/chinese/dict.txt` — 18383 characters).

## `ppocrv5_det.onnx` — PP-OCRv5 text detection (DBNet)

| field | value |
|---|---|
| **What** | DBNet text-detection model. Input `[1,3,H,W]`, output probability map `[1,1,H,W]`. |
| **Upstream model** | PP-OCRv5 mobile detection (`PP-OCRv5_mobile_det`). |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **Pre-converted source** | <https://huggingface.co/ilaylow/PP_OCRv5_mobile_onnx> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX; then `scripts/strip_onnx_dims.py` (dim-name/shape-hint cleanup, no weight modification). |
| **Cleared by** | pdfspine maintainers — Apache-2.0 is in the permitted set. |

## `ppocrv5_rec.onnx` — PP-OCRv5 text recognition (CRNN + CTC)

| field | value |
|---|---|
| **What** | CRNN + CTC recognition model. Input `[1,3,48,W]`, output softmax probs `[1,T,18385]`. |
| **Upstream model** | PP-OCRv5 mobile recognition (`PP-OCRv5_mobile_rec`), index-aligned to `ppocr_keys_v5.txt`. |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **Pre-converted source** | <https://huggingface.co/ilaylow/PP_OCRv5_mobile_onnx> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX; then `scripts/strip_onnx_dims.py` (dim-name/shape-hint cleanup, no weight modification). |
| **Cleared by** | pdfspine maintainers — Apache-2.0 is in the permitted set. |

## `ppocrv5_cls.onnx` — PP-OCRv5 text-line orientation classifier (180°)

| field | value |
|---|---|
| **What** | Text-line orientation classifier (PP-LCNet). Input concrete `[1,3,80,160]`, output `[1,2]` (0° / 180°). |
| **Upstream model** | `PP-LCNet_x1_0_textline_ori` (PP-OCRv5 default text-line-orientation model). |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **Pre-converted source** | <https://huggingface.co/monkt/paddleocr-onnx> (`preprocessing/textline-orientation/`) |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX; then `scripts/strip_onnx_dims.py` (dim-name/shape-hint cleanup, no weight modification). |
| **Cleared by** | pdfspine maintainers — Apache-2.0 is in the permitted set. |
| **Variant note** | We ship the `x1_0` variant (≈6.8 MB, 98.85% acc per upstream). The smaller `x0_25` default was not available pre-converted to ONNX from a trusted Apache-2.0 source; converting it would require the full PaddlePaddle toolchain and produce a tract-unverified artifact. Orientation is a best-effort refinement, so `x1_0` (verified to load + run in tract 0.21) is the safe, accurate choice. |

## `ppocr_keys_v5.txt` — PP-OCRv5 recognition character dictionary

| field | value |
|---|---|
| **What** | Recognition character dictionary, **index-aligned** to the `ppocrv5_rec.onnx` output class axis (line 0 = CTC blank, lines 1..18383 = characters, last line = a single space). Total 18385 lines = the rec model's output width. |
| **Upstream** | <https://huggingface.co/monkt/paddleocr-onnx> (`languages/chinese/dict.txt`, 18383 characters — PP-OCRv5 character set covering Simplified/Traditional Chinese, Japanese, Latin, digits, and symbols). |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Baking** | The raw 18383-line dict is baked to the index-aligned form (prepend `blank`, append a space line → 18385 lines) by `scripts/strip_onnx_dims.py dict`, matching `CharTable::load()`'s expectation. |
| **Cleared by** | pdfspine maintainers — Apache-2.0 is in the permitted set. |

See the top-level [`NOTICE`](../../../NOTICE) for the attribution that must
accompany binary distributions of these bundled models.
