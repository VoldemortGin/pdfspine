# Table structure eval set — pdfspine `find_tables` vs FinTabNet.c gold (GriTS / TEDS-Struct / cell-F1)

> Historical source-annotation benchmark from 2026-09-08. This is not a current validation run or human-reviewed acceptance; the current review/provenance gates in docs/PRD-NEXT.md remain authoritative.
Harness: `conformance/gt/eval_tables.py run` — backends `lines`, `text`, `onnx`, plus the PyMuPDF oracle; modes `e2e` and `gold-crop`.  
Metrics: **GriTS_Top / GriTS_Con** (`conformance/gt/grits.py`), **TEDS-Struct** and **cell-alignment F1** (`conformance/gt/table_metrics.py`).  
Dataset: **FinTabNet.c** — 150 pages / 186 structure-eligible gold tables; annotations `CDLA-Permissive-2.0`, source PDFs `CDLA-Permissive-1.0`.  
Code: branch `feat/table-eval-set` on `72b1d4a`; pdfspine `0.7.1`; onnxruntime 1.29.0, **CPUExecutionProvider** (`auto` never selects CoreML).  
Scored: 2026-09-08 on an Apple M-series mini (10 cores, `--jobs 5`). The scoring host held an rsync'd copy of the tree, so the harness recorded `commit: null`; the commit is stated here instead.

## Why these three metrics

They disagree on purpose, and the disagreements are the point.

**GriTS** (Smock et al., arXiv:2303.00716) is the canonical FinTabNet.c metric, so it is the only one of the three directly comparable with published Table-Transformer numbers. It scores a grid alignment with per-cell partial credit: `Top` on cell topology, `Con` on cell content.

**TEDS-Struct** is the PubTabNet / Docling metric with text removed — tree edit distance over the `<table>/<tr>/<td colspan rowspan>` tree. It charges a flat price per structural edit where GriTS hands out partial credit, so a single merged header row is one TEDS edit but a whole row of degraded GriTS cells. Where TEDS-S and GriTS diverge, the error is concentrated rather than spread.

**Cell-alignment F1** is the only one of the three that is *not* position-invariant: predicted cells are matched one-to-one against gold cells at bbox IoU >= 0.5. GriTS and TEDS both score a perfectly-shaped grid drawn in the wrong place as perfect. This one does not.

That last point is the main finding below.

## Sample / provenance / license

- Pages scored: **150**; gold tables: **186** (every `exclude_for_structure=false` table in the slice).
- Annotations license: **CDLA-Permissive-2.0**; source-PDF license: **CDLA-Permissive-1.0**. Only permissively-licensed data is used; the corpus itself is gitignored (`conformance/gt/corpus-*/`) — the committed deliverables are the harness, the metrics, this report and the seed subset.
- Missed gold tables score 0 on every metric (**recall-weighted**, the FinTabNet.c convention). `m/` columns are **matched-only** means, i.e. structure quality *given* detection.
- TEDS-S `None` (tree over `--teds-max-nodes` or past `--teds-timeout`) is counted in `teds-skip` and never as 0. No table hit either limit in this run.
- `tatr` was requested and skipped cleanly: no TATR weights are held locally.

## Aggregate

| backend | mode | gold | pred | matched | det-F1 | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | m/GriTS_Con | m/TEDS-S | teds-skip | errors | s/table |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| lines | e2e | 186 | 194 | 17 | 0.089 | 0.045 | 0.045 | 0.041 | 0.042 | 0.495 | 0.450 | 0 | 0 | 0.003 |
| text | e2e | 186 | 148 | 25 | 0.150 | 0.038 | 0.035 | 0.070 | 0.028 | 0.260 | 0.520 | 0 | 0 | 0.003 |
| onnx | e2e | 186 | 236 | 166 | 0.787 | 0.782 | 0.692 | 0.759 | 0.319 | 0.775 | 0.851 | 0 | 0 | 2.197 |
| fitz-oracle | e2e | 186 | 182 | 18 | 0.098 | 0.046 | 0.040 | 0.046 | 0.043 | 0.418 | 0.476 | 0 | 0 | 0.159 |
| lines | gold-crop | 186 | 220 | 41 | n/a | 0.076 | 0.072 | 0.075 | 0.062 | 0.326 | 0.338 | 0 | 0 | 0.003 |
| text | gold-crop | 186 | 184 | 184 | n/a | 0.185 | 0.125 | 0.255 | 0.076 | 0.127 | 0.258 | 0 | 0 | 0.004 |
| onnx | gold-crop | 186 | 186 | 186 | n/a | 0.863 | 0.766 | 0.836 | 0.371 | 0.766 | 0.836 | 0 | 0 | 0.177 |

Recall-weighted means unless prefixed `m/` (matched-only). `det-F1` is table-detection F1 and is meaningless in `gold-crop`, where the gold bbox is supplied.

**`gold-crop` honours `clip` only for the ONNX backend.** pdfspine forwards `clip=` to the vision
backends only; the native `lines`/`text` strategies ignore it and run whole-page detection, after
which the harness takes the best-IoU table. Their gold-crop rows are therefore *not* TSR-only
numbers and must not be read beside Microsoft's ~0.98 — each run records `clip_honored` in the
JSON for exactly this reason. Only the `onnx/gold-crop` row is a true structure-stage measurement.

### By tag — lines / e2e

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.039 | 0.036 | 0.033 | 0.028 |
| borderless | 139 | 0.000 | 0.000 | 0.000 | 0.000 |
| multi-header | 92 | 0.047 | 0.049 | 0.042 | 0.043 |
| spanning | 126 | 0.048 | 0.050 | 0.045 | 0.049 |
| wide | 15 | 0.058 | 0.065 | 0.048 | 0.068 |
| tall | 22 | 0.037 | 0.037 | 0.044 | 0.049 |
| multi-table | 64 | 0.065 | 0.067 | 0.056 | 0.054 |

### By tag — text / e2e

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.018 | 0.018 | 0.033 | 0.009 |
| borderless | 139 | 0.040 | 0.037 | 0.072 | 0.027 |
| multi-header | 92 | 0.035 | 0.031 | 0.062 | 0.024 |
| spanning | 126 | 0.048 | 0.043 | 0.088 | 0.038 |
| wide | 15 | 0.025 | 0.023 | 0.044 | 0.016 |
| tall | 22 | 0.247 | 0.224 | 0.435 | 0.214 |
| multi-table | 64 | 0.006 | 0.006 | 0.011 | 0.003 |

### By tag — onnx / e2e

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.810 | 0.712 | 0.790 | 0.325 |
| borderless | 139 | 0.775 | 0.670 | 0.754 | 0.301 |
| multi-header | 92 | 0.783 | 0.700 | 0.753 | 0.343 |
| spanning | 126 | 0.769 | 0.682 | 0.745 | 0.317 |
| wide | 15 | 0.774 | 0.715 | 0.747 | 0.252 |
| tall | 22 | 0.753 | 0.652 | 0.732 | 0.290 |
| multi-table | 64 | 0.763 | 0.696 | 0.741 | 0.322 |

### By tag — fitz-oracle / e2e

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.039 | 0.033 | 0.036 | 0.031 |
| borderless | 139 | 0.000 | 0.000 | 0.000 | 0.000 |
| multi-header | 92 | 0.047 | 0.042 | 0.049 | 0.043 |
| spanning | 126 | 0.049 | 0.044 | 0.051 | 0.049 |
| wide | 15 | 0.056 | 0.049 | 0.048 | 0.068 |
| tall | 22 | 0.053 | 0.035 | 0.077 | 0.051 |
| multi-table | 64 | 0.064 | 0.056 | 0.058 | 0.054 |

### By tag — lines / gold-crop

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.081 | 0.076 | 0.077 | 0.053 |
| borderless | 139 | 0.000 | 0.000 | 0.000 | 0.000 |
| multi-header | 92 | 0.078 | 0.074 | 0.077 | 0.063 |
| spanning | 126 | 0.073 | 0.070 | 0.073 | 0.066 |
| wide | 15 | 0.086 | 0.093 | 0.077 | 0.101 |
| tall | 22 | 0.074 | 0.069 | 0.073 | 0.071 |
| multi-table | 64 | 0.079 | 0.081 | 0.068 | 0.065 |

### By tag — text / gold-crop

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.146 | 0.094 | 0.190 | 0.038 |
| borderless | 139 | 0.189 | 0.125 | 0.257 | 0.070 |
| multi-header | 92 | 0.197 | 0.135 | 0.278 | 0.081 |
| spanning | 126 | 0.204 | 0.141 | 0.286 | 0.093 |
| wide | 15 | 0.195 | 0.139 | 0.258 | 0.046 |
| tall | 22 | 0.345 | 0.297 | 0.574 | 0.286 |
| multi-table | 64 | 0.164 | 0.100 | 0.203 | 0.039 |

### By tag — onnx / gold-crop

| tag | n | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 |
|---|---:|---:|---:|---:|---:|
| plain | 60 | 0.895 | 0.795 | 0.870 | 0.398 |
| borderless | 139 | 0.863 | 0.755 | 0.838 | 0.362 |
| multi-header | 92 | 0.847 | 0.768 | 0.817 | 0.369 |
| spanning | 126 | 0.847 | 0.752 | 0.820 | 0.359 |
| wide | 15 | 0.841 | 0.799 | 0.812 | 0.306 |
| tall | 22 | 0.798 | 0.687 | 0.772 | 0.315 |
| multi-table | 64 | 0.873 | 0.798 | 0.853 | 0.389 |

## Per-document


