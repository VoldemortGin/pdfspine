#!/usr/bin/env python3
"""Render FinTabNet.c pages, overlay ONNX layout/table output and gold boxes.

Eyeball harness for the ONNX vision backend (`find_layout()` /
`get_layout_html()` / `find_tables(strategy="vision", backend="onnx")`):
renders each requested page, draws predicted layout blocks + table cells on
one PNG and the FinTabNet.c gold table/cell boxes on another, dumps the
layout HTML, and writes per-page structural stats (rows/cols, IoU-free bbox
comparison inputs, unclaimed-word counts) to a JSON summary. Requires the
`[onnx]` extra and `PDFSPINE_ONNX_MODELS` set (see docs/guide/layout-html.md)
plus the FinTabNet.c corpus fetched by `conformance/gt/fetch_fintabnet.py`.

    .venv/bin/python scripts/onnx_vis.py --out /tmp/onnx-vis
    .venv/bin/python scripts/onnx_vis.py --out /tmp/onnx-vis --pages ADBE_2011_page_118

See docs/onnx-backend-baseline-2026-09-08.md for the first recorded run.
"""

from __future__ import annotations

import argparse
import html as htmlmod
import json
from pathlib import Path
import re
import time

from PIL import Image, ImageDraw, ImageFont

import pdfspine

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "conformance" / "gt" / "corpus-fintabnet"
DEFAULT_PAGES = ["ADBE_2011_page_118", "ADI_2010_page_51", "AMP_2015_page_94"]

LABEL_COLORS = {
    "title": (220, 20, 60),
    "plain text": (30, 144, 255),
    "abandon": (128, 128, 128),
    "figure": (255, 140, 0),
    "figure_caption": (255, 165, 0),
    "table": (0, 160, 0),
    "table_caption": (0, 200, 120),
    "table_footnote": (0, 128, 128),
    "isolate_formula": (148, 0, 211),
    "formula_caption": (186, 85, 211),
}
CELL_COLOR = (200, 0, 200)
SPAN_COLOR = (255, 0, 0)
DET_COLOR = (0, 100, 0)
GOLD_TABLE = (0, 0, 255)
GOLD_CELL = (255, 100, 0)
GOLD_SPAN = (255, 0, 0)


def font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for candidate in (
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ):
        try:
            return ImageFont.truetype(candidate, size)
        except OSError:
            continue
    return ImageFont.load_default()


FONT = font(14)
FONT_SMALL = font(11)


def rect_of(r: object) -> list[float]:
    if hasattr(r, "x0"):
        return [r.x0, r.y0, r.x1, r.y1]  # type: ignore[attr-defined]
    return [float(v) for v in list(r)[:4]]  # type: ignore[call-overload]


def label_text(
    draw: ImageDraw.ImageDraw,
    xy: tuple[float, float],
    text: str,
    color: tuple[int, int, int],
    fnt: object = FONT,
) -> None:
    x, y = xy
    tw, th = draw.textbbox((0, 0), text, font=fnt)[2:]
    y0 = max(0, y - th - 2)
    draw.rectangle([x, y0, x + tw + 4, y0 + th + 2], fill=color)
    draw.text((x + 2, y0 + 1), text, fill=(255, 255, 255), font=fnt)


def unclaimed_text(html: str) -> str:
    m = re.search(r'<pre class="unclaimed">(.*?)</pre>', html, re.S)
    return htmlmod.unescape(m.group(1)) if m else ""


def legend(d: ImageDraw.ImageDraw, height: int, kind: str) -> None:
    items = (
        list(LABEL_COLORS.items())
        + [
            ("cell (thin)", CELL_COLOR),
            ("merged cell (thick)", SPAN_COLOR),
            ("Table.bbox", (0, 0, 0)),
            ("detection_bbox", DET_COLOR),
        ]
        if kind == "pred"
        else [
            ("gold table", GOLD_TABLE),
            ("gold cell", GOLD_CELL),
            ("gold merged", GOLD_SPAN),
        ]
    )
    x, y = 8, height - 17 * len(items) - 8
    for name, color in items:
        d.rectangle([x, y, x + 14, y + 14], fill=color)
        d.text((x + 20, y), name, fill=(0, 0, 0), font=FONT_SMALL)
        y += 17


