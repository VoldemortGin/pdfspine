#!/usr/bin/env python3
"""OCR accuracy benchmark: pdfspine's PaddleOCR vs Tesseract (= fitz's OCR).

fitz's OCR backend is Tesseract-only, so pdfspine's Tesseract path is exactly
what fitz can do. This harness scores CHARACTER-level accuracy (1 - normalized
Levenshtein distance) of each engine against KNOWN ground truth, separately for
the Chinese (CJK) content and the Latin/digit content of every scanned image in
``conformance/ocr/`` (built by ``gen_corpus.py``).

Tesseract is invoked with its DEFAULT language (``eng``) — the same default fitz
uses — so it has no Chinese model loaded and scores near-zero on CJK, while the
pure-Rust PaddleOCR engine (PP-OCRv4, embedded in the wheel) scores high. That
gap is the quantified win.

Run in ``.venv`` (the pdfspine wheel):

    source .venv/bin/activate
    python conformance/ocr/run_ocr_bench.py
"""

from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path

import pdfspine

HERE = Path(__file__).resolve().parent
MANIFEST = HERE / "manifest.json"
RESULTS = HERE / "results.json"


# --- pure-stdlib PNG -> raw RGB decoder (8-bit truecolor, non-interlaced) ----
# The wheel has no Pillow; insert_image's raw-RGB path needs width*height*3 bytes.


def _png_to_rgb(data: bytes) -> tuple[int, int, bytes]:
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    width = height = bit_depth = color_type = 0
    idat = bytearray()
    pos = 8
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        ctype = data[pos + 4 : pos + 8]
        chunk = data[pos + 8 : pos + 8 + length]
        if ctype == b"IHDR":
            width, height, bit_depth, color_type = struct.unpack(">IIBB", chunk[:10])
        elif ctype == b"IDAT":
            idat += chunk
        elif ctype == b"IEND":
            break
        pos += 12 + length
    assert bit_depth == 8 and color_type == 2, (
        f"expected 8-bit truecolor PNG, got bit_depth={bit_depth} color_type={color_type}"
    )
    raw = zlib.decompress(bytes(idat))
    stride = width * 3
    out = bytearray(width * height * 3)
    prev = bytearray(stride)
    src = 0
    for row in range(height):
        filt = raw[src]
        src += 1
        line = bytearray(raw[src : src + stride])
        src += stride
        if filt == 1:
            for i in range(3, stride):
                line[i] = (line[i] + line[i - 3]) & 0xFF
        elif filt == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif filt == 3:
            for i in range(stride):
                a = line[i - 3] if i >= 3 else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif filt == 4:
            for i in range(stride):
                a = line[i - 3] if i >= 3 else 0
                b = prev[i]
                c = prev[i - 3] if i >= 3 else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        out[row * stride : (row + 1) * stride] = line
        prev = line
    return width, height, bytes(out)


# --- scoring ----------------------------------------------------------------


def _levenshtein(a: str, b: str) -> int:
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def _is_cjk(ch: str) -> bool:
    o = ord(ch)
    return 0x4E00 <= o <= 0x9FFF or 0x3400 <= o <= 0x4DBF


def _cjk_only(s: str) -> str:
    return "".join(c for c in s if _is_cjk(c))


def _latin_tokens(s: str) -> list[str]:
    """Latin/digit alnum tokens, lower-cased (punctuation/whitespace split)."""
    out: list[str] = []
    cur: list[str] = []
    for c in s:
        if c.isascii() and c.isalnum():
            cur.append(c.lower())
        elif cur:
            out.append("".join(cur))
            cur = []
    if cur:
        out.append("".join(cur))
    return out


def _char_acc(pred: str, truth: str) -> float:
    """1 - normalized Levenshtein distance over the concatenated char stream.

    Used for CJK, where ground truth and prediction are both pure-CJK streams
    (non-CJK is stripped from each side), so there is no cross-script noise.
    """
    if not truth:
        return 1.0
    dist = _levenshtein(pred, truth)
    return max(0.0, 1.0 - dist / max(len(truth), 1))


