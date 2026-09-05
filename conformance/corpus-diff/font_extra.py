"""从 out/fitz/*.json 汇总每页额外字体标志：desc_positive（/Descent>0）、std14_no_widths、type3。输出 font_extra.json"""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
files = [l for l in (HERE / "corpus.txt").read_text().split("\n") if l.strip()]
STD14 = {"Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique", "Helvetica", "Helvetica-Bold",
         "Helvetica-Oblique", "Helvetica-BoldOblique", "Times-Roman", "Times-Bold", "Times-Italic",
         "Times-BoldItalic", "Symbol", "ZapfDingbats", "Arial", "Arial,Bold", "ArialMT"}
out = {}
desc_pos_fonts = []
for i, f in enumerate(files):
    p = HERE / "out" / "fitz" / f"{i:03d}.json"
    if not p.exists():
        continue
    d = json.loads(p.read_text())
    fonts = d["fonts"]
    for pno, pg in d["pages"].items():
        flags = {"desc_positive": 0, "std14_no_widths": 0, "no_widths_other": 0, "type3": 0, "type0_no_W": 0, "n_fonts": 0}
        for x in pg.get("fonts", []):
            fe = fonts.get(str(x["xref"]))
            if not fe:
                continue
            flags["n_fonts"] += 1
            try:
                if fe.get("Descent") is not None and float(fe["Descent"]) > 0:
                    flags["desc_positive"] += 1
                    desc_pos_fonts.append((Path(f).name, fe.get("BaseFont"), fe.get("Ascent"), fe.get("Descent")))
            except ValueError:
                pass
            st = fe.get("Subtype")
            base = (fe.get("BaseFont") or "").lstrip("/").split("+", 1)[-1]
            if st in ("/Type1", "/TrueType", "/MMType1") and not fe.get("has_Widths"):
                if base in STD14:
                    flags["std14_no_widths"] += 1
                else:
                    flags["no_widths_other"] += 1
            if st == "/Type3":
                flags["type3"] += 1
            if st == "/Type0" and not fe.get("has_W"):
                flags["type0_no_W"] += 1
        out[f"{f}#{pno}"] = flags
(HERE / "font_extra.json").write_text(json.dumps(out))
uniq = sorted(set(desc_pos_fonts))
print("pages:", len(out), "| desc_positive font uses:", len(desc_pos_fonts), "| unique (file,font):", len(uniq))
for u in uniq[:40]:
    print("  ", u)