### lines / e2e

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.018 | spanning, wide |
| ADI_2010_page_51 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.294 | 0.353 | 0.473 | 0.582 | 0.004 | spanning, tall |
| AEE_2007_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.024 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.022 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.024 | plain, borderless |
| AES_2016_page_188 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.018 | plain, borderless |
| AFL_2009_page_40 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain, borderless |
| AIG_2010_page_258 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| AIG_2012_page_244 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| AIZ_2005_page_142 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| AWK_2013_page_131 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| BIIB_2008_page_47 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, spanning |
| BMY_2008_page_82 | 1 | 1 | 0.980 | 0.951 | 0.962 | 0.831 | 0.003 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 1 | 0.333 | 0.296 | 0.281 | 0.167 | 0.002 | plain, multi-table |
| CF_2015_page_32 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| CMI_2014_page_81 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.345 | 0.360 | 0.399 | 0.457 | 0.009 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| EFX_2017_page_112 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.172 | 0.093 | 0.094 | 0.050 | 0.001 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless |
| FE_2010_page_23 | 1 | 1 | 0.568 | 0.496 | 0.463 | 0.494 | 0.004 | plain |
| FITB_2008_page_21 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.008 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| FLS_2012_page_67 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| GD_2004_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| GE_2012_page_148 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| GM_2010_page_167 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| GPN_2002_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| HBAN_2009_page_169 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| HOLX_2010_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | spanning |
| HOLX_2015_page_36 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.009 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| HPE_2016_page_196 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| HUM_2015_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| HWM_2016_page_140 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| INCY_2007_page_90 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| IRM_2010_page_116 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| KIM_2010_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | spanning |
| KMB_2010_page_18 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| KO_2013_page_150 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.727 | 0.716 | 0.739 | 0.889 | 0.005 | spanning |
| LKQ_2009_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, spanning |
| MAR_2015_page_101 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MA_2016_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.010 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 3 | 0.424 | 0.486 | 0.359 | 0.495 | 0.005 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| MNST_2006_page_107 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain |
| MNST_2015_page_100 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| MO_2017_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain |
| MPC_2018_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MRK_2012_page_57 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MRO_2006_page_84 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| NEM_2008_page_75 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PEP_2015_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| PG_2012_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain |
| PM_2015_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| PM_2017_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain |
| PNC_2012_page_267 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PNC_2015_page_192 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PNW_2015_page_198 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| PRU_2005_page_83 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| PXD_2005_page_109 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.008 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 1 | 0.389 | 0.413 | 0.310 | 0.330 | 0.005 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 1 | 0.267 | 0.254 | 0.209 | 0.275 | 0.002 | plain, multi-table |
| UNP_2012_page_64 | 3 | 3 | 0.559 | 0.576 | 0.516 | 0.366 | 0.004 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 1 | 0.368 | 0.349 | 0.278 | 0.365 | 0.003 | spanning |
| UNP_2013_page_71 | 1 | 1 | 0.410 | 0.394 | 0.333 | 0.427 | 0.006 | spanning |
| VAR_2012_page_122 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | multi-header, spanning |
| V_2009_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| WRB_2016_page_127 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain |
| XEL_2005_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| XEL_2009_page_163 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.009 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| ZBH_2003_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, tall |

### text / e2e

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.018 | spanning, wide |
| ADI_2010_page_51 | 1 | 1 | 0.252 | 0.258 | 0.612 | 0.323 | 0.003 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.025 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.156 | 0.167 | 0.627 | 0.287 | 0.024 | spanning, tall |
| AEE_2007_page_125 | 1 | 1 | 0.121 | 0.126 | 0.611 | 0.016 | 0.026 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.019 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| AES_2016_page_188 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| AFL_2009_page_40 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| AIG_2010_page_258 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| AIG_2012_page_244 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 1 | 0.441 | 0.333 | 0.622 | 0.167 | 0.006 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| AIZ_2005_page_142 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 1 | 0.299 | 0.284 | 0.553 | 0.035 | 0.002 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| AWK_2013_page_131 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 1 | 0.483 | 0.438 | 0.515 | 0.038 | 0.010 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| BIIB_2008_page_47 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, spanning |
| BMY_2008_page_82 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, multi-table |
| CF_2015_page_32 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| CMI_2014_page_81 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.505 | 0.435 | 0.578 | 0.123 | 0.005 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| EFX_2017_page_112 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.247 | 0.271 | 0.718 | 0.565 | 0.003 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless |
| FE_2010_page_23 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain |
| FITB_2008_page_21 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.008 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| FLS_2012_page_67 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 1 | 0.126 | 0.125 | 0.429 | 0.000 | 0.002 | borderless, spanning |
| GD_2004_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless |
| GE_2012_page_148 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.010 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| GM_2010_page_167 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| GPN_2002_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| HBAN_2009_page_169 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain |
| HOLX_2010_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.008 | spanning |
| HOLX_2015_page_36 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, spanning |
| HPE_2016_page_196 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| HUM_2015_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning |
| HWM_2016_page_140 | 1 | 1 | 0.343 | 0.377 | 0.453 | 0.000 | 0.002 | plain, borderless |
| INCY_2007_page_90 | 2 | 1 | 0.100 | 0.114 | 0.194 | 0.065 | 0.002 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| IRM_2010_page_116 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| KIM_2010_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | spanning |
| KMB_2010_page_18 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| KO_2013_page_150 | 1 | 1 | 0.207 | 0.158 | 0.340 | 0.069 | 0.002 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.055 | 0.029 | 0.103 | 0.000 | 0.003 | spanning |
| LKQ_2009_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning |
| MAR_2015_page_101 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| MA_2016_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 1 | 0.080 | 0.066 | 0.384 | 0.000 | 0.002 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 1 | 0.369 | 0.352 | 0.657 | 0.244 | 0.008 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless |
| MNST_2006_page_107 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain |
| MNST_2015_page_100 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MO_2017_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| MPC_2018_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MRK_2012_page_57 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MRO_2006_page_84 | 1 | 1 | 0.291 | 0.285 | 0.806 | 0.423 | 0.004 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 1 | 0.658 | 0.613 | 0.791 | 0.668 | 0.003 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| NEM_2008_page_75 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 1 | 0.470 | 0.377 | 0.586 | 0.056 | 0.003 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| PEP_2015_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| PG_2012_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain |
| PM_2015_page_106 | 1 | 1 | 0.132 | 0.141 | 0.271 | 0.342 | 0.008 | plain |
| PM_2017_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain |
| PNC_2012_page_267 | 1 | 1 | 0.055 | 0.049 | 0.190 | 0.000 | 0.003 | plain, borderless |
| PNC_2015_page_192 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PNW_2015_page_198 | 1 | 1 | 0.136 | 0.116 | 0.316 | 0.000 | 0.003 | plain, borderless |
| PRU_2005_page_83 | 1 | 1 | 0.498 | 0.430 | 0.714 | 0.752 | 0.009 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| PXD_2005_page_109 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.007 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, multi-table |
| UNP_2012_page_64 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | spanning |
| UNP_2013_page_71 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | spanning |
| VAR_2012_page_122 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning |
| V_2009_page_105 | 1 | 1 | 0.372 | 0.272 | 0.605 | 0.250 | 0.004 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| WRB_2016_page_127 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain |
| XEL_2005_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| XEL_2009_page_163 | 2 | 1 | 0.082 | 0.093 | 0.163 | 0.031 | 0.008 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 1 | 0.414 | 0.388 | 0.807 | 0.737 | 0.014 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| ZBH_2003_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, tall |

