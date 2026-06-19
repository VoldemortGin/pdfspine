# pdfspine-ocr-models

The PP-OCRv4 ONNX model weights for [pdfspine](https://github.com/VoldemortGin/pdfspine)'s
pure-Rust PaddleOCR engine (`engine="paddle"`).

This is a **pure-data companion distribution**. The published `pdfspine` wheel
already contains the OCR *code* (compiled in), but ships **no models**; this
package supplies the ~16 MB of weights. You normally do not install it directly —
install the extra instead:

```bash
pip install pdfspine[ocr]
```

which pulls this package in. pdfspine then resolves the models at runtime by
reading `pdfspine_ocr_models.models_dir()` and exporting it as the
`PDFSPINE_OCR_MODELS` environment variable for the Rust engine. Everything is
offline — no model download at runtime.

```python
import pdfspine_ocr_models
print(pdfspine_ocr_models.models_dir())  # dir holding the 3 ONNX files
```

## License

Apache-2.0. The redistributed PP-OCR model weights are Copyright (c) PaddlePaddle
Authors (Apache-2.0), converted to ONNX via Paddle2ONNX. See
[`pdfspine_ocr_models/NOTICE`](./pdfspine_ocr_models/NOTICE) and
[`pdfspine_ocr_models/PROVENANCE.md`](./pdfspine_ocr_models/PROVENANCE.md).
