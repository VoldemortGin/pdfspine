#!/usr/bin/env python3
"""Born-digital CJK (Chinese) multi-column PDF generator with reading-order GT.

Companion to ``born_digital.py``: same objective ground-truth philosophy, but the
document body is neutral, self-contained Simplified-Chinese prose (NOT fetched
from any network source). This validates pdfspine's CJK CMap (CID->Unicode)
support — does pdfspine extract Chinese characters correctly, on par with fitz?

Why the ground truth is trustworthy (identical reasoning to born_digital.py):
    Paragraphs are laid out with CSS multi-column (``column-count: N``,
    ``column-fill: auto``). The browser FLOWS content column-major: it fills
    column 1 top-to-bottom, then column 2. So the human reading order ==
    the source DOM order == our ground truth. Chrome may serialize the PDF
    content stream row-major across columns even though a human reads
    column-major — that mismatch is the reading-order extraction challenge, and
    our ground truth tells the right (column-major) answer.

The Chrome render path (``_render_pdf`` + ``_kill_tree`` + ``_chrome_cmd`` +
``_find_chrome``) is REUSED verbatim from ``born_digital.py`` by import, so the
hang-on-exit handling (poll for a stable ``%PDF`` then SIGKILL the process group)
is shared, not duplicated.

Ground truth = the source paragraphs in reading order, joined by blank lines.
Nothing the page does not contain is ever added.

CLI::

    .venv/bin/python conformance/gt/born_cjk.py --out conformance/gt/corpus-cjk

Self-test (no args)::

    .venv/bin/python conformance/gt/born_cjk.py --self-test
"""

from __future__ import annotations

import argparse
import html
import json
import sys
import tempfile
from pathlib import Path

# Reuse the exact Chrome-render helpers from the sibling born_digital module so
# the hang-on-exit (poll-for-stable-PDF then SIGKILL the process group) handling
# is shared, not re-implemented.
GT_DIR = Path(__file__).resolve().parent
if str(GT_DIR) not in sys.path:
    sys.path.insert(0, str(GT_DIR))

from born_digital import _find_chrome, _render_pdf  # noqa: E402

# --------------------------------------------------------------------------- #
# Neutral, self-contained Simplified-Chinese prose. NOT fetched from anywhere.
# Each entry is one paragraph; horizontal writing, plain everyday sentences.
# --------------------------------------------------------------------------- #
_CJK_PARAGRAPHS = [
    "春天来了，花儿开放，鸟儿歌唱。绿色的树叶在温暖的风中轻轻摇动。",
    "夏天的阳光明亮而温暖，孩子们在公园里快乐地玩耍。湖水清澈，倒映着蓝天和白云。",
    "秋天到了，树叶渐渐变成金黄色，慢慢飘落到地上。农民们在田野里收获成熟的庄稼。",
    "冬天下起了大雪，大地变成一片洁白。人们穿上厚厚的衣服，围坐在温暖的火炉旁边。",
    "清晨的露水挂在草尖上，在阳光下闪闪发光。远处连绵的山峦笼罩在薄薄的晨雾之中。",
    "夜晚的天空繁星点点，明亮的月亮高高地挂在空中。微风缓缓吹过，带来阵阵花香。",
    "每天坚持读书可以增长知识，开阔眼界。只要不断努力学习，就会一点一点地进步。",
    "朋友之间应该互相帮助，真诚地对待彼此。真挚的友谊是人生中非常宝贵的财富。",
    "大海宽广无边，波浪一层接着一层涌向岸边。海鸥在天空中自由自在地飞翔鸣叫。",
    "城市里高楼林立，街道上车水马龙。公园中绿树成荫，是人们休息散步的好地方。",
]

# Repeat the block enough times to fill two columns on a Letter page.
_REPEAT = 4

# Default variants. Horizontal writing, multi-column (validates reading order).
DEFAULT_VARIANTS = [
    "zh-1col",
    "zh-2col",
    "zh-3col",
]