### onnx / e2e

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 1 | 0.631 | 0.568 | 0.539 | 0.078 | 0.830 | spanning, wide |
| ADI_2010_page_51 | 1 | 1 | 0.730 | 0.562 | 0.781 | 0.338 | 1.733 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 1 | 0.925 | 0.891 | 0.943 | 0.281 | 3.610 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.882 | 0.875 | 0.853 | 0.445 | 4.390 | spanning, tall |
| AEE_2007_page_125 | 1 | 1 | 0.913 | 0.875 | 0.906 | 0.829 | 3.605 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 2 | 0.832 | 0.673 | 0.797 | 0.198 | 3.932 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 1 | 0.933 | 0.789 | 0.892 | 0.333 | 2.230 | plain, borderless |
| AES_2016_page_188 | 1 | 1 | 0.944 | 0.834 | 0.920 | 0.400 | 2.548 | plain, borderless |
| AFL_2009_page_40 | 1 | 1 | 1.000 | 0.815 | 1.000 | 0.000 | 2.943 | plain, borderless |
| AIG_2010_page_258 | 1 | 1 | 0.963 | 0.866 | 0.971 | 0.552 | 2.685 | borderless, spanning |
| AIG_2012_page_244 | 1 | 1 | 0.901 | 0.837 | 0.878 | 0.590 | 3.099 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 1 | 0.883 | 0.676 | 0.838 | 0.037 | 3.279 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 1 | 0.909 | 0.736 | 0.886 | 0.000 | 2.919 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 1 | 1.000 | 0.974 | 1.000 | 0.700 | 2.679 | plain |
| AIZ_2005_page_142 | 1 | 1 | 0.623 | 0.308 | 0.672 | 0.256 | 2.525 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 1 | 0.982 | 0.828 | 0.969 | 0.255 | 1.884 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 3 | 0.645 | 0.523 | 0.615 | 0.101 | 2.872 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 1 | 0.893 | 0.583 | 0.897 | 0.148 | 2.597 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 1 | 0.932 | 0.904 | 0.907 | 0.374 | 2.689 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 1 | 0.960 | 0.917 | 0.931 | 0.667 | 2.464 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 1 | 0.923 | 0.653 | 0.926 | 0.162 | 2.676 | borderless, spanning |
| AWK_2013_page_131 | 1 | 1 | 1.000 | 0.992 | 1.000 | 0.586 | 2.757 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 1 | 0.824 | 0.710 | 0.769 | 0.000 | 3.209 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 1 | 0.875 | 0.852 | 0.874 | 0.292 | 3.042 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 2 | 0.995 | 1.000 | 0.992 | 0.074 | 2.961 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 1 | 1.000 | 0.670 | 1.000 | 0.429 | 2.456 | plain, borderless |
| BIIB_2008_page_47 | 2 | 2 | 0.971 | 0.887 | 0.961 | 0.650 | 1.950 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 1 | 0.850 | 0.663 | 0.843 | 0.000 | 2.516 | borderless, spanning |
| BMY_2008_page_82 | 1 | 1 | 1.000 | 0.971 | 1.000 | 0.432 | 3.116 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 1 | 0.480 | 0.409 | 0.362 | 0.000 | 2.432 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 2 | 1.000 | 0.983 | 1.000 | 0.542 | 2.651 | plain, multi-table |
| CF_2015_page_32 | 2 | 2 | 0.812 | 0.748 | 0.770 | 0.113 | 3.020 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 1 | 0.750 | 0.760 | 0.708 | 0.492 | 2.603 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 1 | 0.933 | 0.283 | 0.878 | 0.133 | 2.805 | plain, borderless |
| CMI_2014_page_81 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.448 | 2.628 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.863 | 0.682 | 0.853 | 0.320 | 3.129 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 1 | 0.860 | 0.723 | 0.754 | 0.172 | 2.900 | plain, borderless |
| EFX_2017_page_112 | 2 | 2 | 0.832 | 0.806 | 0.777 | 0.143 | 3.004 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 2 | 0.820 | 0.777 | 0.701 | 0.146 | 2.974 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 2 | 0.764 | 0.731 | 0.698 | 0.351 | 2.628 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 1 | 0.952 | 0.827 | 0.912 | 0.286 | 2.024 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.517 | 0.451 | 0.363 | 0.235 | 2.850 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 2 | 0.512 | 0.532 | 0.508 | 0.161 | 2.362 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 2 | 0.720 | 0.581 | 0.731 | 0.016 | 2.695 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 1 | 1.000 | 0.934 | 1.000 | 0.250 | 2.755 | plain, borderless |
| FE_2010_page_23 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.407 | 2.506 | plain |
| FITB_2008_page_21 | 1 | 1 | 0.911 | 0.911 | 0.912 | 0.515 | 2.866 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 1 | 1.000 | 0.815 | 1.000 | 0.857 | 2.932 | plain, borderless |
| FLS_2012_page_67 | 1 | 1 | 0.601 | 0.452 | 0.612 | 0.170 | 3.177 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 1 | 0.880 | 0.845 | 0.885 | 0.050 | 3.103 | borderless, spanning |
| GD_2004_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.425 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 1 | 1.000 | 0.744 | 1.000 | 0.000 | 2.376 | plain, borderless |
| GE_2012_page_148 | 1 | 1 | 0.875 | 0.856 | 0.862 | 0.833 | 2.780 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 1 | 0.889 | 0.734 | 0.842 | 0.148 | 2.678 | plain, borderless |
| GM_2010_page_167 | 1 | 1 | 1.000 | 0.643 | 1.000 | 0.250 | 2.615 | plain, borderless |
| GPN_2002_page_48 | 1 | 1 | 1.000 | 0.551 | 1.000 | 0.333 | 2.573 | plain, borderless |
| HBAN_2009_page_169 | 3 | 3 | 0.879 | 0.642 | 0.906 | 0.208 | 3.544 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 1 | 0.958 | 0.946 | 0.931 | 0.651 | 2.790 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 1 | 0.764 | 0.719 | 0.710 | 0.084 | 2.594 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 1 | 0.902 | 0.897 | 0.896 | 0.815 | 2.668 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 1 | 0.971 | 1.000 | 0.947 | 0.762 | 2.998 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 1 | 0.667 | 0.483 | 0.688 | 0.000 | 2.456 | plain |
| HOLX_2010_page_70 | 1 | 1 | 1.000 | 0.970 | 1.000 | 0.820 | 2.656 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 3.069 | spanning |
| HOLX_2015_page_36 | 2 | 2 | 1.000 | 0.998 | 1.000 | 0.617 | 3.059 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.627 | borderless, spanning |
| HPE_2016_page_196 | 1 | 1 | 0.760 | 0.387 | 0.680 | 0.235 | 2.784 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 2 | 0.776 | 0.628 | 0.718 | 0.238 | 2.923 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 1 | 0.905 | 0.728 | 0.897 | 0.250 | 2.875 | borderless, spanning |
| HUM_2015_page_113 | 1 | 1 | 0.687 | 0.629 | 0.622 | 0.069 | 2.847 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 1 | 0.714 | 0.664 | 0.679 | 0.061 | 3.203 | borderless, spanning |
| HWM_2016_page_140 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.566 | plain, borderless |
| INCY_2007_page_90 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.558 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 1 | 0.961 | 0.877 | 0.938 | 0.039 | 2.825 | plain |
| IRM_2010_page_116 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.827 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 3 | 1.000 | 0.906 | 1.000 | 0.772 | 2.962 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 1 | 0.887 | 0.868 | 0.878 | 0.386 | 2.422 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 2 | 0.839 | 0.829 | 0.775 | 0.277 | 2.917 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 1 | 0.656 | 0.445 | 0.585 | 0.000 | 2.841 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 1 | 1.000 | 0.989 | 1.000 | 0.525 | 2.582 | plain, borderless |
| KIM_2010_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.761 | spanning |
| KMB_2010_page_18 | 1 | 1 | 0.877 | 0.788 | 0.879 | 0.680 | 2.730 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.225 | plain, borderless |
| KO_2013_page_150 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.440 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.727 | 0.716 | 0.739 | 0.741 | 2.548 | spanning |
| LKQ_2009_page_69 | 1 | 1 | 0.958 | 0.744 | 0.929 | 0.585 | 2.506 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 1 | 0.839 | 0.684 | 0.800 | 0.337 | 2.891 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 1 | 0.660 | 0.666 | 0.594 | 0.217 | 2.752 | borderless, spanning |
| MAR_2015_page_101 | 1 | 1 | 1.000 | 0.886 | 1.000 | 0.333 | 2.858 | plain, borderless |
| MA_2016_page_69 | 1 | 1 | 0.561 | 0.425 | 0.472 | 0.172 | 3.159 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 1 | 0.952 | 0.855 | 0.939 | 0.683 | 2.615 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 1 | 0.814 | 0.717 | 0.828 | 0.475 | 3.245 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 3 | 0.892 | 0.850 | 0.916 | 0.560 | 3.189 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.000 | 2.521 | plain, borderless |
| MNST_2006_page_107 | 1 | 1 | 0.857 | 0.687 | 0.810 | 0.357 | 2.441 | plain |
| MNST_2015_page_100 | 1 | 1 | 0.923 | 0.754 | 0.864 | 0.308 | 2.351 | plain, borderless |
| MO_2017_page_72 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.111 | 2.566 | plain |
| MPC_2018_page_111 | 1 | 1 | 0.636 | 0.686 | 0.591 | 0.200 | 2.762 | plain, borderless |
| MRK_2012_page_57 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.938 | 2.588 | plain, borderless |
| MRO_2006_page_84 | 1 | 1 | 0.765 | 0.705 | 0.710 | 0.147 | 2.563 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 3.047 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 1 | 0.889 | 0.691 | 0.891 | 0.314 | 3.354 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.288 | plain, borderless |
| NEM_2008_page_75 | 1 | 1 | 0.817 | 0.843 | 0.711 | 0.169 | 2.389 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.933 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 1 | 0.729 | 0.722 | 0.653 | 0.036 | 2.452 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 1 | 0.833 | 0.724 | 0.793 | 0.400 | 2.648 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 1 | 0.958 | 0.566 | 0.939 | 0.553 | 2.319 | plain, borderless |
| PEP_2015_page_89 | 1 | 1 | 0.962 | 0.854 | 0.950 | 0.118 | 1.906 | borderless, spanning |
| PG_2012_page_30 | 1 | 1 | 1.000 | 0.996 | 1.000 | 0.722 | 3.069 | plain |
| PM_2015_page_106 | 1 | 1 | 0.526 | 0.481 | 0.323 | 0.484 | 2.846 | plain |
| PM_2017_page_77 | 1 | 1 | 0.957 | 0.931 | 0.918 | 0.609 | 2.473 | plain |
| PNC_2012_page_267 | 1 | 1 | 1.000 | 0.998 | 1.000 | 0.625 | 2.692 | plain, borderless |
| PNC_2015_page_192 | 3 | 2 | 0.595 | 0.590 | 0.576 | 0.326 | 2.770 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.167 | 2.137 | plain, borderless |
| PNW_2015_page_198 | 1 | 1 | 1.000 | 0.967 | 1.000 | 0.560 | 2.250 | plain, borderless |
| PRU_2005_page_83 | 1 | 1 | 0.915 | 0.600 | 0.905 | 0.415 | 3.718 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 1 | 0.944 | 0.622 | 0.929 | 0.857 | 2.310 | borderless, spanning |
| PXD_2005_page_109 | 1 | 1 | 0.952 | 0.961 | 0.909 | 0.424 | 2.767 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 1 | 0.991 | 0.989 | 0.984 | 0.819 | 2.531 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 2 | 0.757 | 0.740 | 0.703 | 0.148 | 2.391 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 1 | 0.911 | 0.613 | 0.914 | 0.157 | 2.008 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 1 | 0.889 | 0.422 | 0.783 | 0.133 | 2.220 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 1 | 0.904 | 0.817 | 0.909 | 0.286 | 2.936 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 1 | 0.959 | 0.889 | 0.935 | 0.166 | 2.688 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 2 | 0.944 | 0.923 | 0.940 | 0.065 | 2.612 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 3 | 0.924 | 0.765 | 0.895 | 0.608 | 2.896 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 1 | 0.988 | 0.731 | 0.979 | 0.379 | 2.866 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 1 | 0.884 | 0.882 | 0.824 | 0.510 | 3.192 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 1 | 0.848 | 0.752 | 0.747 | 0.429 | 2.606 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 1 | 0.875 | 0.782 | 0.846 | 0.500 | 2.504 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 2 | 0.967 | 0.974 | 0.941 | 0.749 | 2.649 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 1 | 0.885 | 0.878 | 0.873 | 0.652 | 2.979 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 2 | 0.938 | 0.590 | 0.892 | 0.555 | 3.431 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 1 | 0.400 | 0.350 | 0.360 | 0.029 | 2.429 | plain, multi-table |
| UNP_2012_page_64 | 3 | 3 | 0.958 | 0.940 | 0.933 | 0.595 | 2.970 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 1 | 1.000 | 0.954 | 1.000 | 0.106 | 2.848 | spanning |
| UNP_2013_page_71 | 1 | 1 | 0.784 | 0.758 | 0.723 | 0.160 | 2.701 | spanning |
| VAR_2012_page_122 | 1 | 1 | 0.700 | 0.620 | 0.618 | 0.317 | 2.463 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.692 | 2.647 | multi-header, spanning |
| V_2009_page_105 | 1 | 1 | 0.852 | 0.622 | 0.825 | 0.593 | 3.224 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 1 | 0.933 | 0.602 | 0.935 | 0.186 | 2.700 | borderless, spanning |
| WRB_2016_page_127 | 1 | 1 | 1.000 | 0.986 | 1.000 | 0.143 | 2.864 | plain |
| XEL_2005_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.555 | plain, borderless |
| XEL_2009_page_163 | 2 | 1 | 0.432 | 0.429 | 0.442 | 0.400 | 2.594 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 1 | 0.961 | 0.684 | 0.957 | 0.167 | 3.688 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 2.754 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 1 | 0.909 | 0.797 | 0.860 | 0.208 | 2.520 | plain, borderless |
| ZBH_2003_page_69 | 1 | 1 | 0.800 | 0.793 | 0.729 | 0.091 | 2.229 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 1 | 0.766 | 0.698 | 0.709 | 0.588 | 1.683 | multi-header, spanning, tall |

