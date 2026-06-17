# Table-extraction differential — oxide-pdf vs fitz

Harness: `/Users/linhan/workspace/pypdf/conformance/gt/tables_diff.py`  
oxide python: `/Users/linhan/workspace/pypdf/.venv/bin/python`  
fitz python:  `/Users/linhan/workspace/pypdf/.venv-oracle/bin/python` (PyMuPDF 1.27.2.3)  
Match rule: bbox IoU > 0.5; grid-shape = exact (rows,cols); cell-text = token-F1 (`gt/score.py`) of flattened cells, oxide-vs-fitz.

## Aggregate (oxide vs fitz)

- Documents scored: **30** (0 open-errors), pages: **1581**
- Tables detected: oxide **1537**, fitz **170** (ratio oxide/fitz = 9.04)
- **Table-count agreement** (per-page #oxide==#fitz): **8.9%** (141/1581 pages)
- Tables matched by IoU>0.5: **41** (= 3% of oxide, 24% of fitz tables)
- **Grid-shape match** on matched pairs (exact rows×cols): **2.4%** (1/41)
- **Mean cell-text F1** on matched pairs: **0.806**

## Per-document

| doc | pages | ox tbl | fz tbl | cnt-agree | matched | shape% | cell-F1 |
|-----|------:|-------:|-------:|----------:|--------:|-------:|--------:|
| cdc-mmwr-7301a1.pdf | 5 | 5 | 0 | 0% | 0 | — | — |
| cdc-mmwr-7302a1.pdf | 5 | 5 | 0 | 0% | 0 | — | — |
| govinfo-hjres1.pdf | 2 | 2 | 0 | 0% | 0 | — | — |
| govinfo-hr1.pdf | 175 | 175 | 0 | 0% | 0 | — | — |
| govinfo-hr2.pdf | 213 | 213 | 0 | 0% | 0 | — | — |
| govinfo-hr3056.pdf | 15 | 15 | 0 | 0% | 0 | — | — |
| govinfo-hr815enr.pdf | 110 | 110 | 0 | 0% | 0 | — | — |
| govinfo-s1.pdf | 607 | 607 | 0 | 0% | 0 | — | — |
| irs-f4868.pdf | 4 | 4 | 2 | 0% | 0 | — | — |
| irs-f8843.pdf | 4 | 4 | 6 | 0% | 0 | — | — |
| irs-fw4.pdf | 5 | 5 | 6 | 0% | 0 | — | — |
| irs-p502.pdf | 27 | 27 | 0 | 0% | 0 | — | — |
| usgs-fs20183024.pdf | 6 | 6 | 0 | 0% | 0 | — | — |
| irs-f1040sb.pdf | 1 | 1 | 2 | 0% | 1 | 0% | 0.224 |
| irs-f8949.pdf | 2 | 2 | 4 | 0% | 2 | 0% | 0.506 |
| irs-fw7.pdf | 1 | 1 | 2 | 0% | 1 | 0% | 0.758 |
| irs-f1040sc.pdf | 2 | 2 | 5 | 0% | 1 | 0% | 0.837 |
| irs-f1040.pdf | 2 | 2 | 7 | 0% | 2 | 0% | 0.866 |
| cdc-mmwr-7251a1.pdf | 8 | 7 | 0 | 12% | 0 | — | — |
| irs-p501.pdf | 31 | 31 | 9 | 16% | 4 | 0% | 0.777 |
| irs-p15.pdf | 59 | 59 | 13 | 19% | 7 | 0% | 0.997 |
| nist-sp800-63-3.pdf | 76 | 76 | 22 | 24% | 7 | 0% | 0.615 |
| irs-f941.pdf | 3 | 3 | 3 | 33% | 0 | — | — |
| irs-fw9.pdf | 6 | 6 | 8 | 33% | 0 | — | — |
| nasa-ntrs-19950009349.pdf | 107 | 68 | 0 | 36% | 0 | — | — |
| irs-f2848.pdf | 2 | 2 | 11 | 50% | 0 | — | — |
| irs-f1065.pdf | 6 | 6 | 12 | 50% | 5 | 0% | 0.916 |
| govinfo-cdoc110-50.pdf | 85 | 81 | 47 | 60% | 1 | 0% | 0.125 |
| irs-f1099msc.pdf | 6 | 6 | 4 | 67% | 4 | 0% | 0.995 |
| irs-f1120.pdf | 6 | 6 | 7 | 83% | 6 | 17% | 0.900 |

## Worst divergences (one-line cause guess)

- **irs-f1040sb.pdf** — ox 1 / fz 2 tables, shape 0%, cellF1 0.224 → _oxide under-segments: 1 vs fitz 2 tables (merges/misses)_
- **irs-f8949.pdf** — ox 2 / fz 4 tables, shape 0%, cellF1 0.506 → _oxide under-segments: 2 vs fitz 4 tables (merges/misses)_
- **irs-fw7.pdf** — ox 1 / fz 2 tables, shape 0%, cellF1 0.758 → _oxide under-segments: 1 vs fitz 2 tables (merges/misses)_
- **irs-f1040sc.pdf** — ox 2 / fz 5 tables, shape 0%, cellF1 0.837 → _oxide under-segments: 2 vs fitz 5 tables (merges/misses)_
- **irs-f1040.pdf** — ox 2 / fz 7 tables, shape 0%, cellF1 0.866 → _oxide under-segments: 2 vs fitz 7 tables (merges/misses)_
- **cdc-mmwr-7301a1.pdf** — ox 5 / fz 0 tables, shape —, cellF1 — → _oxide finds tables where fitz finds none (over-detection)_
- **cdc-mmwr-7302a1.pdf** — ox 5 / fz 0 tables, shape —, cellF1 — → _oxide finds tables where fitz finds none (over-detection)_
- **govinfo-hjres1.pdf** — ox 2 / fz 0 tables, shape —, cellF1 — → _oxide finds tables where fitz finds none (over-detection)_

## Verdict

**Weak parity.** oxide's table detection diverges substantially from fitz on this corpus — treat `find_tables` as experimental.
- oxide **over-detects**: 1537 tables vs fitz 170 (splitting one table into many / spurious detections).
- Of matched (overlapping) tables, exact grid shape agrees 2% of the time and cell text scores F1 0.81.

## Notes / follow-ups

- This is parity-with-fitz *agreement*, not accuracy vs a human gold; fitz table detection is itself heuristic (lattice/stream) and imperfect.
- FinTabNet structural ground truth was considered as an objective anchor but **skipped** to keep the run self-contained and fast (HF fetch is heavy/flaky); add it later for an absolute structure score.
- All numbers are oxide-vs-fitz; no Rust changes were made.