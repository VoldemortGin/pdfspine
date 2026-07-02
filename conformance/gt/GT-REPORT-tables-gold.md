# Table cell-structure GOLD GT — pdfspine `find_tables` vs FinTabNet.c (GriTS)

Harness: `/Users/linhan/workspace/spine/pdfspine/conformance/gt/tables_diff.py` (`--gold --strategy lines`; find_tables `strategy="lines"`)  
Metric: **GriTS** (Grid Table Similarity, Smock et al. arXiv:2303.00716 / 2203.12555) — `conformance/gt/grits.py`.  
Dataset: **FinTabNet.c** — annotations `CDLA-Permissive-2.0`, source PDFs `CDLA-Permissive-1.0`.  
Provenance: annotations from `https://huggingface.co/datasets/bsmock/FinTabNet.c/resolve/main/FinTabNet.c-PDF_Annotations.tar.gz`; source PDFs from `https://huggingface.co/datasets/Leon1207/FinTabNet/resolve/main/archive.zip`.

## Why GriTS

GriTS scores cell **topology** (row/col spans) and cell **content** in one F-score framework with per-cell partial credit, is transpose- and position-invariant (the two properties an ideal TSR metric should have), and is the canonical metric for FinTabNet.c — so the number is directly comparable to published Table-Transformer results. We compute **GriTS_Top** (topology) and **GriTS_Con** (content) via the factored 2D-MSS heuristic, a faithful stdlib port of Microsoft's reference `grits.py` (numpy→lists, `fitz.Rect` IoU→plain arithmetic; no AGPL `fitz` is imported).

## Sample / provenance / license

- Sample requested: **150** pages; fetched **150** gold annotation pages (**186** structure-eligible gold tables).
- Source PDFs fetched: **150** / 150 (pdf status: `{'cached': 150}`).
- Annotations license: **CDLA-Permissive-2.0** (permissive; commercial reuse OK).
- Source-PDF license: **CDLA-Permissive-1.0** (permissive).
- Only permissively-licensed data is used. The data itself is gitignored (`conformance/gt/corpus-*/`); the committed deliverables are the fetcher, the metric, the harness mode, and this report.

## Absolute cell-structure score (pdfspine `strategy="lines"` vs gold)

- Gold tables scored: **186** (across 150 pages); pdfspine detected **194**, matched **40** by bbox IoU.
- **GriTS_Top (topology): mean 0.073, median 0.000**
- **GriTS_Con (content):  mean 0.070, median 0.000**
  (missed gold tables count as 0; this is recall-weighted over all gold tables, the standard FinTabNet.c convention.)

## Per-document

