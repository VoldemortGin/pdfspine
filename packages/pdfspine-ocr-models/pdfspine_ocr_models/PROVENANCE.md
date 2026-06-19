# `pdfspine-ocr-models` — bundled OCR model provenance

This data distribution redistributes the *permissively-licensed* PaddleOCR PP-OCR
model weights (converted to ONNX) that pdfspine's pure-Rust PaddleOCR engine loads
at runtime. It is the data companion that the `pdfspine[ocr]` extra pulls in; the
models are the same files tracked in the pdfspine repo at `crates/pdf-ocr/models/`
(the build force-includes them from there, it does not keep a second copy).

The project thesis is **license cleanliness**: every redistributed byte has a
recorded, affirmatively-permissive license and a recorded upstream source.

All three files originate from the **PaddleOCR** project
(<https://github.com/PaddlePaddle/PaddleOCR>), which — including its published
PP-OCR model weights — is distributed under the **Apache License, Version 2.0**
(SPDX: `Apache-2.0`), compatible with this project's Apache-2.0 license. The
required attribution that must accompany binary distributions is carried in the
[`NOTICE`](./NOTICE) shipped alongside this file.

> **Conversion note.** The upstream PaddleOCR weights are published in
> PaddlePaddle's native inference format. The bundled `*.onnx` files were
> converted to ONNX with [Paddle2ONNX](https://github.com/PaddlePaddle/Paddle2ONNX)
> so they can be loaded by the pure-Rust `tract-onnx` runtime (no `onnxruntime` /
> C dependency). Conversion is a mechanical format transcode; it does not change
> the licensing of the weights.

## `ppocrv4_det.onnx` — PP-OCRv4 text detection (DBNet)

| field | value |
|---|---|
| **What** | DBNet text-detection model. Input `[1,3,H,W]`, output probability map `[1,1,H,W]`. |
| **Upstream model** | PP-OCRv4 mobile/server detection (`PP-OCRv4_det`). |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX (no weight modification). |

## `ppocrv4_rec.onnx` — PP-OCRv4 text recognition (CRNN + CTC)

| field | value |
|---|---|
| **What** | CRNN + CTC recognition model. Input `[1,3,48,W]`, output logits `[1,T,6625]`. |
| **Upstream model** | PP-OCRv4 recognition (`PP-OCRv4_rec`), index-aligned to the embedded `ppocr_keys_v4.txt`. |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX (no weight modification). |

## `ppocrv2_cls.onnx` — PP-OCRv2 angle classifier (180°)

| field | value |
|---|---|
| **What** | 180° text-angle classifier. Input concrete `[1,3,48,192]`, output `[1,2]`. |
| **Upstream model** | PP-OCRv2 direction classifier (`ch_ppocr_mobile_v2.0_cls`). |
| **Upstream** | <https://github.com/PaddlePaddle/PaddleOCR> |
| **License** | **Apache-2.0** (PaddlePaddle Authors). SPDX: `Apache-2.0`. |
| **Conversion** | PaddlePaddle inference model → ONNX via Paddle2ONNX (no weight modification). |

> **Note.** The ~26 KB recognition character dictionary (`ppocr_keys_v4.txt`) is
> NOT in this companion — it stays embedded in the pdfspine wheel via
> `include_str!` (see `crates/pdf-ocr/src/paddle/model.rs`). Only the multi-MB
> ONNX weights ship here.
