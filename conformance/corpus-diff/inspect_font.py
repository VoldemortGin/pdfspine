"""fitz 侧字体/内容流检查：python inspect_font.py <pdf> <pno> <target-word>

打印：页上各字体的 Widths（零宽度码位）、rawdict 中目标词各字符的字体 / 原点 / bbox 宽度、
以及内容流中包含目标词首字符串的片段。
"""
import re
import sys

import fitz

path, pno, target = sys.argv[1], int(sys.argv[2]), sys.argv[3]
doc = fitz.open(path)
page = doc[pno]
print("page rect", page.rect, "rotation", page.rotation)
fonts = page.get_fonts(full=True)
for f in fonts:
    xref = f[0]
    st = doc.xref_get_key(xref, "Subtype")[1]
    fc = doc.xref_get_key(xref, "FirstChar")[1]
    w = doc.xref_get_key(xref, "Widths")[1]
    enc = doc.xref_get_key(xref, "Encoding")
    print(f"font xref={xref} {f[3]} {st} FirstChar={fc} enc={enc} ref_by={f[6]}")
    if w and w != "null":
        nums = [float(x) for x in re.findall(r"-?\d+(?:\.\d+)?", w)]
        zero_codes = [int(fc) + i for i, v in enumerate(nums) if v == 0]
        nonzero = [(int(fc) + i, v) for i, v in enumerate(nums) if v != 0]
        print(f"   Widths n={len(nums)} zeros={len(zero_codes)} zero_codes(sample)={zero_codes[:40]}")
        print(f"   nonzero(sample)={nonzero[:12]}")
    if enc[0] == "xref":
        ex = int(enc[1].split()[0])
        print("   Encoding obj:", doc.xref_object(ex)[:300].replace("\n", " "))

# rawdict：目标词的字符
rd = page.get_text("rawdict")
found = 0
for b in rd["blocks"]:
    for l in b.get("lines", []):
        for sp in l["spans"]:
            txt = "".join(c["c"] for c in sp["chars"])
            if target in txt:
                k = txt.index(target)
                chars = sp["chars"][k:k + len(target)]
                print(f"span font={sp['font']} size={sp['size']} flags={sp['flags']} dir={l['dir']} text={txt[:60]!r}")
                for c in chars:
                    x0, y0, x1, y1 = c["bbox"]
                    print(f"   {c['c']!r} origin=({c['origin'][0]:.2f},{c['origin'][1]:.2f}) bbox_w={x1-x0:.2f}")
                found += 1
                if found >= 2:
                    break
        if found >= 2:
            break
    if found >= 2:
        break

# 内容流片段
cs = page.read_contents()
first = target[0].encode("latin-1", "replace")
i = cs.find(b"(" + first)
print("content stream len", len(cs))
if i >= 0:
    print(cs[max(0, i - 300): i + 500].decode("latin-1", "replace"))
else:
    # 找 TJ/Tj 任意片段
    m = re.search(rb"\[.{0,200}\]\s*TJ", cs, re.S)
    print(cs[m.start():m.start() + 600].decode("latin-1", "replace") if m else cs[:600].decode("latin-1", "replace"))
