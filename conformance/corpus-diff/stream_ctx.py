"""打印内容流中包含某字符串片段的上下文：python stream_ctx.py <pdf> <pno> <needle> [ctx_bytes]"""
import re
import sys

import fitz

path, pno, needle = sys.argv[1], int(sys.argv[2]), sys.argv[3]
ctx = int(sys.argv[4]) if len(sys.argv) > 4 else 250
page = fitz.open(path)[pno]
cs = page.read_contents()
print("content len", len(cs), "| Tc ops:", len(re.findall(rb"[-\d.]+\s+Tc", cs)),
      "| Tw ops:", len(re.findall(rb"[-\d.]+\s+Tw", cs)), "| Tz ops:", len(re.findall(rb"[-\d.]+\s+Tz", cs)),
      "| Tf sizes:", sorted(set(re.findall(rb"/\S+\s+([-\d.]+)\s+Tf", cs)))[:8],
      "| BT count:", cs.count(b"BT"))
n = needle.encode("latin-1", "replace")
hits = [m.start() for m in re.finditer(re.escape(n), cs)]
if not hits:
    # 尝试按十六进制串或分字节找首两个字符
    n2 = n[:2]
    hits = [m.start() for m in re.finditer(re.escape(n2), cs)][:3]
    print(f"needle not found verbatim; showing hits for {n2!r}: {len(hits)}")
for h in hits[:3]:
    print("-----", h)
    print(cs[max(0, h - ctx): h + ctx].decode("latin-1", "replace"))