| doc | gold | pred | matched | GriTS_Top | GriTS_Con |
|-----|-----:|-----:|--------:|----------:|----------:|
| ADI_2010_page_51 | 1 | 0 | 0 | 0.000 | 0.000 |
| ADI_2014_page_38 | 1 | 0 | 0 | 0.000 | 0.000 |
| AEE_2007_page_125 | 1 | 0 | 0 | 0.000 | 0.000 |
| AEE_2017_page_148 | 2 | 0 | 0 | 0.000 | 0.000 |
| AEE_2017_page_49 | 1 | 0 | 0 | 0.000 | 0.000 |
| AES_2016_page_188 | 1 | 0 | 0 | 0.000 | 0.000 |
| AFL_2009_page_40 | 1 | 0 | 0 | 0.000 | 0.000 |
| AIG_2010_page_258 | 1 | 0 | 0 | 0.000 | 0.000 |
| AIG_2012_page_271 | 1 | 0 | 0 | 0.000 | 0.000 |
| AIG_2018_page_280 | 1 | 0 | 0 | 0.000 | 0.000 |
| AIZ_2005_page_142 | 1 | 0 | 0 | 0.000 | 0.000 |
| AMAT_2015_page_117 | 1 | 0 | 0 | 0.000 | 0.000 |
| AMP_2015_page_94 | 3 | 0 | 0 | 0.000 | 0.000 |
| AMZN_2004_page_76 | 1 | 0 | 0 | 0.000 | 0.000 |
| AON_2009_page_104 | 1 | 0 | 0 | 0.000 | 0.000 |
| APH_2016_page_35 | 1 | 0 | 0 | 0.000 | 0.000 |
| ATO_2019_page_30 | 1 | 0 | 0 | 0.000 | 0.000 |
| AWK_2013_page_131 | 1 | 0 | 0 | 0.000 | 0.000 |
| BAC_2011_page_153 | 1 | 0 | 0 | 0.000 | 0.000 |
| BAC_2016_page_172 | 1 | 0 | 0 | 0.000 | 0.000 |
| BBY_2008_page_27 | 2 | 0 | 0 | 0.000 | 0.000 |
| BDX_2009_page_79 | 1 | 0 | 0 | 0.000 | 0.000 |
| BIIB_2008_page_47 | 2 | 0 | 0 | 0.000 | 0.000 |
| BLK_2011_page_33 | 1 | 0 | 0 | 0.000 | 0.000 |
| BMY_2018_page_55 | 1 | 0 | 0 | 0.000 | 0.000 |
| CF_2015_page_32 | 2 | 0 | 0 | 0.000 | 0.000 |
| CHTR_2006_page_20 | 1 | 0 | 0 | 0.000 | 0.000 |
| CMI_2012_page_105 | 1 | 0 | 0 | 0.000 | 0.000 |
| ED_2013_page_123 | 1 | 0 | 0 | 0.000 | 0.000 |
| EL_2010_page_140 | 2 | 1 | 0 | 0.000 | 0.000 |
| EMR_2017_page_68 | 2 | 0 | 0 | 0.000 | 0.000 |
| ETR_2009_page_141 | 1 | 0 | 0 | 0.000 | 0.000 |
| ETR_2013_page_29 | 3 | 0 | 0 | 0.000 | 0.000 |
| EXR_2018_page_28 | 2 | 0 | 0 | 0.000 | 0.000 |
| FCX_2012_page_97 | 1 | 0 | 0 | 0.000 | 0.000 |
| FITB_2008_page_21 | 1 | 0 | 0 | 0.000 | 0.000 |
| FLIR_2010_page_73 | 1 | 0 | 0 | 0.000 | 0.000 |
| FLS_2012_page_67 | 1 | 0 | 0 | 0.000 | 0.000 |
| FRT_2010_page_42 | 1 | 0 | 0 | 0.000 | 0.000 |
| GD_2005_page_62 | 1 | 0 | 0 | 0.000 | 0.000 |
| GE_2012_page_148 | 1 | 0 | 0 | 0.000 | 0.000 |
| GE_2018_page_19 | 1 | 0 | 0 | 0.000 | 0.000 |
| GM_2010_page_167 | 1 | 0 | 0 | 0.000 | 0.000 |
| GPN_2002_page_48 | 1 | 0 | 0 | 0.000 | 0.000 |
| HBAN_2009_page_169 | 3 | 0 | 0 | 0.000 | 0.000 |
| HBAN_2015_page_71 | 1 | 0 | 0 | 0.000 | 0.000 |
| HBAN_2016_page_111 | 1 | 0 | 0 | 0.000 | 0.000 |
| HLT_2014_page_106 | 1 | 0 | 0 | 0.000 | 0.000 |
| HOLX_2015_page_36 | 2 | 0 | 0 | 0.000 | 0.000 |
| HON_2004_page_79 | 1 | 0 | 0 | 0.000 | 0.000 |
| HPE_2016_page_196 | 1 | 0 | 0 | 0.000 | 0.000 |
| HPQ_2006_page_111 | 2 | 0 | 0 | 0.000 | 0.000 |
| HSY_2007_page_29 | 1 | 0 | 0 | 0.000 | 0.000 |
| HUM_2015_page_113 | 1 | 0 | 0 | 0.000 | 0.000 |
| HUM_2018_page_115 | 1 | 0 | 0 | 0.000 | 0.000 |
| HWM_2016_page_140 | 1 | 0 | 0 | 0.000 | 0.000 |
| INCY_2007_page_90 | 2 | 0 | 0 | 0.000 | 0.000 |
| IPGP_2018_page_89 | 1 | 2 | 0 | 0.000 | 0.000 |
| IRM_2010_page_116 | 2 | 0 | 0 | 0.000 | 0.000 |
| IRM_2016_page_107 | 3 | 0 | 0 | 0.000 | 0.000 |
| IVZ_2017_page_133 | 1 | 2 | 0 | 0.000 | 0.000 |
| JKHY_2017_page_25 | 2 | 0 | 0 | 0.000 | 0.000 |
| JKHY_2019_page_50 | 1 | 0 | 0 | 0.000 | 0.000 |
| JPM_2006_page_108 | 1 | 0 | 0 | 0.000 | 0.000 |
| KIM_2010_page_125 | 1 | 9 | 0 | 0.000 | 0.000 |
| KMB_2010_page_18 | 1 | 0 | 0 | 0.000 | 0.000 |
| KO_2006_page_68 | 1 | 0 | 0 | 0.000 | 0.000 |
| KO_2013_page_150 | 1 | 0 | 0 | 0.000 | 0.000 |
| LKQ_2009_page_69 | 1 | 0 | 0 | 0.000 | 0.000 |
| LMT_2005_page_39 | 1 | 0 | 0 | 0.000 | 0.000 |
| L_2007_page_168 | 1 | 0 | 0 | 0.000 | 0.000 |
| MAR_2015_page_101 | 1 | 0 | 0 | 0.000 | 0.000 |
| MA_2016_page_69 | 1 | 0 | 0 | 0.000 | 0.000 |
| MCK_2006_page_26 | 1 | 0 | 0 | 0.000 | 0.000 |
| MMM_2007_page_24 | 1 | 0 | 0 | 0.000 | 0.000 |
| MNST_2015_page_100 | 1 | 0 | 0 | 0.000 | 0.000 |
| MPC_2018_page_111 | 1 | 0 | 0 | 0.000 | 0.000 |
| MRK_2012_page_57 | 1 | 0 | 0 | 0.000 | 0.000 |
| MRO_2006_page_84 | 1 | 0 | 0 | 0.000 | 0.000 |
| MSCI_2012_page_76 | 1 | 0 | 0 | 0.000 | 0.000 |
| MS_2013_page_56 | 1 | 0 | 0 | 0.000 | 0.000 |
| NEM_2008_page_65 | 1 | 0 | 0 | 0.000 | 0.000 |
| NEM_2008_page_75 | 1 | 0 | 0 | 0.000 | 0.000 |
| NRG_2013_page_77 | 1 | 0 | 0 | 0.000 | 0.000 |
| NTRS_2017_page_94 | 1 | 0 | 0 | 0.000 | 0.000 |
| NWS_2016_page_120 | 1 | 0 | 0 | 0.000 | 0.000 |
| PEAK_2006_page_110 | 1 | 0 | 0 | 0.000 | 0.000 |
| PEP_2015_page_89 | 1 | 0 | 0 | 0.000 | 0.000 |
| PNC_2012_page_267 | 1 | 0 | 0 | 0.000 | 0.000 |
| PNC_2015_page_192 | 3 | 0 | 0 | 0.000 | 0.000 |
| PNW_2013_page_165 | 1 | 0 | 0 | 0.000 | 0.000 |
| PNW_2015_page_198 | 1 | 0 | 0 | 0.000 | 0.000 |
| PRU_2005_page_83 | 1 | 0 | 0 | 0.000 | 0.000 |
| PWR_2015_page_130 | 1 | 0 | 0 | 0.000 | 0.000 |
| PXD_2005_page_109 | 1 | 0 | 0 | 0.000 | 0.000 |
| RE_2007_page_61 | 1 | 0 | 0 | 0.000 | 0.000 |
| RE_2010_page_125 | 2 | 0 | 0 | 0.000 | 0.000 |
| RL_2008_page_49 | 1 | 0 | 0 | 0.000 | 0.000 |
| SLB_2015_page_72 | 1 | 0 | 0 | 0.000 | 0.000 |
| SNA_2007_page_95 | 2 | 0 | 0 | 0.000 | 0.000 |
| SNPS_2011_page_77 | 3 | 0 | 0 | 0.000 | 0.000 |
| SNPS_2013_page_76 | 1 | 0 | 0 | 0.000 | 0.000 |
| SPGI_2017_page_64 | 1 | 0 | 0 | 0.000 | 0.000 |
| TDG_2009_page_93 | 1 | 0 | 0 | 0.000 | 0.000 |
| TTWO_2009_page_91 | 2 | 0 | 0 | 0.000 | 0.000 |
| UNH_2017_page_72 | 2 | 0 | 0 | 0.000 | 0.000 |
| VAR_2012_page_122 | 1 | 0 | 0 | 0.000 | 0.000 |
| V_2009_page_105 | 1 | 0 | 0 | 0.000 | 0.000 |
| WM_2015_page_90 | 1 | 0 | 0 | 0.000 | 0.000 |
| XEL_2005_page_70 | 1 | 0 | 0 | 0.000 | 0.000 |
| XEL_2009_page_163 | 2 | 0 | 0 | 0.000 | 0.000 |
| XEL_2013_page_98 | 1 | 0 | 0 | 0.000 | 0.000 |
| XLNX_2006_page_34 | 1 | 0 | 0 | 0.000 | 0.000 |
| ZBH_2003_page_42 | 1 | 0 | 0 | 0.000 | 0.000 |
| ZBH_2003_page_69 | 1 | 0 | 0 | 0.000 | 0.000 |
| ZION_2017_page_111 | 1 | 10 | 1 | 0.090 | 0.082 |
| STX_2006_page_88 | 1 | 8 | 1 | 0.114 | 0.086 |
| ETR_2011_page_370 | 1 | 1 | 1 | 0.172 | 0.092 |
| SBAC_2006_page_86 | 1 | 6 | 1 | 0.210 | 0.099 |
| HFC_2012_page_54 | 1 | 8 | 1 | 0.106 | 0.106 |
| HOLX_2012_page_144 | 1 | 16 | 1 | 0.167 | 0.111 |
| ADBE_2011_page_118 | 1 | 7 | 1 | 0.113 | 0.113 |
| HOLX_2010_page_70 | 1 | 5 | 1 | 0.138 | 0.120 |
| PM_2015_page_106 | 1 | 20 | 1 | 0.121 | 0.121 |
| CMI_2014_page_81 | 1 | 3 | 1 | 0.205 | 0.154 |
| PM_2017_page_77 | 1 | 6 | 1 | 0.160 | 0.160 |
| PG_2012_page_30 | 1 | 4 | 1 | 0.188 | 0.181 |
| RL_2011_page_113 | 1 | 1 | 1 | 0.276 | 0.184 |
| EFX_2017_page_112 | 2 | 9 | 2 | 0.201 | 0.201 |
| AIG_2012_page_244 | 1 | 1 | 1 | 0.353 | 0.234 |
| WRB_2016_page_127 | 1 | 16 | 1 | 0.240 | 0.240 |
| AIZ_2004_page_155 | 1 | 2 | 1 | 0.263 | 0.242 |
| UNP_2012_page_42 | 2 | 1 | 1 | 0.267 | 0.254 |
| HOLX_2010_page_106 | 1 | 3 | 1 | 0.267 | 0.267 |
| BSX_2007_page_128 | 2 | 1 | 1 | 0.333 | 0.296 |
| MKTX_2011_page_65 | 1 | 1 | 1 | 0.307 | 0.299 |
| MNST_2006_page_107 | 1 | 12 | 1 | 0.316 | 0.300 |
| VLO_2016_page_61 | 1 | 2 | 1 | 0.300 | 0.300 |
| GD_2004_page_48 | 1 | 2 | 1 | 0.410 | 0.323 |
| UNP_2012_page_66 | 1 | 1 | 1 | 0.368 | 0.349 |
| ADP_2008_page_35 | 1 | 1 | 1 | 0.294 | 0.353 |
| DISCA_2011_page_51 | 1 | 1 | 1 | 0.345 | 0.360 |
| UNP_2013_page_71 | 1 | 1 | 1 | 0.410 | 0.394 |
| UHS_2015_page_133 | 1 | 2 | 1 | 0.389 | 0.413 |
| MO_2017_page_72 | 1 | 12 | 1 | 0.462 | 0.462 |
| MKTX_2018_page_134 | 3 | 3 | 3 | 0.424 | 0.486 |
| FE_2010_page_23 | 1 | 6 | 1 | 0.568 | 0.496 |
| UNP_2012_page_64 | 3 | 4 | 3 | 0.559 | 0.576 |
| LEG_2011_page_70 | 1 | 2 | 1 | 0.727 | 0.716 |
| BMY_2008_page_82 | 1 | 2 | 1 | 0.980 | 0.951 |

