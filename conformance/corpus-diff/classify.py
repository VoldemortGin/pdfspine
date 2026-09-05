"""把每个 over-split 示例归类：pdfspine text 模式里是 换行切断(newline_cut) / 插空格(space_insert) / 仅 words 模式(words_only)。
并把 stream_flags 与页级 over-split 做相关性。输出到 stdout。
"""
import json
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
s = json.loads((HERE / "summary.json").read_text())
sf = json.loads((HERE / "stream_flags.json").read_text()) if (HERE / "stream_flags.json").exists() else {}

cat_total = Counter()
cat_by_file = defaultdict(Counter)
cache = {}


def text_of(i, pno):
    k = (i, pno)
    if k not in cache:
        d = json.loads((HERE / "out" / "pdfspine" / f"{i:03d}.json").read_text())
        cache[k] = d["pages"][str(pno)].get("text", "")
    return cache[k]


# 重新对所有页跑一遍示例分类（per_page_brief 没存示例，用 top 与 per_file 示例 + 重新计算示例太重；这里改为读 per_file 示例）
idx_of = {p: i for i, p in enumerate(files)}
for f in s["per_file"]:
    i = idx_of[f["path"]]
    for ex in f["over_examples"]:
        w, parts = ex[0], ex[1]
        # 找到该文件任意页 text 含 parts 拼接
        found = "unknown"
        for pno in range(20):
            try:
                t = text_of(i, pno)
            except Exception:  # noqa: BLE001
                break
            nl = "\n".join(parts)
            sp = " ".join(parts)
            if parts[0] + "\n" + parts[1] in t or nl in t:
                found = "newline_cut"; break
            if sp in t:
                found = "space_insert"; break
            if w in t:
                found = "words_only"  # text 模式完整，仅 words 拆
                break
        cat_total[found] += 1
        cat_by_file[Path(f["path"]).name][found] += 1

print("== over-split 示例归类（基于 per-file 前 10 个示例）==")
print(dict(cat_total))
for name, c in sorted(cat_by_file.items(), key=lambda kv: -sum(kv[1].values()))[:30]:
    print(f"  {name:36s} {dict(c)}")

if sf:
    print("\n== 内容流特征 vs 页级 over-split ==")
    rows = []
    for r in s["per_page_brief"]:
        p = sf.get(r["path"], {}).get(str(r["pno"]))
        if not p or "error" in p:
            continue
        rows.append((r, p))
    for flag in ("tf_le1", "tc_nonzero", "tw_nonzero", "tz_non100"):
        w = [(r, p) for r, p in rows if p[flag]]
        wo = [(r, p) for r, p in rows if not p[flag]]
        def stat(rs):
            n = sum(r["n_fitz"] for r, _ in rs)
            o = sum(r["over"] for r, _ in rs)
            oa = sum(r["over_alpha"] for r, _ in rs)
            return len(rs), o, oa, (o / n * 1000 if n else 0), sum(1 for r, _ in rs if r["over"] > 0)
        a, b = stat(w), stat(wo)
        print(f"{flag:12s} with: pages={a[0]:4d} over={a[1]:5d} alpha={a[2]:5d} /1k={a[3]:5.2f} pages_over>0={a[4]:4d} | "
              f"without: pages={b[0]:4d} over={b[1]:5d} alpha={b[2]:5d} /1k={b[3]:5.2f} pages_over>0={b[4]:4d}")
    # 交叉：tc 且 tf_le1
    print("\n  top-30 over 页的 stream 特征：")
    for r, p in sorted(rows, key=lambda x: -x[0]["over"])[:30]:
        print(f"   over={r['over']:3d} alpha={r['over_alpha']:3d} {Path(r['path']).name:32s}#p{r['pno']:<3d} "
              f"tf_min={p['tf_min']} tc_max={p['tc_max']} tw={p['tw_nonzero']} tz={p['tz_non100']} BT={p['bt']} fontflags={r['font_flags']}")
