"""前 N 页并排对照：text 模式行 + words 模式序列。用法：python sidebyside2.py [N]"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
N = int(sys.argv[1]) if len(sys.argv) > 1 else 5
s = json.loads((HERE / "summary.json").read_text())
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
idx_of = {p: i for i, p in enumerate(files)}


def nospace(x):
    return "".join(x.split())


for r in s["top_pages"][:N]:
    i = idx_of[r["path"]]
    pa = json.loads((HERE / "out" / "fitz" / f"{i:03d}.json").read_text())["pages"][str(r["pno"])]
    pb = json.loads((HERE / "out" / "pdfspine" / f"{i:03d}.json").read_text())["pages"][str(r["pno"])]
    la, lb = pa["text"].split("\n"), pb["text"].split("\n")
    wa, wb = [w[4] for w in pa["words"]], [w[4] for w in pb["words"]]
    print(f"===== {Path(r['path']).name}#p{r['pno']}  over={r['over']} under={r['under']}")
    shown = 0
    for w, parts in r["over_examples"]:
        if len(w) < 4:
            continue
        fa = next((l for l in la if f" {w} " in f" {l} "), None)
        if fa is None:
            continue
        key = nospace(fa)
        # pdfspine：找去空格后包含该 fitz 行前 12 字符的行；或被切开的相邻行
        fb = next((l for l in lb if nospace(l) and nospace(l) in key and len(nospace(l)) >= min(12, len(key))), None)
        fb2 = None
        if fb is None:
            for k, l in enumerate(lb):
                if nospace(l) and key.startswith(nospace(l)) and k + 1 < len(lb):
                    fb, fb2 = l, lb[k + 1]
                    break
        # words 序列：fitz 从 w 开始取 5 个；pdfspine 从 parts[0] 起取足够多
        try:
            ka = wa.index(w)
            seq_a = wa[ka:ka + 5]
        except ValueError:
            seq_a = [w]
        seq_b = []
        target = nospace("".join(seq_a))
        for k in range(len(wb)):
            if wb[k] == parts[0] and (k + 1 < len(wb) and wb[k + 1] == parts[1]):
                acc = ""
                j = k
                while j < len(wb) and len(acc) < len(target):
                    acc += wb[j]; seq_b.append(wb[j]); j += 1
                break
        print(f"  [{w!r} -> {parts}]")
        print(f"    PyMuPDF  text : {fa.strip()[:100]}")
        print(f"    pdfspine text : {(fb or '?').strip()[:100]}" + (f"  ⏎  {fb2.strip()[:60]}" if fb2 else ""))
        print(f"    PyMuPDF  words: {seq_a}")
        print(f"    pdfspine words: {seq_b}")
        shown += 1
        if shown >= 3:
            break