## Strategy comparison: `lines` (default) vs `text` (2026-07-02 run, same 150-page corpus)

Same manifest, same GriTS harness; only `find_tables(strategy=...)` differs
(`tables_diff.py --gold --strategy text`; full text-strategy per-doc detail in
`GT-REPORT-tables-gold-text.md`).

| aggregate (186 gold tables, 150 pages) | `lines` (default) | `text` |
|---|---:|---:|
| pages with ≥1 detection | 39/150 | 148/150 |
| predicted tables / matched (IoU>0.5) | 194 / 40 | 148 / 148 |
| **recall-weighted** GriTS_Top mean (median) | **0.073** (0.000) | **0.185** (0.177) |
| **recall-weighted** GriTS_Con mean (median) | **0.070** (0.000) | **0.107** (0.078) |
| matched-only GriTS_Top mean (median) | 0.340 (0.303) | 0.233 (0.217) |
| matched-only GriTS_Con mean (median) | 0.325 (0.299) | 0.135 (0.108) |

(recall-weighted = missed gold tables count 0, the standard FinTabNet.c convention;
matched-only = mean over IoU-matched pairs, i.e. structure quality given detection.)

### Interpretation

- **`lines` (the default) is a parity result, not an engine deficiency ranking.**
  FinTabNet.c pages are borderless financial tables, so a ruling-line detector finds
  nothing on 111/150 pages. fitz (PyMuPDF) has the same `lines` default and the same
  failure mode — spot-check ADI_2010_page_51 (gold 42×4): pdfspine default 0 tables,
  fitz default 0 tables. Near-zero here means "lines strategy cannot see borderless
  tables", for both engines equally.