def process(doc_id: str, out: Path, dpi: int, variant: str | None = None) -> dict:
    scale = dpi / 72.0

    def px(box: object) -> tuple[float, float, float, float]:
        x0, y0, x1, y1 = (float(v) for v in rect_of(box))
        return x0 * scale, y0 * scale, x1 * scale, y1 * scale

    t0 = time.time()
    pdf = CORPUS / "pdfs" / f"{doc_id}.pdf"
    gold = json.loads((CORPUS / "annotations" / f"{doc_id}_tables.json").read_text())
    doc = pdfspine.open(str(pdf))
    page = doc[0]
    info: dict = {"doc_id": doc_id, "page_rect": rect_of(page.rect)}

    options = {"layout_variant": variant} if variant else {}
    blocks = page.find_layout(**options)
    html = page.get_layout_html(**options)
    tables = list(
        page.find_tables(strategy="vision", backend="onnx", vision_options=options)
    )
    (out / f"{doc_id}.out.html").write_text(html, encoding="utf-8")

    info["blocks"] = [
        {
            "label": b.label,
            "raw_label": b.raw_label,
            "score": round(b.score, 3),
            "bbox": [round(v, 1) for v in rect_of(b.bbox)],
        }
        for b in blocks
    ]
    info["tables"] = []
    for t in tables:
        cells = t.cells
        n_cells = sum(1 for c in cells if c is not None)
        spans = t.spans
        n_span = sum(1 for (_, _, rs, cs, _) in spans if rs > 1 or cs > 1)
        md = dict(t.metadata or {})
        info["tables"].append(
            {
                "bbox": [round(v, 1) for v in rect_of(t.bbox)],
                "row_count": t.row_count,
                "col_count": t.col_count,
                "n_cells_nonnull": n_cells,
                "n_cells_total": len(cells),
                "n_spans_entries": len(spans),
                "n_merged": n_span,
                "confidence": t.confidence,
                "detection_bbox": (
                    [round(float(v), 1) for v in md.get("detection_bbox", [])]
                    if md.get("detection_bbox")
                    else None
                ),
                "recognition_crop_bbox": (
                    [round(float(v), 1) for v in md.get("recognition_crop_bbox", [])]
                    if md.get("recognition_crop_bbox")
                    else None
                ),
                "metadata_keys": sorted(str(k) for k in md),
            }
        )
    unc = unclaimed_text(html)
    info["unclaimed_words"] = len(unc.split())
    info["unclaimed_text"] = unc[:600]
    info["html_len"] = len(html)
    info["html_tables"] = html.count("<table")
    info["html_tags"] = {
        tag: len(re.findall(rf"<{tag}[\s>]", html))
        for tag in ("h2", "p", "table", "pre", "figure")
    }

    # Gold summary + orientation check: do gold cell bboxes contain the words they claim?
    words = page.get_text("words")
    info["n_words"] = len(words)
    gold_info = []
    for gt in gold:
        rows: set[int] = set()
        cols: set[int] = set()
        n_span = 0
        ok = 0
        checked = 0
        for c in gt["cells"]:
            rows.update(c["row_nums"])
            cols.update(c["column_nums"])
            if len(c["row_nums"]) > 1 or len(c["column_nums"]) > 1:
                n_span += 1
            gx0, gy0, gx1, gy1 = c["pdf_bbox"]
            inside = [
                w[4]
                for w in words
                if gx0 <= (w[0] + w[2]) / 2 <= gx1 and gy0 <= (w[1] + w[3]) / 2 <= gy1
            ]
            gt_text = (
                c.get("json_text_content") or c.get("pdf_text_content") or ""
            ).strip()
            if gt_text:
                checked += 1
                if " ".join(inside).replace(" ", "") == gt_text.replace(" ", ""):
                    ok += 1
        gold_info.append(
            {
                "bbox": [round(v, 1) for v in gt["pdf_table_bbox"]],
                "rows": len(rows),
                "cols": len(cols),
                "n_cells": len(gt["cells"]),
                "n_merged": n_span,
                "text_match": f"{ok}/{checked}",
            }
        )
    info["gold"] = gold_info

    pm = page.get_pixmap(dpi=dpi)
    render_path = out / f"_{doc_id}_render.png"
    pm.save(str(render_path))
    base = Image.open(render_path).convert("RGB")
    render_path.unlink()

    vis = base.copy()
    d = ImageDraw.Draw(vis)
    for i, b in enumerate(blocks):
        color = LABEL_COLORS.get(b.label, (0, 0, 0))
        x0, y0, x1, y1 = px(b.bbox)
        d.rectangle([x0, y0, x1, y1], outline=color, width=3)
        label_text(d, (x0, y0), f"{i}:{b.raw_label} {b.score:.2f}", color)
    for ti, t in enumerate(tables):
        md = dict(t.metadata or {})
        if md.get("detection_bbox"):
            x0, y0, x1, y1 = px(md["detection_bbox"])
            d.rectangle([x0, y0, x1, y1], outline=DET_COLOR, width=2)
        if md.get("recognition_crop_bbox"):
            x0, y0, x1, y1 = px(md["recognition_crop_bbox"])
            d.rectangle([x0, y0, x1, y1], outline=(0, 180, 180), width=1)
        x0, y0, x1, y1 = px(t.bbox)
        d.rectangle([x0, y0, x1, y1], outline=(0, 0, 0), width=2)
        label_text(
            d,
            (x1 - 160, y1 + 18),
            f"T{ti} {t.row_count}x{t.col_count}",
            (0, 0, 0),
            FONT_SMALL,
        )
        for _r, _c, rs, cs, cb in t.spans:
            cx0, cy0, cx1, cy1 = px(cb)
            merged = rs > 1 or cs > 1
            d.rectangle(
                [cx0, cy0, cx1, cy1],
                outline=SPAN_COLOR if merged else CELL_COLOR,
                width=3 if merged else 1,
            )
    legend(d, vis.height, "pred")
    vis.save(out / f"{doc_id}.vis.png")

    gimg = base.copy()
    d = ImageDraw.Draw(gimg)
    for gi, gt in enumerate(gold):
        x0, y0, x1, y1 = px(gt["pdf_table_bbox"])
        d.rectangle([x0, y0, x1, y1], outline=GOLD_TABLE, width=3)
        label_text(
            d,
            (x0, y0),
            f"gold T{gi} {gold_info[gi]['rows']}x{gold_info[gi]['cols']}",
            GOLD_TABLE,
        )
        for c in gt["cells"]:
            merged = len(c["row_nums"]) > 1 or len(c["column_nums"]) > 1
            cx0, cy0, cx1, cy1 = px(c["pdf_bbox"])
            d.rectangle(
                [cx0, cy0, cx1, cy1],
                outline=GOLD_SPAN if merged else GOLD_CELL,
                width=3 if merged else 1,
            )
    legend(d, gimg.height, "gold")
    gimg.save(out / f"{doc_id}.gold.png")

    info["elapsed_s"] = round(time.time() - t0, 1)
    return info


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out", type=Path, default=Path.cwd(), help="output directory (default: cwd)"
    )
    parser.add_argument(
        "--pages",
        nargs="+",
        default=DEFAULT_PAGES,
        help="FinTabNet.c doc ids under conformance/gt/corpus-fintabnet (default: 3-page sample)",
    )
    parser.add_argument(
        "--dpi", type=int, default=150, help="render DPI for the PNGs (default: 150)"
    )
    parser.add_argument(
        "--variant",
        choices=("pp_doclayout_l", "pp_doclayoutv3"),
        default=None,
        help="PP-DocLayout variant (default: the backend default, PP-DocLayoutV3)",
    )
    args = parser.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    results = []
    for doc_id in args.pages:
        print("processing", doc_id, flush=True)
        results.append(process(doc_id, args.out, args.dpi, args.variant))
    (args.out / "stats.json").write_text(
        json.dumps(results, indent=1, ensure_ascii=False)
    )
    for r in results:
        print(
            r["doc_id"],
            "blocks",
            len(r["blocks"]),
            "tables",
            len(r["tables"]),
            "gold",
            len(r["gold"]),
            "unclaimed",
            r["unclaimed_words"],
            [(t["row_count"], t["col_count"]) for t in r["tables"]],
            [(g["rows"], g["cols"], g["text_match"]) for g in r["gold"]],
        )


if __name__ == "__main__":
    main()
