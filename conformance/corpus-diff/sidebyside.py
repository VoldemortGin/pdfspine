"""对 over-split 最高的前 N 页，给出 get_text("text") 的并排对照行。用法：python sidebyside.py [N]"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
N = int(sys.argv[1]) if len(sys.argv) > 1 else 5
s = json.loads((HERE / "summary.json").read_text())
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
idx_of = {p: i for i, p in enumerate(files)}

for r in s["top_pages"][:N]:
    i = idx_of[r["path"]]
    a = json.loads((HERE / "out" / "fitz" / f"{i:03d}.json").read_text())["pages"][str(r["pno"])]["text"]
    b = json.loads((HERE / "out" / "pdfspine" / f"{i:03d}.json").read_text())["pages"][str(r["pno"])]["text"]
    la, lb = a.split("\n"), b.split("\n")
    print(f"===== {Path(r['path']).name}#p{r['pno']}  (over={r['over']}, fitz lines={len(la)}, pdfspine lines={len(lb)})")
    shown = 0
    for ex in r["over_examples"]:
        w, parts = ex[0], ex[1]
        split_form = " ".join(parts)
        # 找 fitz 含该词的行、pdfspine 含拆开形式的行
        fa = next((l for l in la if f" {w} " in f" {l} "), None)
        fb = next((l for l in lb if split_form in l), None)
        if fa is None or fb is None:
            continue
        print(f"  [{w!r} -> {parts}]")
        print(f"    PyMuPDF : {fa.strip()[:110]}")
        print(f"    pdfspine: {fb.strip()[:110]}")
        shown += 1
        if shown >= 3:
            break
    if shown == 0:
        print("  (未找到可并排的行)")
