# pdfspine-ocr-models

> **Deprecated and archived.** Current pdfspine releases use the shared
> `ocrspine-models` base dependency. This directory is retained only to document
> and support the legacy `pdfspine_ocr_models` fallback; do not publish it.

The PP-OCRv5 ONNX model weights for [pdfspine](https://github.com/VoldemortGin/pdfspine)'s
pure-Rust PaddleOCR engine (`engine="paddle"`).

This is a **pure-data companion distribution**. The published `pdfspine` wheel
already contains the OCR *code* (compiled in), but ships **no models**; this
package supplies the ~16 MB of weights. You normally do not install it directly —
For current releases install pdfspine normally:

```bash
pip install pdfspine
```

which pulls in `ocrspine-models`. pdfspine still recognizes this legacy
package's `models_dir()` when it is already installed, after trying the shared
package first.

```python
import pdfspine_ocr_models
print(pdfspine_ocr_models.models_dir())  # dir holding the 3 ONNX files
```

## License

Apache-2.0. The redistributed PP-OCR model weights are Copyright (c) PaddlePaddle
Authors (Apache-2.0), converted to ONNX via Paddle2ONNX. See
[`pdfspine_ocr_models/NOTICE`](./pdfspine_ocr_models/NOTICE) and
[`pdfspine_ocr_models/PROVENANCE.md`](./pdfspine_ocr_models/PROVENANCE.md).