### fitz-oracle / e2e

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.267 | spanning, wide |
| ADI_2010_page_51 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.176 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.207 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.345 | 0.251 | 0.532 | 0.588 | 0.211 | spanning, tall |
| AEE_2007_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.124 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.230 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.139 | plain, borderless |
| AES_2016_page_188 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.187 | plain, borderless |
| AFL_2009_page_40 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.179 | plain, borderless |
| AIG_2010_page_258 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.190 | borderless, spanning |
| AIG_2012_page_244 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.124 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.178 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.128 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.214 | plain |
| AIZ_2005_page_142 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.166 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.157 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.159 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.163 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.164 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.209 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.136 | borderless, spanning |
| AWK_2013_page_131 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.161 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.210 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.186 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.140 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.176 | plain, borderless |
| BIIB_2008_page_47 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.146 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.258 | borderless, spanning |
| BMY_2008_page_82 | 1 | 1 | 0.871 | 0.869 | 0.925 | 0.831 | 0.288 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.161 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 1 | 0.333 | 0.296 | 0.281 | 0.167 | 0.221 | plain, multi-table |
| CF_2015_page_32 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.168 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.196 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.167 | plain, borderless |
| CMI_2014_page_81 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.239 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.257 | 0.193 | 0.526 | 0.444 | 0.292 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.187 | plain, borderless |
| EFX_2017_page_112 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.200 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.291 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.182 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.162 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.132 | 0.052 | 0.122 | 0.050 | 0.149 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.161 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.163 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.229 | plain, borderless |
| FE_2010_page_23 | 1 | 1 | 0.526 | 0.496 | 0.559 | 0.647 | 0.404 | plain |
| FITB_2008_page_21 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.204 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.148 | plain, borderless |
| FLS_2012_page_67 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.141 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.150 | borderless, spanning |
| GD_2004_page_48 | 1 | 1 | 0.443 | 0.274 | 0.511 | 0.038 | 0.123 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.191 | plain, borderless |
| GE_2012_page_148 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.263 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.196 | plain, borderless |
| GM_2010_page_167 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.193 | plain, borderless |
| GPN_2002_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.158 | plain, borderless |
| HBAN_2009_page_169 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.173 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.151 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.121 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.357 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.159 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.241 | plain |
| HOLX_2010_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.301 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.465 | spanning |
| HOLX_2015_page_36 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.166 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.170 | borderless, spanning |
| HPE_2016_page_196 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.188 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.150 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.153 | borderless, spanning |
| HUM_2015_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.134 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.139 | borderless, spanning |
| HWM_2016_page_140 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.146 | plain, borderless |
| INCY_2007_page_90 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.160 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.206 | plain |
| IRM_2010_page_116 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.143 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.165 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.287 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.176 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.167 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.209 | plain, borderless |
| KIM_2010_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.185 | spanning |
| KMB_2010_page_18 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.188 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.160 | plain, borderless |
| KO_2013_page_150 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.242 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.727 | 0.716 | 0.739 | 0.889 | 0.240 | spanning |
| LKQ_2009_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.199 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.212 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.176 | borderless, spanning |
| MAR_2015_page_101 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.122 | plain, borderless |
| MA_2016_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.139 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.110 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.312 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 3 | 0.406 | 0.362 | 0.359 | 0.495 | 0.355 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.192 | plain, borderless |
| MNST_2006_page_107 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.309 | plain |
| MNST_2015_page_100 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.174 | plain, borderless |
| MO_2017_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.417 | plain |
| MPC_2018_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.174 | plain, borderless |
| MRK_2012_page_57 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.189 | plain, borderless |
| MRO_2006_page_84 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.137 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.165 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.179 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.156 | plain, borderless |
| NEM_2008_page_75 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.197 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.182 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.161 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.164 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.183 | plain, borderless |
| PEP_2015_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.124 | borderless, spanning |
| PG_2012_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.302 | plain |
| PM_2015_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.466 | plain |
| PM_2017_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.204 | plain |
| PNC_2012_page_267 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.153 | plain, borderless |
| PNC_2015_page_192 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.161 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.159 | plain, borderless |
| PNW_2015_page_198 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.147 | plain, borderless |
| PRU_2005_page_83 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.247 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.168 | borderless, spanning |
| PXD_2005_page_109 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.179 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.174 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.181 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.177 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.150 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.280 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.144 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.144 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.161 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.137 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.181 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.280 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.155 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.156 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 1 | 0.389 | 0.356 | 0.310 | 0.330 | 0.269 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.170 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 1 | 0.267 | 0.225 | 0.209 | 0.275 | 0.210 | plain, multi-table |
| UNP_2012_page_64 | 3 | 3 | 0.557 | 0.487 | 0.557 | 0.367 | 0.232 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 1 | 0.369 | 0.338 | 0.278 | 0.365 | 0.200 | spanning |
| UNP_2013_page_71 | 1 | 1 | 0.410 | 0.385 | 0.333 | 0.427 | 0.212 | spanning |
| VAR_2012_page_122 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.156 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.214 | multi-header, spanning |
| V_2009_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.188 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.160 | borderless, spanning |
| WRB_2016_page_127 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.397 | plain |
| XEL_2005_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.188 | plain, borderless |
| XEL_2009_page_163 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.224 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.300 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.164 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.208 | plain, borderless |
| ZBH_2003_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.190 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.227 | multi-header, spanning, tall |


### lines / gold-crop

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 1 | 0.113 | 0.112 | 0.113 | 0.132 | 0.020 | spanning, wide |
| ADI_2010_page_51 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.294 | 0.353 | 0.473 | 0.582 | 0.003 | spanning, tall |
| AEE_2007_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.023 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.022 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.017 | plain, borderless |
| AES_2016_page_188 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| AFL_2009_page_40 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| AIG_2010_page_258 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning |
| AIG_2012_page_244 | 1 | 1 | 0.314 | 0.234 | 0.273 | 0.089 | 0.001 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 1 | 0.263 | 0.242 | 0.720 | 0.105 | 0.003 | plain |
| AIZ_2005_page_142 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| AWK_2013_page_131 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| BIIB_2008_page_47 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, spanning |
| BMY_2008_page_82 | 1 | 1 | 0.980 | 0.951 | 0.962 | 0.831 | 0.006 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 1 | 0.333 | 0.296 | 0.281 | 0.167 | 0.002 | plain, multi-table |
| CF_2015_page_32 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| CMI_2014_page_81 | 1 | 1 | 0.205 | 0.154 | 0.184 | 0.222 | 0.004 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.345 | 0.360 | 0.399 | 0.457 | 0.012 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| EFX_2017_page_112 | 2 | 2 | 0.201 | 0.201 | 0.180 | 0.084 | 0.002 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.172 | 0.093 | 0.094 | 0.050 | 0.002 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | plain, borderless |
| FE_2010_page_23 | 1 | 1 | 0.568 | 0.496 | 0.463 | 0.494 | 0.005 | plain |
| FITB_2008_page_21 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| FLS_2012_page_67 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| GD_2004_page_48 | 1 | 1 | 0.410 | 0.324 | 0.266 | 0.000 | 0.001 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| GE_2012_page_148 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.006 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| GM_2010_page_167 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| GPN_2002_page_48 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| HBAN_2009_page_169 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 1 | 0.106 | 0.018 | 0.105 | 0.000 | 0.004 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 1 | 0.267 | 0.267 | 0.312 | 0.133 | 0.003 | plain |
| HOLX_2010_page_70 | 1 | 1 | 0.138 | 0.121 | 0.288 | 0.122 | 0.004 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 1 | 0.167 | 0.111 | 0.257 | 0.118 | 0.009 | spanning |
| HOLX_2015_page_36 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| HPE_2016_page_196 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| HUM_2015_page_113 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| HWM_2016_page_140 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| INCY_2007_page_90 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain |
| IRM_2010_page_116 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | plain, borderless |
| KIM_2010_page_125 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | spanning |
| KMB_2010_page_18 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| KO_2013_page_150 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.727 | 0.716 | 0.739 | 0.889 | 0.002 | spanning |
| LKQ_2009_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| MAR_2015_page_101 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MA_2016_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 1 | 0.307 | 0.301 | 0.320 | 0.365 | 0.013 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 3 | 0.424 | 0.486 | 0.359 | 0.495 | 0.005 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MNST_2006_page_107 | 1 | 1 | 0.316 | 0.300 | 0.412 | 0.211 | 0.005 | plain |
| MNST_2015_page_100 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| MO_2017_page_72 | 1 | 1 | 0.462 | 0.462 | 0.308 | 0.000 | 0.002 | plain |
| MPC_2018_page_111 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| MRK_2012_page_57 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| MRO_2006_page_84 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless |
| NEM_2008_page_75 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PEP_2015_page_89 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, spanning |
| PG_2012_page_30 | 1 | 1 | 0.188 | 0.181 | 0.156 | 0.188 | 0.005 | plain |
| PM_2015_page_106 | 1 | 1 | 0.121 | 0.062 | 0.085 | 0.000 | 0.004 | plain |
| PM_2017_page_77 | 1 | 1 | 0.160 | 0.160 | 0.115 | 0.133 | 0.002 | plain |
| PNC_2012_page_267 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PNC_2015_page_192 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | plain, borderless |
| PNW_2015_page_198 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| PRU_2005_page_83 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| PXD_2005_page_109 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 1 | 0.276 | 0.184 | 0.611 | 0.231 | 0.002 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 1 | 0.211 | 0.099 | 0.303 | 0.171 | 0.004 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 1 | 0.114 | 0.086 | 0.156 | 0.103 | 0.003 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.001 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 1 | 0.389 | 0.413 | 0.310 | 0.330 | 0.006 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 2 | 0.533 | 0.507 | 0.419 | 0.549 | 0.003 | plain, multi-table |
| UNP_2012_page_64 | 3 | 3 | 0.559 | 0.576 | 0.516 | 0.366 | 0.003 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 1 | 0.368 | 0.349 | 0.278 | 0.365 | 0.003 | spanning |
| UNP_2013_page_71 | 1 | 1 | 0.410 | 0.394 | 0.333 | 0.427 | 0.003 | spanning |
| VAR_2012_page_122 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 1 | 0.300 | 0.300 | 0.263 | 0.333 | 0.004 | multi-header, spanning |
| V_2009_page_105 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.003 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, spanning |
| WRB_2016_page_127 | 1 | 1 | 0.240 | 0.240 | 0.138 | 0.160 | 0.005 | plain |
| XEL_2005_page_70 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.004 | plain, borderless |
| XEL_2009_page_163 | 2 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.005 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| ZBH_2003_page_69 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 1 | 0.090 | 0.082 | 0.052 | 0.102 | 0.002 | multi-header, spanning, tall |

