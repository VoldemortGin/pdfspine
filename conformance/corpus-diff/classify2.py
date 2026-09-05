"""全量 over-split 事件归类（需要 compare.py 以 MAX_EXAMPLES=100000 重跑后的 summary.json）。

机制类别（看 pdfspine text 模式）：newline_cut（同一词被切成两行）/ space_insert（text 模式也插了空格）/
words_only（text 模式完整，仅 words 模式拆）/ unknown。
形态子类：letters（全部拆成单字符、≥3 段）/ punct_boundary（边界处是 ’“”–—•■§ 等非 ASCII 标点）/
digit_boundary（字母与数字交界，脚注上标类）/ other。
并与 stream_flags / font_extra 做页级交叉。
"""
import json
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
idx_of = {p: i for i, p in enumerate(files)}
s = json.loads((HERE / "summary.json").read_text())
sf = json.loads((HERE / "stream_flags.json").read_text())
fx = json.loads((HERE / "font_extra.json").read_text())
PUNCT = set("’‘“”–—•■§«»…·°'\"()[]/,.:;")
cache = {}


def text_of(i, pno):
    k = (i, pno)
    if k not in cache:
        d = json.loads((HERE / "out" / "pdfspine" / f"{i:03d}.json").read_text())
        cache[k] = d["pages"][str(pno)].get("text", "")
    return cache[k]


def mech(t, w, parts):
    if len(w) >= 3 and w in t:
        return "words_only"
    for k in range(1, len(parts)):
        if "".join(parts[:k]) + "\n" + "".join(parts[k:]) in t:
            return "newline_cut"
    if " ".join(parts) in t:
        return "space_insert"
    for k in range(1, len(parts)):
        if "".join(parts[:k]) + " " + "".join(parts[k:]) in t:
            return "space_insert"
    if w in t:
        return "words_only"
    return "unknown"


def shape(w, parts):
    if len(parts) >= 3 and all(len(p) == 1 for p in parts):
        return "letters"
    for a, b in zip(parts, parts[1:]):
        if a[-1] in PUNCT or b[0] in PUNCT:
            return "punct_boundary"
        if (a[-1].isdigit() != b[0].isdigit()) and any(ch.isalpha() for ch in w):
            return "digit_boundary"
    return "other"


def group(path):
    n = Path(path).name
    if "corpus-eurlex" in path:
        return "eurlex"
    if "corpus-fintabnet" in path:
        return "fintabnet"
    if "corpus-govinfo" in path:
        return "govinfo(FR/GAO/USCOURTS)"
    if "corpus-robustness" in path:
        return "govdocs1"
    if "corpus-pmc" in path:
        return "pmc"
    if "fixtures/corpus" in path:
        return "fixtures/corpus(irs/cdc/govinfo/…)"
    return "other"


mech_tot, shape_tot, cross = Counter(), Counter(), Counter()
by_group = defaultdict(Counter)
by_file_mech = defaultdict(Counter)
alpha_by_mech = Counter()
for r in s["per_page_brief"]:
    i = idx_of[r["path"]]
    t = text_of(i, r["pno"])
    for w, parts in r["over_examples"]:
        m, sh = mech(t, w, parts), shape(w, parts)
        mech_tot[m] += 1
        shape_tot[sh] += 1
        cross[(m, sh)] += 1
        by_group[group(r["path"])][m] += 1
        by_file_mech[Path(r["path"]).name][m] += 1
        if all(ch.isalpha() for ch in w):
            alpha_by_mech[m] += 1

print("== 全量 over-split 事件 =", sum(mech_tot.values()))
print("机制:", dict(mech_tot))
print("其中纯字母词:", dict(alpha_by_mech))
print("形态:", dict(shape_tot))
print("机制×形态:")
for (m, sh), v in sorted(cross.items(), key=lambda kv: -kv[1]):
    print(f"   {m:13s} {sh:15s} {v}")
print("按语料分组:")
for g, c in sorted(by_group.items(), key=lambda kv: -sum(kv[1].values())):
    print(f"   {g:36s} total={sum(c.values()):5d} {dict(c)}")
print("按文件（前 25）:")
for n, c in sorted(by_file_mech.items(), key=lambda kv: -sum(kv[1].values()))[:25]:
    print(f"   {n:36s} total={sum(c.values()):4d} {dict(c)}")

# 页级交叉：desc_positive / tc_nonzero / std14_no_widths
print("\n== 页级特征 vs over-split（over/1k 词，pages_over>0）==")
rows = []
for r in s["per_page_brief"]:
    p = sf.get(r["path"], {}).get(str(r["pno"]), {})
    e = fx.get(f"{r['path']}#{r['pno']}", {})
    rows.append((r, p, e))


def stat(rs):
    n = sum(r["n_fitz"] for r, _, _ in rs)
    o = sum(r["over"] for r, _, _ in rs)
    return len(rs), o, (o / n * 1000 if n else 0.0), sum(1 for r, _, _ in rs if r["over"] > 0)


for name, pred in [
    ("Tc≠0 且 Descent>0", lambda p, e: p.get("tc_nonzero") and e.get("desc_positive", 0) > 0),
    ("Tc≠0 且 Descent≤0", lambda p, e: p.get("tc_nonzero") and e.get("desc_positive", 0) == 0),
    ("Tc=0 且 Descent>0", lambda p, e: not p.get("tc_nonzero") and e.get("desc_positive", 0) > 0),
    ("Tc=0 且 Descent≤0", lambda p, e: not p.get("tc_nonzero") and e.get("desc_positive", 0) == 0),
    ("std14 无 Widths", lambda p, e: e.get("std14_no_widths", 0) > 0),
    ("Type3", lambda p, e: e.get("type3", 0) > 0),
    ("Type0 无 W", lambda p, e: e.get("type0_no_W", 0) > 0),
    ("其他简单字体无 Widths", lambda p, e: e.get("no_widths_other", 0) > 0),
]:
    w = [x for x in rows if pred(x[1], x[2])]
    a = stat(w)
    print(f"   {name:22s} pages={a[0]:4d} over={a[1]:5d} over/1k={a[2]:5.2f} pages_over>0={a[3]:4d}")
