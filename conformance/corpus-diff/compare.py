"""差分：对齐 fitz 与 pdfspine 的词序列，统计 over-split / under-split，并关联字体特征。

用法：python compare.py  →  写 summary.json，并在 stdout 打印汇总表。
"""
import difflib
import json
import unicodedata
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT_A, OUT_B = HERE / "out" / "fitz", HERE / "out" / "pdfspine"
MIN_BLOCK = 12  # difflib 匹配块最短字符数，短于此不做词边界分析
MAX_EXAMPLES = 100000
STD14 = {"Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique", "Helvetica", "Helvetica-Bold",
         "Helvetica-Oblique", "Helvetica-BoldOblique", "Times-Roman", "Times-Bold", "Times-Italic",
         "Times-BoldItalic", "Symbol", "ZapfDingbats", "Arial", "Arial,Bold", "ArialMT"}


def norm(s: str) -> str:
    return unicodedata.normalize("NFKC", s)


def word_kind(w: str) -> str:
    if all(ch.isalpha() for ch in w):
        return "alpha"
    if any(ch.isalpha() for ch in w):
        return "mixed"
    return "punct_num"


def segments(words_a: list[str], words_b: list[str]):
    """两边无空格串相等的前提下，按公共词边界切段，返回 (a_words, b_words) 段列表。"""
    ba, bb, off = set(), set(), 0
    for w in words_a:
        off += len(w)
        ba.add(off)
    off = 0
    for w in words_b:
        off += len(w)
        bb.add(off)
    common = sorted((ba & bb) | {0})
    out, ia, ib = [], 0, 0
    oa = ob = 0
    for c in common[1:]:
        sa, sb = [], []
        while oa < c:
            sa.append(words_a[ia]); oa += len(words_a[ia]); ia += 1
        while ob < c:
            sb.append(words_b[ib]); ob += len(words_b[ib]); ib += 1
        out.append((sa, sb))
    return out


def align_page(wa: list[str], wb: list[str]):
    """返回 dict：over/under/mixed 计数与示例；先用 difflib 找公共字符块，块内按词边界分段。"""
    sa, sb = "".join(wa), "".join(wb)
    res = {"content_equal": sa == sb, "ratio": None, "over": 0, "over_alpha": 0, "over_punct": 0,
           "under": 0, "under_alpha": 0, "mixed": 0, "over_examples": [], "under_examples": [],
           "n_fitz": len(wa), "n_pdfspine": len(wb), "unmatched_chars_a": 0, "unmatched_chars_b": 0}
    if not sa or not sb:
        return res
    # 词的字符起止区间
    def spans(ws):
        s, o = [], 0
        for w in ws:
            s.append((o, o + len(w))); o += len(w)
        return s
    spa, spb = spans(wa), spans(wb)
    if sa == sb:
        blocks = [(0, 0, len(sa))]
        res["ratio"] = 1.0
    else:
        sm = difflib.SequenceMatcher(None, sa, sb, autojunk=False)
        res["ratio"] = round(sm.ratio(), 4)
        blocks = [(i, j, n) for i, j, n in sm.get_matching_blocks() if n >= MIN_BLOCK]
        matched = sum(n for _, _, n in blocks)
        res["unmatched_chars_a"] = len(sa) - matched
        res["unmatched_chars_b"] = len(sb) - matched
    for i, j, n in blocks:
        # 取完整落在块内的词
        sub_a = [w for w, (s, e) in zip(wa, spa) if s >= i and e <= i + n]
        sub_b = [w for w, (s, e) in zip(wb, spb) if s >= j and e <= j + n]
        # 需要两边子串起点一致：裁到共同起点/终点
        # 找到块内第一个词的起点在两边是否相同偏移
        sa_off = next((s for (s, e) in spa if s >= i and e <= i + n), None)
        sb_off = next((s for (s, e) in spb if s >= j and e <= j + n), None)
        if sa_off is None or sb_off is None:
            continue
        # 把两边对齐到同一起始偏移（相对块）
        ra, rb = sa_off - i, sb_off - j
        if ra != rb:
            # 丢掉起点更早的一侧的前缀词，直到对齐
            while ra < rb and sub_a:
                ra += len(sub_a.pop(0))
            while rb < ra and sub_b:
                rb += len(sub_b.pop(0))
            if ra != rb:
                continue
        ta, tb = "".join(sub_a), "".join(sub_b)
        # 末尾对齐
        while len(ta) > len(tb) and sub_a:
            sub_a.pop(); ta = "".join(sub_a)
        while len(tb) > len(ta) and sub_b:
            sub_b.pop(); tb = "".join(sub_b)
        if ta != tb or not ta:
            continue
        for seg_a, seg_b in segments(sub_a, sub_b):
            if len(seg_a) == 1 and len(seg_b) > 1:
                res["over"] += 1
                k = word_kind(seg_a[0])
                if k == "alpha":
                    res["over_alpha"] += 1
                elif k == "punct_num":
                    res["over_punct"] += 1
                if len(res["over_examples"]) < MAX_EXAMPLES:
                    res["over_examples"].append([seg_a[0], seg_b])
            elif len(seg_a) > 1 and len(seg_b) == 1:
                res["under"] += 1
                if word_kind(seg_b[0]) == "alpha":
                    res["under_alpha"] += 1
                if len(res["under_examples"]) < MAX_EXAMPLES:
                    res["under_examples"].append([seg_a, seg_b[0]])
            elif len(seg_a) > 1 and len(seg_b) > 1:
                res["mixed"] += 1
    return res