### text / gold-crop

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 1 | 0.253 | 0.158 | 0.096 | 0.033 | 0.018 | spanning, wide |
| ADI_2010_page_51 | 1 | 1 | 0.252 | 0.258 | 0.612 | 0.323 | 0.003 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 1 | 0.114 | 0.087 | 0.183 | 0.000 | 0.025 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.156 | 0.167 | 0.627 | 0.287 | 0.018 | spanning, tall |
| AEE_2007_page_125 | 1 | 1 | 0.121 | 0.126 | 0.611 | 0.016 | 0.023 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 2 | 0.187 | 0.069 | 0.157 | 0.000 | 0.013 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 1 | 0.255 | 0.214 | 0.301 | 0.133 | 0.001 | plain, borderless |
| AES_2016_page_188 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | plain, borderless |
| AFL_2009_page_40 | 1 | 1 | 0.069 | 0.025 | 0.093 | 0.000 | 0.004 | plain, borderless |
| AIG_2010_page_258 | 1 | 1 | 0.297 | 0.118 | 0.297 | 0.155 | 0.002 | borderless, spanning |
| AIG_2012_page_244 | 1 | 1 | 0.135 | 0.151 | 0.436 | 0.154 | 0.001 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 1 | 0.441 | 0.333 | 0.622 | 0.167 | 0.004 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 1 | 0.145 | 0.203 | 0.484 | 0.062 | 0.004 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 1 | 0.130 | 0.099 | 0.140 | 0.079 | 0.006 | plain |
| AIZ_2005_page_142 | 1 | 1 | 0.291 | 0.121 | 0.350 | 0.033 | 0.002 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 1 | 0.299 | 0.284 | 0.553 | 0.035 | 0.004 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 3 | 0.130 | 0.068 | 0.218 | 0.038 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 1 | 0.167 | 0.155 | 0.318 | 0.281 | 0.002 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 1 | 0.245 | 0.243 | 0.307 | 0.252 | 0.002 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 1 | 0.157 | 0.062 | 0.143 | 0.000 | 0.004 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 1 | 0.227 | 0.183 | 0.342 | 0.265 | 0.001 | borderless, spanning |
| AWK_2013_page_131 | 1 | 1 | 0.150 | 0.066 | 0.186 | 0.000 | 0.002 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 1 | 0.483 | 0.438 | 0.515 | 0.038 | 0.011 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 1 | 0.268 | 0.176 | 0.130 | 0.000 | 0.003 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 2 | 0.254 | 0.189 | 0.374 | 0.053 | 0.003 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 1 | 0.170 | 0.053 | 0.124 | 0.000 | 0.003 | plain, borderless |
| BIIB_2008_page_47 | 2 | 2 | 0.216 | 0.116 | 0.197 | 0.072 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 1 | 0.248 | 0.119 | 0.138 | 0.070 | 0.005 | borderless, spanning |
| BMY_2008_page_82 | 1 | 1 | 0.077 | 0.049 | 0.147 | 0.000 | 0.004 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 1 | 0.269 | 0.201 | 0.212 | 0.116 | 0.003 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 2 | 0.059 | 0.032 | 0.082 | 0.000 | 0.003 | plain, multi-table |
| CF_2015_page_32 | 2 | 2 | 0.160 | 0.069 | 0.268 | 0.014 | 0.002 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 1 | 0.228 | 0.162 | 0.204 | 0.090 | 0.004 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 1 | 0.275 | 0.174 | 0.361 | 0.255 | 0.002 | plain, borderless |
| CMI_2014_page_81 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.002 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.505 | 0.435 | 0.578 | 0.123 | 0.007 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 1 | 0.187 | 0.104 | 0.232 | 0.000 | 0.006 | plain, borderless |
| EFX_2017_page_112 | 2 | 2 | 0.039 | 0.054 | 0.218 | 0.038 | 0.002 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 2 | 0.136 | 0.114 | 0.226 | 0.086 | 0.003 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 2 | 0.157 | 0.125 | 0.168 | 0.066 | 0.003 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 1 | 0.179 | 0.131 | 0.261 | 0.000 | 0.002 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.247 | 0.271 | 0.718 | 0.565 | 0.002 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 3 | 0.109 | 0.055 | 0.130 | 0.053 | 0.002 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 2 | 0.216 | 0.084 | 0.235 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 1 | 0.236 | 0.050 | 0.228 | 0.000 | 0.006 | plain, borderless |
| FE_2010_page_23 | 1 | 1 | 0.233 | 0.066 | 0.200 | 0.000 | 0.003 | plain |
| FITB_2008_page_21 | 1 | 1 | 0.310 | 0.067 | 0.327 | 0.000 | 0.004 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 1 | 0.213 | 0.039 | 0.242 | 0.000 | 0.004 | plain, borderless |
| FLS_2012_page_67 | 1 | 1 | 0.440 | 0.410 | 0.693 | 0.766 | 0.004 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 1 | 0.126 | 0.125 | 0.429 | 0.000 | 0.003 | borderless, spanning |
| GD_2004_page_48 | 1 | 1 | 0.183 | 0.187 | 0.596 | 0.315 | 0.001 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 1 | 0.093 | 0.123 | 0.132 | 0.000 | 0.003 | plain, borderless |
| GE_2012_page_148 | 1 | 1 | 0.273 | 0.065 | 0.164 | 0.024 | 0.007 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 1 | 0.076 | 0.024 | 0.065 | 0.000 | 0.004 | plain, borderless |
| GM_2010_page_167 | 1 | 1 | 0.269 | 0.105 | 0.221 | 0.000 | 0.005 | plain, borderless |
| GPN_2002_page_48 | 1 | 1 | 0.031 | 0.034 | 0.148 | 0.000 | 0.002 | plain, borderless |
| HBAN_2009_page_169 | 3 | 3 | 0.235 | 0.133 | 0.246 | 0.080 | 0.003 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 1 | 0.075 | 0.073 | 0.126 | 0.097 | 0.002 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 1 | 0.102 | 0.107 | 0.386 | 0.153 | 0.001 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 1 | 0.270 | 0.145 | 0.194 | 0.025 | 0.004 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 1 | 0.127 | 0.075 | 0.174 | 0.023 | 0.004 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 1 | 0.233 | 0.063 | 0.190 | 0.000 | 0.003 | plain |
| HOLX_2010_page_70 | 1 | 1 | 0.240 | 0.111 | 0.161 | 0.000 | 0.004 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 1 | 0.105 | 0.101 | 0.161 | 0.065 | 0.008 | spanning |
| HOLX_2015_page_36 | 2 | 2 | 0.191 | 0.154 | 0.205 | 0.021 | 0.007 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 1 | 0.230 | 0.077 | 0.200 | 0.034 | 0.003 | borderless, spanning |
| HPE_2016_page_196 | 1 | 1 | 0.126 | 0.068 | 0.128 | 0.000 | 0.003 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 2 | 0.145 | 0.127 | 0.163 | 0.068 | 0.001 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 1 | 0.119 | 0.101 | 0.257 | 0.116 | 0.003 | borderless, spanning |
| HUM_2015_page_113 | 1 | 1 | 0.205 | 0.118 | 0.226 | 0.088 | 0.002 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 1 | 0.022 | 0.032 | 0.101 | 0.086 | 0.002 | borderless, spanning |
| HWM_2016_page_140 | 1 | 1 | 0.343 | 0.377 | 0.453 | 0.000 | 0.002 | plain, borderless |
| INCY_2007_page_90 | 2 | 2 | 0.160 | 0.192 | 0.310 | 0.137 | 0.002 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 1 | 0.278 | 0.232 | 0.392 | 0.184 | 0.006 | plain |
| IRM_2010_page_116 | 2 | 2 | 0.122 | 0.073 | 0.106 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 3 | 0.135 | 0.076 | 0.167 | 0.019 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 1 | 0.351 | 0.246 | 0.345 | 0.161 | 0.004 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 2 | 0.109 | 0.064 | 0.177 | 0.026 | 0.003 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 1 | 0.149 | 0.139 | 0.322 | 0.021 | 0.003 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 1 | 0.166 | 0.149 | 0.226 | 0.000 | 0.004 | plain, borderless |
| KIM_2010_page_125 | 1 | 1 | 0.006 | 0.005 | 0.030 | 0.000 | 0.002 | spanning |
| KMB_2010_page_18 | 1 | 1 | 0.281 | 0.296 | 0.515 | 0.500 | 0.004 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 1 | 0.052 | 0.039 | 0.075 | 0.005 | 0.003 | plain, borderless |
| KO_2013_page_150 | 1 | 1 | 0.207 | 0.158 | 0.340 | 0.069 | 0.003 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.055 | 0.029 | 0.103 | 0.000 | 0.003 | spanning |
| LKQ_2009_page_69 | 1 | 1 | 0.214 | 0.130 | 0.165 | 0.000 | 0.002 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 1 | 0.208 | 0.067 | 0.207 | 0.011 | 0.004 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 1 | 0.187 | 0.142 | 0.221 | 0.135 | 0.003 | borderless, spanning |
| MAR_2015_page_101 | 1 | 1 | 0.025 | 0.032 | 0.119 | 0.015 | 0.002 | plain, borderless |
| MA_2016_page_69 | 1 | 1 | 0.218 | 0.135 | 0.330 | 0.054 | 0.002 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 1 | 0.080 | 0.066 | 0.384 | 0.000 | 0.001 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 1 | 0.369 | 0.352 | 0.657 | 0.244 | 0.010 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 3 | 0.219 | 0.179 | 0.261 | 0.105 | 0.005 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 1 | 0.071 | 0.042 | 0.057 | 0.000 | 0.003 | plain, borderless |
| MNST_2006_page_107 | 1 | 1 | 0.008 | 0.023 | 0.086 | 0.058 | 0.006 | plain |
| MNST_2015_page_100 | 1 | 1 | 0.055 | 0.026 | 0.159 | 0.000 | 0.002 | plain, borderless |
| MO_2017_page_72 | 1 | 1 | 0.022 | 0.033 | 0.068 | 0.013 | 0.003 | plain |
| MPC_2018_page_111 | 1 | 1 | 0.109 | 0.037 | 0.093 | 0.000 | 0.002 | plain, borderless |
| MRK_2012_page_57 | 1 | 1 | 0.167 | 0.066 | 0.301 | 0.000 | 0.003 | plain, borderless |
| MRO_2006_page_84 | 1 | 1 | 0.291 | 0.285 | 0.806 | 0.423 | 0.002 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 1 | 0.151 | 0.128 | 0.204 | 0.020 | 0.002 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 1 | 0.658 | 0.613 | 0.791 | 0.668 | 0.004 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 1 | 0.106 | 0.093 | 0.153 | 0.000 | 0.002 | plain, borderless |
| NEM_2008_page_75 | 1 | 1 | 0.095 | 0.066 | 0.150 | 0.000 | 0.004 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 1 | 0.090 | 0.044 | 0.200 | 0.082 | 0.004 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 1 | 0.470 | 0.377 | 0.586 | 0.056 | 0.004 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 1 | 0.190 | 0.118 | 0.147 | 0.000 | 0.004 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 1 | 0.160 | 0.089 | 0.143 | 0.000 | 0.003 | plain, borderless |
| PEP_2015_page_89 | 1 | 1 | 0.084 | 0.087 | 0.348 | 0.257 | 0.001 | borderless, spanning |
| PG_2012_page_30 | 1 | 1 | 0.207 | 0.130 | 0.258 | 0.078 | 0.005 | plain |
| PM_2015_page_106 | 1 | 1 | 0.132 | 0.141 | 0.271 | 0.342 | 0.004 | plain |
| PM_2017_page_77 | 1 | 1 | 0.143 | 0.122 | 0.337 | 0.093 | 0.002 | plain |
| PNC_2012_page_267 | 1 | 1 | 0.055 | 0.049 | 0.190 | 0.000 | 0.004 | plain, borderless |
| PNC_2015_page_192 | 3 | 3 | 0.242 | 0.089 | 0.202 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 1 | 0.050 | 0.029 | 0.102 | 0.000 | 0.004 | plain, borderless |
| PNW_2015_page_198 | 1 | 1 | 0.136 | 0.116 | 0.316 | 0.000 | 0.003 | plain, borderless |
| PRU_2005_page_83 | 1 | 1 | 0.498 | 0.430 | 0.714 | 0.752 | 0.007 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 1 | 0.151 | 0.040 | 0.231 | 0.022 | 0.004 | borderless, spanning |
| PXD_2005_page_109 | 1 | 1 | 0.107 | 0.068 | 0.109 | 0.000 | 0.005 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 1 | 0.272 | 0.095 | -0.008 | 0.000 | 0.003 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 2 | 0.295 | 0.181 | 0.276 | 0.029 | 0.003 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 1 | 0.237 | 0.132 | 0.190 | 0.000 | 0.005 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 1 | 0.096 | 0.069 | 0.133 | 0.000 | 0.002 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 1 | 0.202 | 0.148 | 0.163 | 0.093 | 0.004 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 1 | 0.228 | 0.206 | 0.567 | 0.000 | 0.002 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 2 | 0.154 | 0.129 | 0.176 | 0.000 | 0.002 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 3 | 0.131 | 0.048 | 0.221 | 0.000 | 0.002 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 1 | 0.363 | 0.221 | 0.415 | 0.000 | 0.002 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 1 | 0.127 | 0.099 | 0.238 | 0.105 | 0.004 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 1 | 0.158 | 0.139 | 0.359 | 0.079 | 0.006 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 1 | 0.252 | 0.139 | 0.221 | 0.028 | 0.002 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 2 | 0.102 | 0.094 | 0.151 | 0.055 | 0.003 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 1 | 0.228 | 0.111 | 0.296 | 0.025 | 0.004 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 2 | 0.179 | 0.045 | 0.142 | 0.000 | 0.002 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 2 | 0.237 | 0.108 | 0.175 | 0.057 | 0.003 | plain, multi-table |
| UNP_2012_page_64 | 3 | 3 | 0.100 | 0.046 | 0.133 | 0.018 | 0.004 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 1 | 0.374 | 0.285 | 0.387 | 0.194 | 0.004 | spanning |
| UNP_2013_page_71 | 1 | 1 | 0.328 | 0.200 | 0.344 | 0.000 | 0.002 | spanning |
| VAR_2012_page_122 | 1 | 1 | 0.281 | 0.174 | 0.298 | 0.036 | 0.003 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 1 | 0.220 | 0.062 | 0.129 | 0.000 | 0.004 | multi-header, spanning |
| V_2009_page_105 | 1 | 1 | 0.372 | 0.272 | 0.605 | 0.250 | 0.004 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 1 | 0.234 | 0.055 | 0.261 | 0.022 | 0.003 | borderless, spanning |
| WRB_2016_page_127 | 1 | 1 | 0.075 | 0.050 | 0.141 | 0.068 | 0.005 | plain |
| XEL_2005_page_70 | 1 | 1 | 0.106 | 0.052 | 0.109 | 0.000 | 0.003 | plain, borderless |
| XEL_2009_page_163 | 2 | 2 | 0.167 | 0.153 | 0.311 | 0.042 | 0.005 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 1 | 0.414 | 0.388 | 0.807 | 0.737 | 0.011 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 1 | 0.094 | 0.040 | 0.084 | 0.000 | 0.005 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 1 | 0.147 | 0.042 | 0.173 | 0.000 | 0.003 | plain, borderless |
| ZBH_2003_page_69 | 1 | 1 | 0.190 | 0.120 | 0.352 | 0.045 | 0.003 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 1 | 0.259 | 0.243 | 0.472 | 0.316 | 0.005 | multi-header, spanning, tall |