def _latin_acc(pred_text: str, truth_text: str) -> float:
    """Per-token best-match accuracy for the Latin/digit content.

    A mixed CJK+Latin scan run through a CJK-blind engine (Tesseract w/o a
    Chinese model) emits ASCII GARBAGE for the Chinese glyphs, interspersed with
    the genuine Latin tokens. A naive concatenated edit distance would charge
    that CJK-origin noise against the Latin score — unfair to Tesseract, whose
    Latin recognition is actually fine. So Latin is scored token-wise: for each
    ground-truth Latin token, take the best char-similarity over the prediction's
    token set (the closest token it produced), and average. This credits the
    engine for every Latin token it genuinely read and ignores extra noise
    tokens — i.e. it is the metric most FAVORABLE to Tesseract, so the remaining
    gap is real, not a scoring artifact.
    """
    truth = _latin_tokens(truth_text)
    if not truth:
        return 1.0
    pred = _latin_tokens(pred_text)
    if not pred:
        return 0.0
    total = 0.0
    for t in truth:
        best = max(
            (1.0 - _levenshtein(p, t) / max(len(t), len(p), 1)) for p in pred
        )
        total += max(0.0, best)
    return total / len(truth)


# --- engines ----------------------------------------------------------------


def _scanned_pdf(png: bytes) -> pdfspine.Document:
    w, h, rgb = _png_to_rgb(png)
    doc = pdfspine.open()
    page = doc.new_page(width=float(w), height=float(h))
    page.insert_image((0, 0, float(w), float(h)), stream=rgb, width=w, height=h)
    return doc


def _ocr_text(doc: pdfspine.Document, engine: str, dpi: int = 150) -> str:
    # Default language "eng" mirrors fitz's default Tesseract config.
    tp = doc[0].get_textpage_ocr(dpi=dpi, engine=engine)
    return tp.extractText()


def main() -> None:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    docs = manifest["docs"]

    per_doc = []
    agg = {
        "paddle": {"cjk": [], "latin": []},
        "tesseract": {"cjk": [], "latin": []},
    }

    for entry in docs:
        png = (HERE / entry["image"]).read_bytes()
        cjk_truth = _cjk_only(entry["cjk_text"])

        row = {"image": entry["image"]}
        for engine in ("paddle", "tesseract"):
            doc = _scanned_pdf(png)
            try:
                text = _ocr_text(doc, engine)
            except pdfspine.PdfUnsupportedError as e:
                text = ""
                row[f"{engine}_error"] = str(e)
            cjk_acc = _char_acc(_cjk_only(text), cjk_truth)
            latin_acc = _latin_acc(text, entry["latin_text"])
            agg[engine]["cjk"].append(cjk_acc)
            agg[engine]["latin"].append(latin_acc)
            row[f"{engine}_cjk_acc"] = round(cjk_acc, 4)
            row[f"{engine}_latin_acc"] = round(latin_acc, 4)
        per_doc.append(row)
        print(
            f"{entry['image']:>22}  "
            f"paddle[cjk={row['paddle_cjk_acc']:.3f} lat={row['paddle_latin_acc']:.3f}]  "
            f"tess[cjk={row['tesseract_cjk_acc']:.3f} lat={row['tesseract_latin_acc']:.3f}]"
        )

    def mean(xs: list[float]) -> float:
        return round(sum(xs) / len(xs), 4) if xs else 0.0

    summary = {
        "n_docs": len(docs),
        "paddle_cjk_acc": mean(agg["paddle"]["cjk"]),
        "paddle_latin_acc": mean(agg["paddle"]["latin"]),
        "tesseract_cjk_acc": mean(agg["tesseract"]["cjk"]),
        "tesseract_latin_acc": mean(agg["tesseract"]["latin"]),
        "tesseract_lang": "eng (fitz default; no CJK model loaded)",
    }
    out = {"summary": summary, "per_doc": per_doc}
    RESULTS.write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")

    print("\n=== OCR accuracy (mean over corpus) ===")
    print(f"n_docs            : {summary['n_docs']}")
    print(f"PaddleOCR  CJK    : {summary['paddle_cjk_acc']:.3f}")
    print(f"PaddleOCR  Latin  : {summary['paddle_latin_acc']:.3f}")
    print(f"Tesseract  CJK    : {summary['tesseract_cjk_acc']:.3f}  (= fitz default)")
    print(f"Tesseract  Latin  : {summary['tesseract_latin_acc']:.3f}")
    print(f"\nwrote {RESULTS}")


if __name__ == "__main__":
    main()