def _load_paragraphs() -> list[str]:
    """Return the neutral CJK paragraphs repeated to fill multiple columns."""
    paras: list[str] = []
    for _ in range(_REPEAT):
        paras.extend(_CJK_PARAGRAPHS)
    return paras


# --------------------------------------------------------------------------- #
# Ground truth + HTML construction
# --------------------------------------------------------------------------- #
def _ground_truth(paragraphs: list[str]) -> str:
    """Exact source-order reading text: paragraphs joined by blank lines."""
    return "\n\n".join(paragraphs)


def _build_html(paragraphs: list[str], *, columns: int, gap_px: int = 24) -> str:
    """Build a Letter-size, zero-margin, multi-column CJK HTML document.

    Horizontal writing mode. A CJK-capable serif font stack is requested so the
    glyphs render; the embedded font's CMap is what pdfspine must invert back to
    Unicode during extraction.
    """
    paras_html = "\n".join(
        f"    <p>{html.escape(p)}</p>" for p in paragraphs
    )
    # column-count + column-fill:auto makes the browser FLOW paragraphs
    # column-major (fill col 1 top-to-bottom, then col 2, ...).
    return f"""<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<style>
  @page {{
    size: Letter;
    margin: 0;
  }}
  html, body {{
    margin: 0;
    padding: 0;
  }}
  body {{
    font-family: "Songti SC", "STSong", "PingFang SC", "Hiragino Sans GB",
                 "Heiti SC", "Microsoft YaHei", serif;
    font-size: 12pt;
    line-height: 1.7;
    color: #000;
    padding: 0.6in 0.6in 0.6in 0.6in;
    box-sizing: border-box;
    writing-mode: horizontal-tb;
  }}
  .cols {{
    column-count: {columns};
    column-gap: {gap_px}px;
    column-fill: auto;
    height: 9.0in;
  }}
  .cols p {{
    margin: 0 0 10px 0;
    padding: 0;
    text-align: left;
    -webkit-hyphens: none;
    hyphens: none;
    orphans: 2;
    widows: 2;
  }}
</style>
</head>
<body>
  <div class="cols">
{paras_html}
  </div>
</body>
</html>
"""


# --------------------------------------------------------------------------- #
# Variant specs
# --------------------------------------------------------------------------- #
def _variant_spec(name: str) -> dict:
    """Map a variant name to layout parameters. Raises ValueError if unknown."""
    specs: dict[str, dict] = {
        "zh-1col": {"columns": 1, "gap": 24},
        "zh-2col": {"columns": 2, "gap": 24},
        "zh-3col": {"columns": 3, "gap": 24},
    }
    if name not in specs:
        raise ValueError(f"unknown variant {name!r}; known: {', '.join(specs)}")
    return specs[name]


def _has_cjk(s: str) -> bool:
    """True if the string contains a CJK Unified Ideograph (U+4E00..U+9FFF)."""
    return any("一" <= ch <= "鿿" for ch in s)


