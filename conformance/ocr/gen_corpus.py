#!/usr/bin/env python3
"""Generate a deterministic synthetic SCAN corpus for the OCR accuracy benchmark.

Renders mixed Chinese + Latin + digit lines to PNG with a real CJK font, varying
font size, line count, and adding mild scan-like degradation (blur / gaussian
noise) on some images. Writes the images under ``conformance/ocr/images/`` and a
``manifest.json`` mapping each image to its KNOWN ground-truth text (split into a
CJK stream and a Latin stream so each engine can be scored per-script).

Run in ``.venv-oracle`` (the only venv with Pillow); the pdfspine wheel ships no
Pillow/numpy. The output is fully deterministic, so it is regenerable and small
enough to commit.

    source .venv-oracle/bin/activate
    python conformance/ocr/gen_corpus.py
"""

from __future__ import annotations

import json
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
IMG_DIR = HERE / "images"
MANIFEST = HERE / "manifest.json"

CJK_FONT = "/System/Library/Fonts/STHeiti Light.ttc"
# A monospace-ish Latin face keeps Latin rendering crisp for both engines.
LATIN_FONT = "/System/Library/Fonts/Supplemental/Arial.ttf"

# Each line is (cjk_text, latin_text). Either part may be empty. The two streams
# are concatenated for rendering ("<cjk> <latin>") but scored separately, so the
# CJK accuracy is never contaminated by Latin recognition and vice-versa.
LINES: list[tuple[str, str]] = [
    ("纯Rust实现的文字识别引擎", "pdfspine OCR 2026"),
    ("中文与拉丁混排的扫描文档", "invoice no. A1938"),
    ("发票金额合计为壹万贰仟元", "total USD 12000"),
    ("北京市海淀区中关村大街", "Beijing 100190"),
    ("机器学习与深度神经网络", "PaddleOCR v4 model"),
    ("光学字符识别准确率评测", "accuracy 0.97 score"),
    ("这是一段用于测试的中文文本", "sample line 4827"),
    ("人工智能正在改变世界格局", "AI changes world"),
    ("软件工程实践与质量保证", "build passed 100%"),
    ("数据库索引优化技术要点", "index speed up 3x"),
    ("跨平台桌面应用程序开发", "cross-platform app"),
    ("自然语言处理前沿研究综述", "NLP survey 2025"),
    ("分布式系统的一致性算法", "raft consensus v2"),
    ("高性能计算与并行编程模型", "HPC parallel 64"),
    ("图像分割与目标检测网络", "detect 30 objects"),
    ("操作系统内核调度策略分析", "kernel sched fair"),
]


def _font_cjk(size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(CJK_FONT, size)


def _font_latin(size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(LATIN_FONT, size)


def _add_noise(img: Image.Image, rng: random.Random, sigma: int) -> Image.Image:
    """Add mild salt-ish gaussian noise without numpy (point-wise, deterministic)."""
    px = img.load()
    w, h = img.size
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            n = int(rng.gauss(0, sigma))
            px[x, y] = (
                min(255, max(0, r + n)),
                min(255, max(0, g + n)),
                min(255, max(0, b + n)),
            )
    return img


def _render(
    idx: int,
    rows: list[tuple[str, str]],
    *,
    cjk_size: int,
    latin_size: int,
    blur: float,
    noise: int,
    rng: random.Random,
) -> tuple[str, list[str], list[str]]:
    line_h = int(max(cjk_size, latin_size) * 1.6)
    margin = int(cjk_size * 0.8)
    width = 1100
    height = margin * 2 + line_h * len(rows)
    img = Image.new("RGB", (width, height), (250, 250, 248))
    draw = ImageDraw.Draw(img)

    fc = _font_cjk(cjk_size)
    fl = _font_latin(latin_size)

    cjk_gt: list[str] = []
    latin_gt: list[str] = []
    y = margin
    for cjk, latin in rows:
        x = margin
        if cjk:
            draw.text((x, y), cjk, font=fc, fill=(15, 15, 20))
            cjk_gt.append(cjk)
            x += draw.textlength(cjk, font=fc) + cjk_size
        if latin:
            # baseline-align the smaller latin face to the cjk line
            yoff = max(0, cjk_size - latin_size)
            draw.text((x, y + yoff), latin, font=fl, fill=(15, 15, 20))
            latin_gt.append(latin)
        y += line_h

    if blur > 0:
        img = img.filter(ImageFilter.GaussianBlur(blur))
    if noise > 0:
        img = _add_noise(img, rng, noise)

    name = f"scan_{idx:02d}.png"
    img.save(IMG_DIR / name)
    return name, cjk_gt, latin_gt


def main() -> None:
    IMG_DIR.mkdir(parents=True, exist_ok=True)
    for old in IMG_DIR.glob("*.png"):
        old.unlink()

    rng = random.Random(20260619)
    docs = []
    # 16 images: vary line count (3..6), font size, and degradation.
    configs = [
        # (n_lines, cjk_size, latin_size, blur, noise)
        (3, 40, 30, 0.0, 0),
        (3, 34, 26, 0.6, 0),
        (4, 38, 28, 0.0, 6),
        (4, 30, 24, 0.8, 0),
        (5, 36, 28, 0.0, 0),
        (5, 32, 26, 0.5, 4),
        (6, 34, 26, 0.0, 0),
        (6, 30, 24, 0.0, 8),
        (3, 44, 32, 0.4, 0),
        (4, 40, 30, 0.0, 5),
        (5, 28, 22, 0.0, 0),
        (3, 36, 28, 1.0, 0),
        (4, 34, 26, 0.0, 0),
        (5, 38, 30, 0.3, 6),
        (6, 32, 24, 0.6, 0),
        (4, 42, 32, 0.0, 0),
    ]
    cursor = 0
    for i, (n, cs, ls, blur, noise) in enumerate(configs):
        rows = [LINES[(cursor + j) % len(LINES)] for j in range(n)]
        cursor += n
        name, cjk_gt, latin_gt = _render(
            i, rows, cjk_size=cs, latin_size=ls, blur=blur, noise=noise, rng=rng
        )
        docs.append(
            {
                "image": f"images/{name}",
                "cjk_size": cs,
                "latin_size": ls,
                "blur": blur,
                "noise": noise,
                "cjk_text": "".join(cjk_gt),
                "latin_text": " ".join(latin_gt),
            }
        )

    MANIFEST.write_text(
        json.dumps({"font": CJK_FONT, "n_docs": len(docs), "docs": docs}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(f"wrote {len(docs)} images to {IMG_DIR} and manifest {MANIFEST}")


if __name__ == "__main__":
    main()
