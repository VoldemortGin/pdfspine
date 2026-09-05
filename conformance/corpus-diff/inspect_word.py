"""字符级检查（双引擎同一脚本）：python inspect_word.py <fitz|pdfspine> <pdf> <pno> <target-word> [max_spans]

用 rawdict 找包含目标词的 span，打印每个字符的 origin.x / bbox.x0 / bbox.x1 / 与下一字符的 gap。
"""
import sys

engine, path, pno, target = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
max_spans = int(sys.argv[5]) if len(sys.argv) > 5 else 1
mod = __import__("fitz") if engine == "fitz" else __import__("pdfspine")
doc = mod.open(path)
page = doc[pno]
rd = page.get_text("rawdict")
shown = 0
for b in rd["blocks"]:
    for l in b.get("lines", []):
        # 把同一行所有 span 的字符串起来，允许目标词跨 span
        chars = [(sp, c) for sp in l["spans"] for c in sp["chars"]]
        txt = "".join(c["c"] for _, c in chars)
        if target not in txt:
            continue
        k = txt.index(target)
        seg = chars[max(0, k - 1): k + len(target) + 1]
        print(f"[{engine}] line dir={l.get('dir')} spans={len(l['spans'])} text={txt[:70]!r}")
        for i, (sp, c) in enumerate(seg):
            x0, y0, x1, y1 = c["bbox"]
            ox = c["origin"][0]
            nxt = seg[i + 1][1]["origin"][0] if i + 1 < len(seg) else None
            gap = f"{nxt - x1:+.2f}" if nxt is not None else "   -"
            print(f"   {c['c']!r:6} font={sp['font'][:22]:22} sz={sp['size']:.1f} ox={ox:8.2f} bx0={x0:8.2f} bx1={x1:8.2f} adv={x1-ox:6.2f} gap_to_next={gap}")
        shown += 1
        if shown >= max_spans:
            sys.exit(0)
print(f"[{engine}] target not found")