def font_flags(f: dict) -> list[str]:
    flags = []
    st = f.get("Subtype")
    base = (f.get("BaseFont") or "").lstrip("/")
    base_core = base.split("+", 1)[-1]
    if st in ("/Type1", "/TrueType", "/MMType1") and not f.get("has_Widths"):
        flags.append("no_Widths(std14)" if base_core in STD14 else "no_Widths")
    if st == "/Type3":
        flags.append("Type3")
        if f.get("FontMatrix_nonstd"):
            flags.append("Type3_FontMatrix_nonstd")
    if st == "/Type0" and not f.get("has_W"):
        flags.append("Type0_no_W")
    if not f.get("embedded"):
        flags.append("not_embedded")
    v = f.get("asc_plus_absdesc")
    if v is not None and v < 750:
        flags.append("asc+|desc|<750")
    if f.get("has_Widths") and (f.get("Widths_zero_ratio") or 0) > 0.5:
        flags.append("Widths_mostly_zero")
    if f.get("MissingWidth") not in (None, "0"):
        flags.append("MissingWidth")
    return flags


def main():
    files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
    per_file, per_page = [], []
    errs = {"fitz": [], "pdfspine": []}
    tot = Counter()
    for idx, path in enumerate(files):
        fa, fb = OUT_A / f"{idx:03d}.json", OUT_B / f"{idx:03d}.json"
        if not fa.exists() or not fb.exists():
            continue
        a, b = json.loads(fa.read_text()), json.loads(fb.read_text())
        if a["error"]:
            errs["fitz"].append((path, a["error"][:120]))
        if b["error"]:
            errs["pdfspine"].append((path, b["error"][:120]))
        fstat = Counter()
        fex_over, fex_under = [], []
        for pno, pa in a["pages"].items():
            pb = b["pages"].get(pno)
            if pb is None or "error" in pa or "error" in pb:
                if pb is not None and "error" in pb:
                    errs["pdfspine"].append((f"{path}#p{pno}", pb["error"][:120]))
                continue
            wa = [norm(w[4]) for w in pa["words"]]
            wb = [norm(w[4]) for w in pb["words"]]
            r = align_page(wa, wb)
            r["path"], r["pno"] = path, int(pno)
            r["fonts"] = [x["xref"] for x in pa.get("fonts", [])]
            per_page.append(r)
            for k in ("over", "over_alpha", "over_punct", "under", "under_alpha", "mixed", "n_fitz", "n_pdfspine"):
                fstat[k] += r[k]
            fstat["pages"] += 1
            fstat["content_equal"] += int(r["content_equal"])
            fex_over.extend(r["over_examples"])
            fex_under.extend(r["under_examples"])
        per_file.append({"idx": idx, "path": path, **fstat, "over_examples": fex_over[:MAX_EXAMPLES],
                         "under_examples": fex_under[:MAX_EXAMPLES]})
        tot.update(fstat)

    per_page.sort(key=lambda r: (-r["over"], r["path"]))
    top = per_page[:10]
    # 字体特征关联
    font_index = {}
    for idx, path in enumerate(files):
        fa = OUT_A / f"{idx:03d}.json"
        if fa.exists():
            font_index[path] = json.loads(fa.read_text())["fonts"]
    for r in per_page:
        fl = Counter()
        for x in r["fonts"]:
            f = font_index.get(r["path"], {}).get(str(x))
            if f:
                for g in font_flags(f):
                    fl[g] += 1
        r["font_flags"] = dict(fl)
    for r in top:
        r["font_detail"] = [font_index.get(r["path"], {}).get(str(x)) for x in r["fonts"]]

    # 相关性：按页级 flag 是否出现，比较 over-split 率（over / n_fitz）
    corr = {}
    all_flags = set()
    for r in per_page:
        all_flags.update(r["font_flags"])
    for g in sorted(all_flags):
        with_f = [r for r in per_page if g in r["font_flags"]]
        without = [r for r in per_page if g not in r["font_flags"]]
        def rate(rs):
            n = sum(r["n_fitz"] for r in rs)
            return (sum(r["over"] for r in rs) / n * 1000) if n else 0.0
        corr[g] = {"pages_with": len(with_f), "over_with": sum(r["over"] for r in with_f),
                   "over_per_1k_words_with": round(rate(with_f), 2),
                   "pages_without": len(without), "over_without": sum(r["over"] for r in without),
                   "over_per_1k_words_without": round(rate(without), 2),
                   "pages_with_and_over>0": sum(1 for r in with_f if r["over"] > 0)}

    summary = {"files": len(per_file), "pages": len(per_page), "totals": dict(tot), "errors": errs,
               "per_file": per_file, "top_pages": top, "font_corr": corr,
               "per_page_brief": [{k: r[k] for k in ("path", "pno", "over", "over_alpha", "under", "mixed",
                                                     "content_equal", "ratio", "font_flags", "n_fitz", "n_pdfspine", "over_examples", "under_examples")}
                                  for r in per_page]}
    (HERE / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))

    print(f"files={len(per_file)} pages={len(per_page)}")
    print("totals:", dict(tot))
    print("errors fitz:", len(errs["fitz"]), "pdfspine:", len(errs["pdfspine"]))
    print("\n== top over-split pages ==")
    for r in top:
        print(f"{r['over']:4d} over ({r['over_alpha']} alpha) {r['under']:3d} under  ratio={r['ratio']} "
              f"{Path(r['path']).name}#p{r['pno']}  flags={r['font_flags']}")
        for ex in r["over_examples"][:4]:
            print("      ", ex[0], "->", ex[1])
    print("\n== per-file (over>0) ==")
    for f in sorted(per_file, key=lambda x: -x["over"]):
        if f["over"] or f["under"]:
            print(f"{f['over']:5d} over ({f['over_alpha']} alpha/{f['over_punct']} punct) {f['under']:5d} under "
                  f"{f['mixed']:4d} mixed pages={f['pages']} eq={f['content_equal']} {Path(f['path']).name}")
    print("\n== font flag correlation ==")
    for g, c in corr.items():
        print(f"{g:28s} with: pages={c['pages_with']:4d} over={c['over_with']:5d} /1k={c['over_per_1k_words_with']:6.2f} "
              f"pages_over>0={c['pages_with_and_over>0']:4d} | without: pages={c['pages_without']:4d} "
              f"over={c['over_without']:5d} /1k={c['over_per_1k_words_without']:6.2f}")


if __name__ == "__main__":
    main()
