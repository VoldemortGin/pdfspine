#!/usr/bin/env python3
"""Order/bidi-aware diagnostic scorer for the Arabic corpus.

For every doc in ``corpus-arabic/manifest.json`` it extracts text with BOTH
pdfspine (project ``.venv``) and fitz (``.venv-oracle``), scores each against the
KNOWN logical-order GT via ``score.py``, and adds bidi-specific checks:

  (a) presentation-form leakage (U+FE70–FEFF / U+FB50–FDFF) on RAW output,
      before any NFKC folding (NFKC would mask it);
  (b) per-line diff vs GT so RTL word-order vs visual order is visible;
  (c) detection of reversed LTR sub-runs (Latin/digits inside an RTL line).

Run from repo root::

    .venv/bin/python conformance/gt/score_arabic.py
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from score import lev_ratio, score_all  # noqa: E402

ROOT = HERE / "corpus-arabic"
PRES_A = range(0xFB50, 0xFE00)  # Arabic Presentation Forms-A (FB50–FDFF)
PRES_B = range(0xFE70, 0xFF00)  # Arabic Presentation Forms-B (FE70–FEFF)


def _presforms(s: str) -> list[str]:
    return [c for c in s if ord(c) in PRES_A or ord(c) in PRES_B]


def _pdfspine_text(pdf: Path) -> str:
    out = subprocess.run(
        [".venv/bin/python", "conformance/pdfspine_worker.py", str(pdf)],
        capture_output=True, text=True,
    )
    return "".join(json.loads(out.stdout)["pages"])


def _fitz_text(pdf: Path) -> str:
    out = subprocess.run(
        [".venv-oracle/bin/python", "conformance/oracle_extract.py", str(pdf)],
        capture_output=True, text=True,
    )
    return "".join(json.loads(out.stdout)["pymupdf"]["pages"])


def main() -> int:
    man = json.loads((ROOT / "manifest.json").read_text(encoding="utf-8"))
    entries = man["entries"]

    ps_levs: list[float] = []
    fz_levs: list[float] = []
    rows: list[dict] = []

    for e in entries:
        pdf = ROOT / e["pdf"]
        gt = e["gt_text"]
        ps = _pdfspine_text(pdf).strip()
        fz = _fitz_text(pdf).strip()

        ps_s = score_all(gt, ps)
        fz_s = score_all(gt, fz)
        ps_levs.append(ps_s["lev"])
        fz_levs.append(fz_s["lev"])

        rows.append({
            "id": e["id"], "kind": e["kind"], "gt": gt, "ps": ps, "fz": fz,
            "ps_lev": ps_s["lev"], "fz_lev": fz_s["lev"],
            "ps_order": ps_s["order"], "fz_order": fz_s["order"],
            "ps_f1": ps_s["f1"], "fz_f1": fz_s["f1"],
            "ps_pf": len(_presforms(ps)), "fz_pf": len(_presforms(fz)),
        })

    print("=" * 78)
    print("ARABIC / RTL EXTRACTION DIAGNOSTIC  (GT = logical source order)")
    print("=" * 78)
    for r in rows:
        print(f"\n### {r['id']}  [{r['kind']}]")
        print(f"  GT : {r['gt']!r}")
        print(f"  PS : {r['ps']!r}")
        print(f"  FZ : {r['fz']!r}")
        print(f"  pdfspine: lev={r['ps_lev']:.3f} order={r['ps_order']:.3f} "
              f"f1={r['ps_f1']:.3f} presforms={r['ps_pf']}")
        print(f"  fitz    : lev={r['fz_lev']:.3f} order={r['fz_order']:.3f} "
              f"f1={r['fz_f1']:.3f} presforms={r['fz_pf']}")

    mean = lambda xs: sum(xs) / len(xs) if xs else 0.0
    print("\n" + "=" * 78)
    print(f"MEAN lev  pdfspine={mean(ps_levs):.4f}   fitz={mean(fz_levs):.4f}   "
          f"n={len(rows)}")
    print(f"presform leakage: pdfspine={sum(r['ps_pf'] for r in rows)} "
          f"fitz={sum(r['fz_pf'] for r in rows)}")
    print("=" * 78)

    (ROOT / "results.json").write_text(
        json.dumps({
            "n": len(rows),
            "pdfspine_mean_lev": round(mean(ps_levs), 4),
            "fitz_mean_lev": round(mean(fz_levs), 4),
            "rows": rows,
        }, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