### onnx / gold-crop

| doc | gold | matched | GriTS_Top | GriTS_Con | TEDS-S | cell-F1 | s | tags |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ADBE_2011_page_118 | 1 | 1 | 0.662 | 0.586 | 0.578 | 0.095 | 0.201 | spanning, wide |
| ADI_2010_page_51 | 1 | 1 | 0.730 | 0.560 | 0.784 | 0.313 | 0.259 | borderless, spanning, tall |
| ADI_2014_page_38 | 1 | 1 | 0.813 | 0.827 | 0.800 | 0.161 | 0.394 | borderless, multi-header, spanning, wide |
| ADP_2008_page_35 | 1 | 1 | 0.882 | 0.803 | 0.853 | 0.394 | 0.381 | spanning, tall |
| AEE_2007_page_125 | 1 | 1 | 0.913 | 0.875 | 0.906 | 0.819 | 0.413 | borderless, multi-header, spanning, tall |
| AEE_2017_page_148 | 2 | 2 | 0.836 | 0.671 | 0.803 | 0.342 | 0.217 | plain, borderless, multi-table |
| AEE_2017_page_49 | 1 | 1 | 1.000 | 0.883 | 1.000 | 1.000 | 0.069 | plain, borderless |
| AES_2016_page_188 | 1 | 1 | 0.708 | 0.702 | 0.657 | 0.217 | 0.090 | plain, borderless |
| AFL_2009_page_40 | 1 | 1 | 1.000 | 0.875 | 1.000 | 0.167 | 0.093 | plain, borderless |
| AIG_2010_page_258 | 1 | 1 | 0.946 | 0.944 | 0.944 | 0.661 | 0.140 | borderless, spanning |
| AIG_2012_page_244 | 1 | 1 | 0.881 | 0.669 | 0.864 | 0.028 | 0.154 | multi-header, spanning |
| AIG_2012_page_271 | 1 | 1 | 0.701 | 0.539 | 0.621 | 0.023 | 0.502 | borderless, multi-header, spanning, tall |
| AIG_2018_page_280 | 1 | 1 | 0.849 | 0.667 | 0.831 | 0.056 | 0.201 | borderless, multi-header, spanning, wide |
| AIZ_2004_page_155 | 1 | 1 | 1.000 | 0.974 | 1.000 | 0.400 | 0.204 | plain |
| AIZ_2005_page_142 | 1 | 1 | 0.889 | 0.619 | 0.865 | 0.255 | 0.216 | borderless, multi-header, spanning |
| AMAT_2015_page_117 | 1 | 1 | 1.000 | 0.988 | 1.000 | 0.349 | 0.213 | borderless, multi-header, spanning |
| AMP_2015_page_94 | 3 | 3 | 0.725 | 0.646 | 0.719 | 0.163 | 0.108 | borderless, multi-header, spanning, wide, multi-table |
| AMZN_2004_page_76 | 1 | 1 | 0.867 | 0.639 | 0.839 | 0.190 | 0.117 | borderless, multi-header, spanning |
| AON_2009_page_104 | 1 | 1 | 0.984 | 0.984 | 0.971 | 0.252 | 0.120 | borderless, multi-header, spanning |
| APH_2016_page_35 | 1 | 1 | 0.960 | 0.871 | 0.931 | 0.667 | 0.110 | borderless, multi-header, spanning |
| ATO_2019_page_30 | 1 | 1 | 0.923 | 0.539 | 0.926 | 0.000 | 0.060 | borderless, spanning |
| AWK_2013_page_131 | 1 | 1 | 1.000 | 0.992 | 1.000 | 0.759 | 0.071 | borderless, multi-header, spanning |
| BAC_2011_page_153 | 1 | 1 | 0.856 | 0.725 | 0.804 | 0.043 | 0.358 | borderless, spanning, tall |
| BAC_2016_page_172 | 1 | 1 | 0.875 | 0.862 | 0.874 | 0.190 | 0.302 | borderless, spanning, wide, tall |
| BBY_2008_page_27 | 2 | 2 | 0.995 | 1.000 | 0.992 | 0.347 | 0.233 | plain, borderless, multi-header, spanning, tall, multi-table |
| BDX_2009_page_79 | 1 | 1 | 1.000 | 0.677 | 1.000 | 0.429 | 0.063 | plain, borderless |
| BIIB_2008_page_47 | 2 | 2 | 0.971 | 0.896 | 0.961 | 0.493 | 0.079 | plain, borderless, multi-header, spanning, multi-table |
| BLK_2011_page_33 | 1 | 1 | 0.810 | 0.749 | 0.768 | 0.103 | 0.122 | borderless, spanning |
| BMY_2008_page_82 | 1 | 1 | 1.000 | 0.971 | 1.000 | 0.636 | 0.119 | multi-header, spanning |
| BMY_2018_page_55 | 1 | 1 | 0.680 | 0.605 | 0.596 | 0.137 | 0.108 | borderless, multi-header, spanning |
| BSX_2007_page_128 | 2 | 2 | 1.000 | 0.983 | 1.000 | 0.667 | 0.125 | plain, multi-table |
| CF_2015_page_32 | 2 | 2 | 0.859 | 0.780 | 0.836 | 0.098 | 0.169 | borderless, multi-header, spanning, wide, multi-table |
| CHTR_2006_page_20 | 1 | 1 | 0.750 | 0.755 | 0.708 | 0.459 | 0.242 | borderless, multi-header, spanning |
| CMI_2012_page_105 | 1 | 1 | 0.061 | 0.006 | 0.061 | 0.000 | 0.120 | plain, borderless |
| CMI_2014_page_81 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.724 | 0.099 | multi-header, spanning |
| DISCA_2011_page_51 | 1 | 1 | 0.801 | 0.502 | 0.788 | 0.164 | 0.449 | multi-header, spanning, tall |
| ED_2013_page_123 | 1 | 1 | 0.941 | 0.906 | 0.891 | 0.424 | 0.182 | plain, borderless |
| EFX_2017_page_112 | 2 | 2 | 0.879 | 0.826 | 0.814 | 0.081 | 0.211 | multi-header, spanning, multi-table |
| EL_2010_page_140 | 2 | 2 | 0.820 | 0.779 | 0.701 | 0.209 | 0.140 | multi-header, spanning, multi-table |
| EMR_2017_page_68 | 2 | 2 | 0.727 | 0.724 | 0.633 | 0.382 | 0.127 | plain, borderless, multi-table |
| ETR_2009_page_141 | 1 | 1 | 0.909 | 0.892 | 0.838 | 0.636 | 0.118 | plain, borderless |
| ETR_2011_page_370 | 1 | 1 | 0.387 | 0.358 | 0.341 | 0.316 | 0.347 | multi-header, spanning, tall |
| ETR_2013_page_29 | 3 | 3 | 0.880 | 0.878 | 0.866 | 0.461 | 0.095 | borderless, multi-header, spanning, multi-table |
| EXR_2018_page_28 | 2 | 2 | 0.713 | 0.647 | 0.721 | 0.123 | 0.154 | borderless, multi-header, spanning, multi-table |
| FCX_2012_page_97 | 1 | 1 | 1.000 | 0.934 | 1.000 | 0.250 | 0.124 | plain, borderless |
| FE_2010_page_23 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.444 | 0.134 | plain |
| FITB_2008_page_21 | 1 | 1 | 0.911 | 0.906 | 0.912 | 0.865 | 0.204 | borderless, spanning, tall |
| FLIR_2010_page_73 | 1 | 1 | 1.000 | 0.815 | 1.000 | 0.952 | 0.140 | plain, borderless |
| FLS_2012_page_67 | 1 | 1 | 0.505 | 0.476 | 0.470 | 0.205 | 0.236 | borderless, multi-header, spanning, tall |
| FRT_2010_page_42 | 1 | 1 | 0.440 | 0.335 | 0.297 | 0.032 | 0.255 | borderless, spanning |
| GD_2004_page_48 | 1 | 1 | 0.372 | 0.214 | 0.309 | 0.000 | 0.070 | multi-header, spanning, tall |
| GD_2005_page_62 | 1 | 1 | 0.667 | 0.492 | 0.682 | 0.000 | 0.060 | plain, borderless |
| GE_2012_page_148 | 1 | 1 | 0.840 | 0.822 | 0.794 | 0.023 | 0.130 | borderless, multi-header, spanning |
| GE_2018_page_19 | 1 | 1 | 0.889 | 0.716 | 0.842 | 0.148 | 0.083 | plain, borderless |
| GM_2010_page_167 | 1 | 1 | 0.912 | 0.590 | 0.889 | 0.424 | 0.103 | plain, borderless |
| GPN_2002_page_48 | 1 | 1 | 1.000 | 0.551 | 1.000 | 0.333 | 0.105 | plain, borderless |
| HBAN_2009_page_169 | 3 | 3 | 0.775 | 0.486 | 0.780 | 0.194 | 0.176 | borderless, spanning, multi-table |
| HBAN_2015_page_71 | 1 | 1 | 0.958 | 0.898 | 0.931 | 0.512 | 0.149 | borderless, multi-header, spanning |
| HBAN_2016_page_111 | 1 | 1 | 0.764 | 0.754 | 0.710 | 0.211 | 0.113 | borderless, multi-header, spanning |
| HFC_2012_page_54 | 1 | 1 | 0.902 | 0.897 | 0.896 | 0.533 | 0.147 | multi-header, spanning |
| HLT_2014_page_106 | 1 | 1 | 0.971 | 1.000 | 0.947 | 0.508 | 0.149 | borderless, multi-header, spanning |
| HOLX_2010_page_106 | 1 | 1 | 1.000 | 0.994 | 1.000 | 0.100 | 0.093 | plain |
| HOLX_2010_page_70 | 1 | 1 | 1.000 | 0.983 | 1.000 | 0.131 | 0.147 | multi-header, spanning |
| HOLX_2012_page_144 | 1 | 1 | 0.926 | 0.917 | 0.919 | 0.769 | 0.144 | spanning |
| HOLX_2015_page_36 | 2 | 2 | 1.000 | 1.000 | 1.000 | 0.655 | 0.312 | plain, borderless, multi-table |
| HON_2004_page_79 | 1 | 1 | 0.868 | 0.500 | 0.870 | 0.371 | 0.130 | borderless, spanning |
| HPE_2016_page_196 | 1 | 1 | 0.854 | 0.567 | 0.808 | 0.324 | 0.130 | borderless, multi-header, spanning |
| HPQ_2006_page_111 | 2 | 2 | 0.807 | 0.703 | 0.784 | 0.359 | 0.175 | borderless, multi-header, spanning, wide, multi-table |
| HSY_2007_page_29 | 1 | 1 | 0.905 | 0.735 | 0.897 | 0.350 | 0.195 | borderless, spanning |
| HUM_2015_page_113 | 1 | 1 | 0.719 | 0.692 | 0.659 | 0.211 | 0.211 | borderless, multi-header, spanning |
| HUM_2018_page_115 | 1 | 1 | 0.929 | 0.834 | 0.909 | 0.444 | 0.191 | borderless, spanning |
| HWM_2016_page_140 | 1 | 1 | 1.000 | 0.882 | 1.000 | 0.188 | 0.218 | plain, borderless |
| INCY_2007_page_90 | 2 | 2 | 0.798 | 0.690 | 0.750 | 0.090 | 0.244 | plain, borderless, multi-table |
| IPGP_2018_page_89 | 1 | 1 | 0.961 | 0.799 | 0.937 | 0.052 | 0.255 | plain |
| IRM_2010_page_116 | 2 | 2 | 0.836 | 0.745 | 0.793 | 0.625 | 0.216 | borderless, multi-header, spanning, multi-table |
| IRM_2016_page_107 | 3 | 3 | 1.000 | 0.906 | 1.000 | 0.737 | 0.104 | plain, borderless, multi-header, spanning, multi-table |
| IVZ_2017_page_133 | 1 | 1 | 0.854 | 0.826 | 0.817 | 0.480 | 0.250 | multi-header, spanning, tall |
| JKHY_2017_page_25 | 2 | 2 | 0.839 | 0.835 | 0.775 | 0.400 | 0.258 | plain, borderless, multi-header, spanning, multi-table |
| JKHY_2019_page_50 | 1 | 1 | 0.667 | 0.626 | 0.591 | 0.070 | 0.182 | borderless, multi-header, spanning |
| JPM_2006_page_108 | 1 | 1 | 1.000 | 0.995 | 1.000 | 0.575 | 0.124 | plain, borderless |
| KIM_2010_page_125 | 1 | 1 | 0.600 | 0.507 | 0.500 | 0.222 | 0.146 | spanning |
| KMB_2010_page_18 | 1 | 1 | 0.877 | 0.786 | 0.879 | 0.660 | 0.273 | borderless, multi-header, spanning |
| KO_2006_page_68 | 1 | 1 | 0.667 | 0.300 | 0.677 | 0.533 | 0.203 | plain, borderless |
| KO_2013_page_150 | 1 | 1 | 0.973 | 0.957 | 0.948 | 0.622 | 0.175 | plain, borderless |
| LEG_2011_page_70 | 1 | 1 | 0.727 | 0.716 | 0.739 | 0.741 | 0.121 | spanning |
| LKQ_2009_page_69 | 1 | 1 | 0.917 | 0.689 | 0.893 | 0.500 | 0.106 | borderless, multi-header, spanning |
| LMT_2005_page_39 | 1 | 1 | 0.715 | 0.604 | 0.607 | 0.155 | 0.119 | borderless, multi-header, spanning |
| L_2007_page_168 | 1 | 1 | 0.660 | 0.666 | 0.594 | 0.174 | 0.100 | borderless, spanning |
| MAR_2015_page_101 | 1 | 1 | 1.000 | 0.784 | 1.000 | 0.417 | 0.050 | plain, borderless |
| MA_2016_page_69 | 1 | 1 | 0.713 | 0.540 | 0.638 | 0.101 | 0.149 | borderless, multi-header, spanning |
| MCK_2006_page_26 | 1 | 1 | 0.952 | 0.824 | 0.939 | 0.537 | 0.142 | borderless, spanning |
| MKTX_2011_page_65 | 1 | 1 | 0.856 | 0.834 | 0.842 | 0.569 | 0.528 | multi-header, spanning, wide, tall |
| MKTX_2018_page_134 | 3 | 3 | 0.934 | 0.916 | 0.941 | 0.442 | 0.166 | multi-header, spanning, wide, multi-table |
| MMM_2007_page_24 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.125 | 0.144 | plain, borderless |
| MNST_2006_page_107 | 1 | 1 | 0.667 | 0.527 | 0.611 | 0.308 | 0.159 | plain |
| MNST_2015_page_100 | 1 | 1 | 0.923 | 0.855 | 0.864 | 0.385 | 0.110 | plain, borderless |
| MO_2017_page_72 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.667 | 0.087 | plain |
| MPC_2018_page_111 | 1 | 1 | 0.636 | 0.686 | 0.591 | 0.133 | 0.213 | plain, borderless |
| MRK_2012_page_57 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.917 | 0.187 | plain, borderless |
| MRO_2006_page_84 | 1 | 1 | 0.889 | 0.803 | 0.871 | 0.100 | 0.240 | borderless, spanning, tall |
| MSCI_2012_page_76 | 1 | 1 | 0.883 | 0.852 | 0.841 | 0.165 | 0.155 | borderless, multi-header, spanning, wide |
| MS_2013_page_56 | 1 | 1 | 0.881 | 0.626 | 0.878 | 0.284 | 0.437 | borderless, spanning, tall |
| NEM_2008_page_65 | 1 | 1 | 1.000 | 0.838 | 1.000 | 0.333 | 0.076 | plain, borderless |
| NEM_2008_page_75 | 1 | 1 | 0.800 | 0.840 | 0.692 | 0.533 | 0.171 | borderless, multi-header, spanning, wide |
| NRG_2013_page_77 | 1 | 1 | 0.733 | 0.591 | 0.710 | 0.146 | 0.137 | borderless, multi-header, spanning |
| NTRS_2017_page_94 | 1 | 1 | 0.727 | 0.709 | 0.650 | 0.025 | 0.174 | borderless, multi-header, spanning, tall |
| NWS_2016_page_120 | 1 | 1 | 0.833 | 0.709 | 0.793 | 0.250 | 0.068 | borderless, multi-header, spanning |
| PEAK_2006_page_110 | 1 | 1 | 1.000 | 0.490 | 1.000 | 0.000 | 0.145 | plain, borderless |
| PEP_2015_page_89 | 1 | 1 | 0.962 | 0.622 | 0.950 | 0.157 | 0.065 | borderless, spanning |
| PG_2012_page_30 | 1 | 1 | 1.000 | 0.994 | 1.000 | 0.796 | 0.097 | plain |
| PM_2015_page_106 | 1 | 1 | 0.526 | 0.475 | 0.323 | 0.484 | 0.258 | plain |
| PM_2017_page_77 | 1 | 1 | 0.873 | 0.790 | 0.782 | 0.667 | 0.090 | plain |
| PNC_2012_page_267 | 1 | 1 | 1.000 | 0.998 | 1.000 | 0.625 | 0.089 | plain, borderless |
| PNC_2015_page_192 | 3 | 3 | 0.929 | 0.914 | 0.909 | 0.225 | 0.070 | borderless, multi-header, spanning, multi-table |
| PNW_2013_page_165 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.167 | 0.061 | plain, borderless |
| PNW_2015_page_198 | 1 | 1 | 1.000 | 0.985 | 1.000 | 0.560 | 0.097 | plain, borderless |
| PRU_2005_page_83 | 1 | 1 | 0.915 | 0.590 | 0.905 | 0.452 | 0.808 | borderless, spanning, tall |
| PWR_2015_page_130 | 1 | 1 | 0.944 | 0.627 | 0.929 | 0.857 | 0.097 | borderless, spanning |
| PXD_2005_page_109 | 1 | 1 | 0.929 | 0.926 | 0.870 | 0.412 | 0.091 | borderless, multi-header, spanning |
| RE_2007_page_61 | 1 | 1 | 0.991 | 0.989 | 0.984 | 0.865 | 0.256 | borderless, multi-header, spanning, wide |
| RE_2010_page_125 | 2 | 2 | 0.771 | 0.723 | 0.735 | 0.237 | 0.202 | borderless, multi-header, spanning, multi-table |
| RL_2008_page_49 | 1 | 1 | 0.911 | 0.619 | 0.914 | 0.275 | 0.097 | borderless, multi-header, spanning |
| RL_2011_page_113 | 1 | 1 | 0.889 | 0.422 | 0.783 | 0.067 | 0.106 | multi-header, spanning |
| SBAC_2006_page_86 | 1 | 1 | 0.904 | 0.804 | 0.909 | 0.531 | 0.321 | multi-header, spanning |
| SLB_2015_page_72 | 1 | 1 | 0.959 | 0.824 | 0.935 | 0.138 | 0.259 | borderless, multi-header, spanning |
| SNA_2007_page_95 | 2 | 2 | 0.778 | 0.665 | 0.781 | 0.088 | 0.200 | plain, borderless, spanning, multi-table |
| SNPS_2011_page_77 | 3 | 3 | 0.912 | 0.840 | 0.884 | 0.759 | 0.124 | borderless, multi-header, spanning, multi-table |
| SNPS_2013_page_76 | 1 | 1 | 0.950 | 0.783 | 0.938 | 0.551 | 0.181 | borderless, multi-header, spanning |
| SPGI_2017_page_64 | 1 | 1 | 0.904 | 0.901 | 0.826 | 0.808 | 0.126 | borderless, multi-header, spanning |
| STX_2006_page_88 | 1 | 1 | 0.882 | 0.832 | 0.836 | 0.558 | 0.241 | multi-header, spanning |
| TDG_2009_page_93 | 1 | 1 | 0.875 | 0.786 | 0.846 | 0.321 | 0.230 | borderless, multi-header, spanning |
| TTWO_2009_page_91 | 2 | 2 | 0.983 | 0.895 | 0.971 | 0.655 | 0.107 | borderless, multi-header, spanning, multi-table |
| UHS_2015_page_133 | 1 | 1 | 0.885 | 0.878 | 0.873 | 0.478 | 0.173 | multi-header, spanning |
| UNH_2017_page_72 | 2 | 2 | 0.955 | 0.650 | 0.921 | 0.453 | 0.157 | plain, borderless, multi-header, spanning, multi-table |
| UNP_2012_page_42 | 2 | 2 | 0.800 | 0.679 | 0.720 | 0.143 | 0.160 | plain, multi-table |
| UNP_2012_page_64 | 3 | 3 | 0.908 | 0.872 | 0.980 | 0.582 | 0.144 | plain, multi-header, spanning, multi-table |
| UNP_2012_page_66 | 1 | 1 | 0.943 | 0.886 | 0.941 | 0.265 | 0.178 | spanning |
| UNP_2013_page_71 | 1 | 1 | 0.784 | 0.723 | 0.723 | 0.346 | 0.200 | spanning |
| VAR_2012_page_122 | 1 | 1 | 0.700 | 0.601 | 0.618 | 0.158 | 0.145 | borderless, multi-header, spanning |
| VLO_2016_page_61 | 1 | 1 | 1.000 | 1.000 | 1.000 | 0.538 | 0.120 | multi-header, spanning |
| V_2009_page_105 | 1 | 1 | 0.892 | 0.559 | 0.851 | 0.357 | 0.465 | borderless, multi-header, spanning, tall |
| WM_2015_page_90 | 1 | 1 | 0.933 | 0.570 | 0.935 | 0.372 | 0.226 | borderless, spanning |
| WRB_2016_page_127 | 1 | 1 | 1.000 | 0.983 | 1.000 | 0.381 | 0.268 | plain |
| XEL_2005_page_70 | 1 | 1 | 0.643 | 0.537 | 0.560 | 0.000 | 0.146 | plain, borderless |
| XEL_2009_page_163 | 2 | 2 | 0.988 | 0.954 | 0.977 | 0.651 | 0.255 | plain, borderless, spanning, tall, multi-table |
| XEL_2013_page_98 | 1 | 1 | 0.887 | 0.663 | 0.854 | 0.231 | 0.719 | borderless, multi-header, spanning, tall |
| XLNX_2006_page_34 | 1 | 1 | 0.722 | 0.740 | 0.682 | 0.452 | 0.165 | borderless, multi-header, spanning |
| ZBH_2003_page_42 | 1 | 1 | 0.909 | 0.749 | 0.860 | 0.312 | 0.099 | plain, borderless |
| ZBH_2003_page_69 | 1 | 1 | 0.833 | 0.790 | 0.752 | 0.226 | 0.166 | borderless, multi-header, spanning, wide |
| ZION_2017_page_111 | 1 | 1 | 0.760 | 0.767 | 0.706 | 0.578 | 0.098 | multi-header, spanning, tall |