- **`text` reflects the engine's real detection ability on this corpus:** detection
  jumps from 39 to 148 pages and every prediction overlaps a gold table (148/148
  matched), lifting the recall-weighted headline scores ~2.5x (Top) / ~1.5x (Con).
- **The trade-off is per-match structure quality:** text mode tends to emit one
  whole-page grid that over-merges surrounding text and over-splits columns
  (e.g. ADI_2010_page_51: 48×9 predicted vs 42×4 gold; fitz text mode is coarser
  still at 64×21 on the same page), so matched-only means drop vs the rare but
  cleaner `lines` matches. Content (Con) suffers more than topology (Top) because
  cell text gets smeared across the over-split columns.
- Published deep-learning TSR (Table-Transformer, arXiv:2303.00716) reports
  GriTS ≈ 0.98 on FinTabNet.c — both heuristic strategies are far from that; these
  numbers are a baseline/tracking metric for `find_tables`, not a competitive claim.

### Provenance note

The original FinTabNet CDN (`dax-cdn.cdn.appdomain.cloud`) is decommissioned (DNS
zone SERVFAILs from public resolvers; verified 2026-07-02). Source PDFs are
Range-extracted from the verbatim HF mirror zip recorded in the header
(`Leon1207/FinTabNet` `archive.zip`, FinTabNet 1.0.0 layout; every member validated
by `%PDF` magic + exact uncompressed size — see `fetch_fintabnet.py`). The 148/148
IoU-matched predictions confirm the mirror PDFs share the annotations' coordinate
space. License unchanged: CDLA-Permissive-1.0.
