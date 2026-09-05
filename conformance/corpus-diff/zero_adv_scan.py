"""pdfspine 侧：扫描含 无/Widths 简单字体 的页，统计 rawdict 中非空白字符 bbox 宽度==0 的字符（宽度按 0 算的证据）。
用法：<pdfspine-python> zero_adv_scan.py
"""
import json
from collections import Counter
from pathlib import Path

import pdfspine

HERE = Path(__file__).resolve().parent
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
targets = []
for i, f in enumerate(files):
    p = HERE / "out" / "fitz" / f"{i:03d}.json"
    if not p.exists():
        continue
    d = json.loads(p.read_text())
    for pno, pg in d["pages"].items():
        fl = [d["fonts"].get(str(x["xref"]), {}) for x in pg.get("fonts", [])]
        if any(fe.get("Subtype") in ("/Type1", "/TrueType", "/MMType1") and not fe.get("has_Widths") for fe in fl):
            targets.append((f, int(pno), [fe.get("BaseFont") for fe in fl if not fe.get("has_Widths")]))
print("pages with no-Widths simple fonts:", len(targets))
tot = Counter()
per_font = Counter()
per_page = []
for f, pno, names in targets:
    try:
        page = pdfspine.open(f)[pno]
        rd = page.get_text("rawdict")
    except Exception as e:  # noqa: BLE001
        print("ERR", f, pno, e)
        continue
    zeros = Counter()
    nchars = 0
    for b in rd["blocks"]:
        for l in b.get("lines", []):
            for sp in l["spans"]:
                for c in sp["chars"]:
                    ch = c["c"]
                    if ch.isspace():
                        continue
                    nchars += 1
                    if abs(c["bbox"][2] - c["bbox"][0]) < 1e-6:
                        zeros[(sp["font"], ch)] += 1
    if zeros:
        per_page.append((Path(f).name, pno, sum(zeros.values()), nchars, zeros.most_common(6)))
    for k, v in zeros.items():
        tot[k[1]] += v
        per_font[k[0]] += v
print("total zero-width non-space chars:", sum(tot.values()))
print("by char:", tot.most_common(30))
print("by font:", per_font.most_common(15))
print("pages with zero-width chars:", len(per_page))
for r in sorted(per_page, key=lambda x: -x[2])[:15]:
    print("  ", r)
