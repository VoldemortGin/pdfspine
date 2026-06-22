# `pdfspine-ocr-models` — bundled OCR model provenance

This data distribution redistributes the *permissively-licensed* PaddleOCR
**PP-OCRv5** model weights (converted to ONNX, then made `tract`-parseable) that
pdfspine's pure-Rust PaddleOCR engine loads at runtime. It is the data companion
that the `pdfspine[ocr]` extra pulls in; the models are the same files tracked in
the pdfspine repo at `crates/pdf-ocr/models/` (the build force-includes them from
there, it does not keep a second copy).

The project thesis is **license cleanliness**: every redistributed byte has a
recorded, affirmatively-permissive license and a recorded upstream source.

All three files originate from the **PaddleOCR** project
(<https://github.com/PaddlePaddle/PaddleOCR>), which — including its published
PP-OCR model weights — is distributed under the **Apache License, Version 2.0**
(SPDX: `Apache-2.0`), compatible with this project's Apache-2.0 license. The
required attribution that must accompany binary distributions is carried in the
[`NOTICE`](./NOTICE) shipped alongside this file.

> **Conversion + strip note.** The upstream PaddleOCR weights are published in
> PaddlePaddle's native inference format. The bundled `*.onnx` files were first
> converted to ONNX with [Paddle2ONNX](https://github.com/PaddlePaddle/Paddle2ONNX)
> (a mechanical format transcode; it does not change the weights or licensing),
> then post-processed by the pdfspine repo's deterministic
> `scripts/strip_onnx_dims.py` (renames illegal `DynamicDimension.N` dims, clears
> `value_info` and output shape hints) so the pure-Rust `tract` runtime can parse
> them. The strip changes no weights — only graph metadata.

## `ppocrv5_det.onnx` — PP-OCRv5 text detection (DBNet)

| field | value |
|---|---|
| **What** | DBNet text-detection model. Input `[1,3,H,W]`, output probability map `[1,1,H,W]`. |
| **Upstream model** | PP-OCRv5 mobile detection (`PP-OCRv5_mobile_det`). |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **Pre-converted source** | <https://huggingface.co/ilaylow/PP_OCRv5_mobile_onnx> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX; then `scripts/strip_onnx_dims.py` (dim cleanup, no weight modification). |

## `ppocrv5_rec.onnx` — PP-OCRv5 text recognition (CRNN + CTC)

| field | value |
|---|---|
| **What** | CRNN + CTC recognition model. Input `[1,3,48,W]`, output softmax probs `[1,T,18385]`. |
| **Upstream model** | PP-OCRv5 mobile recognition (`PP-OCRv5_mobile_rec`), index-aligned to the embedded `ppocr_keys_v5.txt`. |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **Pre-converted source** | <https://huggingface.co/ilaylow/PP_OCRv5_mobile_onnx> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX; then `scripts/strip_onnx_dims.py` (dim cleanup, no weight modification). |

## `ppocrv5_cls.onnx` — PP-OCRv5 text-line orientation classifier (180°)

| field | value |
|---|---|
| **What** | Text-line orientation classifier (PP-LCNet). Input concrete `[1,3,80,160]`, output `[1,2]` (0° / 180°). |
| **Upstream model** | `PP-LCNet_x1_0_textline_ori` (PP-OCRv5 text-line-orientation model). |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **Pre-converted source** | <https://huggingface.co/monkt/paddleocr-onnx> (`preprocessing/textline-orientation/`) |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX; then `scripts/strip_onnx_dims.py` (dim cleanup, no weight modification). |

> **Note.** The ~74 KB recognition character dictionary (`ppocr_keys_v5.txt`) is
> NOT in this companion — it stays embedded in the pdfspine wheel via
> `include_str!` (see `crates/pdf-ocr/src/paddle/model.rs`). Only the multi-MB
> ONNX weights ship here.