## Interpretation

### The ONNX backend is the only one that finds financial tables at all

`onnx/e2e` matches **166 of 186** gold tables (detection F1 **0.787**) against 17 for `lines`, 25 for `text` and 18 for the fitz oracle. On this corpus the heuristic strategies are not competing with a weaker model, they are essentially not detecting: FinTabNet is borderless financial tables, and ruling-line evidence is absent. The `borderless` tag row makes it explicit — `lines/e2e` scores **0.000** across all 139 borderless tables.

### pdfspine `lines` remains at parity with fitz

`lines/e2e` 0.045/0.045 vs `fitz-oracle/e2e` 0.046/0.040 (GriTS Top/Con), detection F1 0.089 vs 0.098. The recall-weighted numbers are near-identical, which reproduces the parity result recorded for P3-5 and confirms this harness measures the same thing the older `tables_diff.py --gold` did. The `lines`/`text` gold-crop rows likewise reproduce the historical figures (0.076/0.072 and 0.185/0.125 against the recorded 0.073/0.070 and 0.185/0.107) — the same whole-page-then-best-IoU pairing.

### Structure quality is genuinely good; cell geometry is not

`onnx/gold-crop` — the ADR 0002 apples-to-apples mode — reaches **GriTS_Top 0.863 / GriTS_Con 0.766 / TEDS-S 0.836**, and because every gold bbox is scored, recall-weighted and matched-only are identical. That is a real structure-recognition result, though still well short of the ~0.98 Table-Transformer publishes on this dataset.