# --------------------------------------------------------------------------- #
# Public API
# --------------------------------------------------------------------------- #
def generate(out_dir: Path, variants: list[str] | None = None) -> list[dict]:
    """Generate CJK multi-column PDFs with reading-order ground truth.

    Returns a list of manifest entries and also writes ``<out_dir>/manifest.json``.
    Each entry: {name, pdf (abs path), gt_text, columns, lang:"zh"}.
    """
    out_dir = Path(out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    variants = list(variants) if variants else list(DEFAULT_VARIANTS)

    paragraphs = _load_paragraphs()
    print(f"[born_cjk] CJK paragraphs: {len(paragraphs)} "
          f"({len(_CJK_PARAGRAPHS)} unique x{_REPEAT})", file=sys.stderr)

    chrome = _find_chrome()
    if chrome is None:
        print(
            "[born_cjk] DIAGNOSTIC: no working Chrome/Chromium found. "
            "HTML will still be written.",
            file=sys.stderr,
        )
    else:
        print(f"[born_cjk] using chrome: {chrome}", file=sys.stderr)

    entries: list[dict] = []
    for name in variants:
        spec = _variant_spec(name)
        body_html = _build_html(paragraphs, columns=spec["columns"], gap_px=spec["gap"])
        html_path = out_dir / f"{name}.html"
        html_path.write_text(body_html, encoding="utf-8")
        pdf_path = out_dir / f"{name}.pdf"

        gt_text = _ground_truth(paragraphs)
        assert _has_cjk(gt_text), f"{name}: gt_text has no CJK characters"

        if chrome is not None:
            ok, diag = _render_pdf(chrome, html_path, pdf_path)
            if not ok:
                print(f"[born_cjk] DIAGNOSTIC for {name}: {diag}", file=sys.stderr)
            else:
                print(f"[born_cjk] rendered {name} -> {pdf_path} ({diag})",
                      file=sys.stderr)

        entries.append({
            "name": name,
            "pdf": str(pdf_path.resolve()),
            "gt_text": gt_text,
            "columns": spec["columns"],
            "lang": "zh",
        })

    manifest_path = out_dir / "manifest.json"
    manifest_path.write_text(json.dumps(entries, indent=2, ensure_ascii=False),
                             encoding="utf-8")
    print(f"[born_cjk] wrote manifest: {manifest_path.resolve()}", file=sys.stderr)
    return entries


# --------------------------------------------------------------------------- #
# Self-test
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    variants = ["zh-2col"]
    with tempfile.TemporaryDirectory(prefix="pdfspine-gt-cjk-selftest-") as tmp:
        out = Path(tmp) / "corpus-cjk"
        entries = generate(out, variants=variants)

        assert len(entries) == len(variants), f"expected {len(variants)} entries"
        by_name = {e["name"]: e for e in entries}

        for name in variants:
            e = by_name[name]
            assert e["lang"] == "zh", f"{name}: lang must be 'zh'"
            assert e["gt_text"].strip(), f"{name}: gt_text is empty"
            assert _has_cjk(e["gt_text"]), f"{name}: gt_text has no CJK chars"
            pdf = Path(e["pdf"])
            assert pdf.exists(), f"{name}: PDF not created at {pdf}"
            size = pdf.stat().st_size
            assert size > 1024, f"{name}: PDF too small ({size} bytes)"
            head = pdf.read_bytes()[:4]
            assert head == b"%PDF", f"{name}: not a PDF (head={head!r})"

        # Manifest exists and is valid.
        manifest = out / "manifest.json"
        assert manifest.exists(), "manifest.json not written"
        loaded = json.loads(manifest.read_text(encoding="utf-8"))
        assert len(loaded) == len(variants)
        assert all(_has_cjk(m["gt_text"]) for m in loaded), "manifest gt_text lacks CJK"

    print("born_cjk.py self-test OK")
    print("variants:", ", ".join(variants))
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=None,
                    help="output directory for generated PDFs + manifest.json")
    ap.add_argument("--variants", nargs="*", default=None,
                    help=f"subset of variants (default: all = {', '.join(DEFAULT_VARIANTS)})")
    ap.add_argument("--self-test", action="store_true",
                    help="render zh-2col into a temp dir and assert")
    args = ap.parse_args(argv)

    if args.self_test or args.out is None:
        return _self_test()

    entries = generate(args.out, variants=args.variants)
    print(f"Generated {len(entries)} variant(s) into {Path(args.out).resolve()}:")
    for e in entries:
        status = "OK" if Path(e["pdf"]).exists() else "NO-PDF"
        print(f"  [{status}] {e['name']:<10} cols={e['columns']} "
              f"lang={e['lang']} gt_chars={len(e['gt_text'])} -> {e['pdf']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
