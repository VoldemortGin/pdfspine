#!/usr/bin/env python3
"""Born-digital Arabic + mixed-bidi corpus generator with KNOWN logical GT.

We control the HTML source text, so the LOGICAL (source) reading order is exact
ground truth. Chrome shapes + lays out the Arabic (RTL) via HarfBuzz; the PDF it
emits stores shaped glyphs with a ToUnicode/cmap back to base Arabic codepoints.
A correct extractor must return the LOGICAL base-letter string, not visual order
and not presentation-form codepoints (U+FE70–FEFF / U+FB50–FDFF).

Reuses the headless-Chrome HTML->PDF path from ``born_digital.py``. Writes PDFs +
a ``manifest.json`` ({id, pdf, gt_text, kind}) under ``corpus-arabic/``.

Run from repo root::

    .venv/bin/python conformance/gt/born_arabic.py
"""

from __future__ import annotations

import html
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from born_digital import _find_chrome, _render_pdf  # noqa: E402

# --------------------------------------------------------------------------- #
# Corpus: (id, kind, [paragraphs]). Each paragraph is one logical line.
# Logical GT = the source string, exactly as written here (base Arabic letters,
# spaces, ASCII digits/Latin where present). Joined with "\n" between paragraphs.
# kinds: pure-arabic | bidi-digits | bidi-latin | rtl-multiline
# --------------------------------------------------------------------------- #
DOCS: list[tuple[str, str, list[str]]] = [
    (
        "ar-pure-1",
        "pure-arabic",
        [
            "اللغة العربية من أكثر اللغات انتشارا في العالم",
            "يتحدث بها مئات الملايين من الناس حول العالم",
        ],
    ),
    (
        "ar-pure-2",
        "pure-arabic",
        [
            "الشمس تشرق من جهة الشرق وتغرب في جهة الغرب",
            "القمر يضيء السماء في الليل بنوره الجميل",
            "النجوم تتلألأ في سماء الصحراء الصافية",
        ],
    ),
    (
        "ar-pure-3",
        "pure-arabic",
        [
            "العلم نور والجهل ظلام يحجب الطريق",
            "القراءة غذاء العقل وزاد الروح في الحياة",
        ],
    ),
    (
        "ar-bidi-digits-1",
        "bidi-digits",
        [
            # Arabic with embedded ASCII digits — visual != logical order.
            "السعر 100 دولار فقط",
            "اشترى الرجل 25 كتابا من المكتبة",
        ],
    ),
    (
        "ar-bidi-digits-2",
        "bidi-digits",
        [
            "وصل القطار في الساعة 9 صباحا",
            "المسافة بين المدينتين 350 كيلومترا",
            "ولد في عام 1990 في مدينة القاهرة",
        ],
    ),
    (
        "ar-bidi-latin-1",
        "bidi-latin",
        [
            # Arabic line with an embedded Latin token.
            "أستخدم نظام Linux في عملي اليومي",
            "لغة Python سهلة التعلم ومفيدة جدا",
        ],
    ),
    (
        "ar-bidi-latin-2",
        "bidi-latin",
        [
            "شركة Apple أعلنت عن منتج جديد اليوم",
            "موقع GitHub يستضيف ملايين المشاريع البرمجية",
        ],
    ),
    (
        "ar-mixed-line",
        "bidi-latin",
        [
            # A single mixed Arabic + English + digit line.
            "الإصدار رقم 3 من برنامج Office متوفر الآن",
        ],
    ),
    (
        "ar-multiline",
        "rtl-multiline",
        [
            "السطر الأول من النص العربي",
            "السطر الثاني يأتي بعد الأول مباشرة",
            "السطر الثالث يختتم هذه الفقرة القصيرة",
            "السطر الرابع والأخير في هذا المستند",
        ],
    ),
]


def _build_html(paragraphs: list[str]) -> str:
    """Single-column RTL Arabic page. SF Arabic / Geeza Pro embed cleanly."""
    paras = "\n".join(f'    <p>{html.escape(p)}</p>' for p in paragraphs)
    return f"""<!DOCTYPE html>
<html lang="ar" dir="rtl">
<head>
<meta charset="utf-8">
<style>
  @page {{ size: Letter; margin: 0; }}
  html, body {{ margin: 0; padding: 0; }}
  body {{
    font-family: "Geeza Pro", "SF Arabic", "Arial", sans-serif;
    font-size: 18pt;
    line-height: 1.8;
    color: #000;
    padding: 0.8in;
    direction: rtl;
    text-align: right;
  }}
  p {{ margin: 0 0 14px 0; padding: 0; }}
</style>
</head>
<body>
{paras}
</body>
</html>
"""


def generate(out_dir: Path) -> list[dict]:
    out_dir.mkdir(parents=True, exist_ok=True)
    chrome = _find_chrome()
    if not chrome:
        raise RuntimeError("no Chrome/Chromium found; cannot render Arabic corpus")

    entries: list[dict] = []
    for doc_id, kind, paragraphs in DOCS:
        html_path = out_dir / f"{doc_id}.html"
        pdf_path = out_dir / f"{doc_id}.pdf"
        gt_text = "\n".join(paragraphs)
        html_path.write_text(_build_html(paragraphs), encoding="utf-8")
        ok, diag = _render_pdf(chrome, html_path, pdf_path)
        if not ok:
            print(f"  [FAIL] {doc_id}: {diag}", file=sys.stderr)
            continue
        print(f"  [ok]   {doc_id}  ({pdf_path.stat().st_size} bytes)")
        entries.append(
            {
                "id": doc_id,
                "kind": kind,
                "pdf": pdf_path.name,
                "gt_text": gt_text,
            }
        )

    manifest = out_dir / "manifest.json"
    manifest.write_text(
        json.dumps(
            {"subset": "arabic", "entries": entries}, ensure_ascii=False, indent=2
        ),
        encoding="utf-8",
    )
    print(f"\nwrote {len(entries)} docs + {manifest}")
    return entries


if __name__ == "__main__":
    out = HERE / "corpus-arabic"
    generate(out)