**But cell-F1 is 0.371.** The grid SLANet-plus predicts is mostly right while the cell boxes it draws are mostly not, for two compounding reasons: SLANet emits *grid-region* boxes whereas FinTabNet's `pdf_bbox` is tight around the text, which depresses IoU even on a perfect grid; and the `$`-column split noted in the 2026-09-08 by-eye baseline is still present, turning an 8-column table into 12-13 columns and shifting every box after it. Anything that crops a cell to read it — the actual downstream use — feels this number, not GriTS. Closing the GriTS/cell-F1 gap is the highest-value next move, and it is now measurable.

### Detection over-fires

`onnx/e2e` predicts 236 tables for 186 gold (precision 0.703, recall 0.892). Recall is the strong half; the surplus is spurious regions. Detection precision, not structure, is where the remaining e2e loss sits: gold-crop lifts GriTS_Con from 0.692 to 0.766.

### Cost

`onnx/e2e` costs **2.20 s/table** (both models, 144 dpi); `onnx/gold-crop` **0.18 s/table**, because `skip_layout` never loads the 130 MB layout model. The heuristic strategies are ~0.003 s/table and the fitz oracle 0.159 s/table.

### Machine-generated summary

- Best recall-weighted GriTS_Con: **onnx/e2e** at 0.692 (GriTS_Top 0.782, TEDS-S 0.759, cell-F1 0.319).
- `lines/e2e`: 169/186 gold tables missed; detection P/R/F1 0.088/0.091/0.089.
- `text/e2e`: 161/186 gold tables missed; detection P/R/F1 0.169/0.134/0.150.
- `onnx/e2e`: 20/186 gold tables missed; detection P/R/F1 0.703/0.892/0.787.
- `fitz-oracle/e2e`: 168/186 gold tables missed; detection P/R/F1 0.099/0.097/0.098.
- GriTS and TEDS-S are position-invariant; cell-F1 is the geometric check. A high GriTS with a low cell-F1 means the grid is right but cells are shifted.
- Best recall-weighted GriTS_Con: **onnx/gold-crop** at 0.766 (GriTS_Top 0.863, TEDS-S 0.836, cell-F1 0.371).
- `lines/gold-crop`: 145/186 gold tables missed; `clip` is not honoured by this backend, so gold-crop is best-overlap on the full page.
- `text/gold-crop`: 2/186 gold tables missed; `clip` is not honoured by this backend, so gold-crop is best-overlap on the full page.
- `onnx/gold-crop`: 0/186 gold tables missed.

## Reproduce

```sh
export PDFSPINE_ONNX_MODELS="$HOME/models/pdfspine-onnx"
P=.venv/bin/python

# end-to-end (whole pipeline, detection included)
$P conformance/gt/eval_tables.py run --mode e2e --jobs 5 \
   --backend lines --backend text --backend onnx --oracle-fitz \
   --report conformance/gt/GT-REPORT-tables-eval.md \
   --out conformance/gt/ci-tables-eval-results.json

# gold-crop (structure stage only -- the ADR 0002 decision-gate mode)
$P conformance/gt/eval_tables.py run --mode gold-crop --jobs 5 \
   --backend lines --backend text --backend onnx

# track a change against a stored run
$P conformance/gt/eval_tables.py run --backend onnx --baseline conformance/gt/ci-tables-eval-results.json
```

`conformance/gt/corpus-finance/README.md` documents how to add hand-annotated pages of your own; point `--corpus` at that directory to score them.
