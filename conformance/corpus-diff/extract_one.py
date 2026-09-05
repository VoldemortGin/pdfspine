"""单文件抽取：python extract_one.py <fitz|pdfspine> <pdf> <out.json>

每页（≤20 页）输出 words（[x0,y0,x1,y1,word]）与 text；fitz 模式额外输出字体特征。
"""
import json
import re
import sys
import traceback

MAX_PAGES = 20
engine, path, out = sys.argv[1], sys.argv[2], sys.argv[3]

if engine == "fitz":
    import fitz as mod  # PyMuPDF 真身
else:
    import pdfspine as mod

XREF_RE = re.compile(r"(\d+)\s+0\s+R")


def key(doc, xref, name):
    try:
        t, v = doc.xref_get_key(xref, name)
    except Exception as e:  # noqa: BLE001
        return f"ERR:{e}"
    if t == "null":
        return None
    return v


def font_features(doc, xref):
    f = {"xref": xref}
    f["Subtype"] = key(doc, xref, "Subtype")
    f["BaseFont"] = key(doc, xref, "BaseFont")
    f["Encoding"] = key(doc, xref, "Encoding")
    f["FirstChar"] = key(doc, xref, "FirstChar")
    f["LastChar"] = key(doc, xref, "LastChar")
    w = key(doc, xref, "Widths")
    f["has_Widths"] = w is not None
    if w:
        nums = re.findall(r"-?\d+(?:\.\d+)?", w)
        f["Widths_len"] = len(nums)
        f["Widths_zero_ratio"] = (sum(1 for n in nums if float(n) == 0) / len(nums)) if nums else None
    f["FontMatrix"] = key(doc, xref, "FontMatrix")
    if f["FontMatrix"]:
        nums = [float(x) for x in re.findall(r"-?\d+(?:\.\d+)?(?:[eE]-?\d+)?", f["FontMatrix"])]
        f["FontMatrix_nonstd"] = nums[:6] != [0.001, 0, 0, 0.001, 0, 0] if len(nums) >= 6 else True
    desc_xref = xref
    if f["Subtype"] == "/Type0":
        d = key(doc, xref, "DescendantFonts")
        m = XREF_RE.search(d or "")
        if m:
            desc_xref = int(m.group(1))
            f["Descendant_xref"] = desc_xref
            f["Descendant_Subtype"] = key(doc, desc_xref, "Subtype")
            f["has_W"] = key(doc, desc_xref, "W") is not None
            f["DW"] = key(doc, desc_xref, "DW")
            f["CIDToGIDMap"] = key(doc, desc_xref, "CIDToGIDMap")
        else:
            f["has_W"] = False
    fd = key(doc, desc_xref, "FontDescriptor")
    m = XREF_RE.search(fd or "")
    f["has_FontDescriptor"] = bool(fd)
    if m:
        fx = int(m.group(1))
        f["MissingWidth"] = key(doc, fx, "MissingWidth")
        asc, dsc = key(doc, fx, "Ascent"), key(doc, fx, "Descent")
        f["Ascent"], f["Descent"] = asc, dsc
        try:
            f["asc_plus_absdesc"] = abs(float(asc)) + abs(float(dsc))
        except (TypeError, ValueError):
            f["asc_plus_absdesc"] = None
        f["Flags"] = key(doc, fx, "Flags")
        ff = [k for k in ("FontFile", "FontFile2", "FontFile3") if key(doc, fx, k)]
        f["FontFile"] = ff[0] if ff else None
        f["embedded"] = bool(ff)
    else:
        f["embedded"] = False
    return f


result = {"engine": engine, "path": path, "pages": {}, "error": None, "fonts": {}}
try:
    doc = mod.open(path)
    n = min(len(doc), MAX_PAGES)
    result["page_count"] = len(doc)
    for pno in range(n):
        pg = {}
        try:
            page = doc[pno]
            words = page.get_text("words")
            pg["words"] = [[round(float(w[0]), 2), round(float(w[1]), 2), round(float(w[2]), 2),
                            round(float(w[3]), 2), str(w[4])] for w in words]
            pg["text"] = page.get_text("text")
            if engine == "fitz":
                xrefs = []
                for ft in page.get_fonts(full=True):
                    xref = ft[0]
                    xrefs.append({"xref": xref, "ext": ft[1], "type": ft[2], "basefont": ft[3],
                                  "name": ft[4], "encoding": ft[5], "referencer": ft[6]})
                    if xref and str(xref) not in result["fonts"]:
                        try:
                            result["fonts"][str(xref)] = font_features(doc, xref)
                        except Exception as e:  # noqa: BLE001
                            result["fonts"][str(xref)] = {"xref": xref, "error": repr(e)}
                pg["fonts"] = xrefs
        except Exception as e:  # noqa: BLE001
            pg["error"] = repr(e)
        result["pages"][str(pno)] = pg
except Exception as e:  # noqa: BLE001
    result["error"] = repr(e) + "\n" + traceback.format_exc()[-500:]

with open(out, "w", encoding="utf-8") as fh:
    json.dump(result, fh, ensure_ascii=False)
