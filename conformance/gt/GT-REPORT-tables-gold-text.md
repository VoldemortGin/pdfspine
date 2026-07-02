# Table cell-structure GOLD GT — pdfspine `find_tables` vs FinTabNet.c (GriTS)

Harness: `/Users/linhan/workspace/spine/pdfspine/conformance/gt/tables_diff.py` (`--gold --strategy text`; find_tables `strategy="text"`)  
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

## Absolute cell-structure score (pdfspine `strategy="text"` vs gold)

- Gold tables scored: **186** (across 150 pages); pdfspine detected **148**, matched **148** by bbox IoU.
- **GriTS_Top (topology): mean 0.185, median 0.177**
- **GriTS_Con (content):  mean 0.107, median 0.078**
  (missed gold tables count as 0; this is recall-weighted over all gold tables, the standard FinTabNet.c convention.)

## Per-document

| doc | gold | pred | matched | GriTS_Top | GriTS_Con |
|-----|-----:|-----:|--------:|----------:|----------:|
| AES_2016_page_188 | 1 | 0 | 0 | 0.000 | 0.000 |
| CMI_2014_page_81 | 1 | 0 | 0 | 0.000 | 0.000 |
| KIM_2010_page_125 | 1 | 1 | 1 | 0.010 | 0.005 |
| PNC_2015_page_192 | 3 | 1 | 1 | 0.094 | 0.020 |
| BSX_2007_page_128 | 2 | 1 | 1 | 0.035 | 0.022 |
| MNST_2006_page_107 | 1 | 1 | 1 | 0.023 | 0.022 |
| UNH_2017_page_72 | 2 | 1 | 1 | 0.107 | 0.023 |
| UNP_2012_page_64 | 3 | 1 | 1 | 0.057 | 0.023 |
| GE_2018_page_19 | 1 | 1 | 1 | 0.102 | 0.025 |
| MNST_2015_page_100 | 1 | 1 | 1 | 0.052 | 0.026 |
| SNPS_2011_page_77 | 3 | 1 | 1 | 0.119 | 0.026 |
| ETR_2013_page_29 | 3 | 1 | 1 | 0.079 | 0.026 |
| AMP_2015_page_94 | 3 | 1 | 1 | 0.071 | 0.026 |
| LEG_2011_page_70 | 1 | 1 | 1 | 0.059 | 0.029 |
| PNW_2013_page_165 | 1 | 1 | 1 | 0.070 | 0.029 |
| JKHY_2017_page_25 | 2 | 1 | 1 | 0.073 | 0.030 |
| IRM_2010_page_116 | 2 | 1 | 1 | 0.063 | 0.031 |
| MAR_2015_page_101 | 1 | 1 | 1 | 0.047 | 0.032 |
| EFX_2017_page_112 | 2 | 1 | 1 | 0.039 | 0.032 |
| EXR_2018_page_28 | 2 | 1 | 1 | 0.120 | 0.032 |
| HUM_2018_page_115 | 1 | 1 | 1 | 0.029 | 0.032 |
| IRM_2016_page_107 | 3 | 1 | 1 | 0.093 | 0.032 |
| MO_2017_page_72 | 1 | 1 | 1 | 0.033 | 0.032 |
| AFL_2009_page_40 | 1 | 1 | 1 | 0.136 | 0.032 |
| GPN_2002_page_48 | 1 | 1 | 1 | 0.125 | 0.034 |
| TTWO_2009_page_91 | 2 | 1 | 1 | 0.042 | 0.036 |
| MPC_2018_page_111 | 1 | 1 | 1 | 0.122 | 0.037 |
| CF_2015_page_32 | 2 | 1 | 1 | 0.089 | 0.039 |
| FLIR_2010_page_73 | 1 | 1 | 1 | 0.248 | 0.039 |
| XLNX_2006_page_34 | 1 | 1 | 1 | 0.123 | 0.040 |
| PWR_2015_page_130 | 1 | 1 | 1 | 0.172 | 0.040 |
| AEE_2017_page_148 | 2 | 1 | 1 | 0.106 | 0.041 |
| ZBH_2003_page_42 | 1 | 1 | 1 | 0.183 | 0.042 |
| MMM_2007_page_24 | 1 | 1 | 1 | 0.071 | 0.042 |
| NRG_2013_page_77 | 1 | 1 | 1 | 0.117 | 0.044 |
| BIIB_2008_page_47 | 2 | 1 | 1 | 0.092 | 0.046 |
| PNC_2012_page_267 | 1 | 1 | 1 | 0.082 | 0.047 |
| WRB_2016_page_127 | 1 | 1 | 1 | 0.083 | 0.050 |
| FCX_2012_page_97 | 1 | 1 | 1 | 0.249 | 0.050 |
| XEL_2005_page_70 | 1 | 1 | 1 | 0.146 | 0.052 |
| KO_2006_page_68 | 1 | 1 | 1 | 0.072 | 0.052 |
| HBAN_2009_page_169 | 3 | 1 | 1 | 0.134 | 0.052 |
| SNA_2007_page_95 | 2 | 1 | 1 | 0.090 | 0.053 |
| BDX_2009_page_79 | 1 | 1 | 1 | 0.255 | 0.053 |
| UNP_2012_page_42 | 2 | 1 | 1 | 0.124 | 0.054 |
| WM_2015_page_90 | 1 | 1 | 1 | 0.255 | 0.055 |
| BMY_2008_page_82 | 1 | 1 | 1 | 0.119 | 0.058 |
| FITB_2008_page_21 | 1 | 1 | 1 | 0.319 | 0.058 |
| VLO_2016_page_61 | 1 | 1 | 1 | 0.220 | 0.062 |
| APH_2016_page_35 | 1 | 1 | 1 | 0.158 | 0.062 |
| HOLX_2010_page_106 | 1 | 1 | 1 | 0.233 | 0.063 |
| GE_2012_page_148 | 1 | 1 | 1 | 0.273 | 0.065 |
| AWK_2013_page_131 | 1 | 1 | 1 | 0.160 | 0.066 |
| NEM_2008_page_75 | 1 | 1 | 1 | 0.095 | 0.066 |
| FE_2010_page_23 | 1 | 1 | 1 | 0.300 | 0.066 |
| MRK_2012_page_57 | 1 | 1 | 1 | 0.250 | 0.066 |
| LMT_2005_page_39 | 1 | 1 | 1 | 0.261 | 0.068 |
| HPE_2016_page_196 | 1 | 1 | 1 | 0.161 | 0.068 |
| PXD_2005_page_109 | 1 | 1 | 1 | 0.106 | 0.068 |
| AIZ_2004_page_155 | 1 | 1 | 1 | 0.182 | 0.069 |
| MCK_2006_page_26 | 1 | 1 | 1 | 0.105 | 0.069 |
| EMR_2017_page_68 | 2 | 1 | 1 | 0.135 | 0.070 |
| RL_2011_page_113 | 1 | 1 | 1 | 0.180 | 0.070 |
| MKTX_2018_page_134 | 3 | 1 | 1 | 0.105 | 0.072 |
| HBAN_2015_page_71 | 1 | 1 | 1 | 0.100 | 0.073 |
| HLT_2014_page_106 | 1 | 1 | 1 | 0.149 | 0.075 |
| HON_2004_page_79 | 1 | 1 | 1 | 0.294 | 0.077 |
| NEM_2008_page_65 | 1 | 1 | 1 | 0.102 | 0.083 |
| PEP_2015_page_89 | 1 | 1 | 1 | 0.092 | 0.087 |
| ADI_2014_page_38 | 1 | 1 | 1 | 0.148 | 0.087 |
| HSY_2007_page_29 | 1 | 1 | 1 | 0.136 | 0.088 |
| HOLX_2015_page_36 | 2 | 1 | 1 | 0.145 | 0.092 |
| HPQ_2006_page_111 | 2 | 1 | 1 | 0.151 | 0.092 |
| XEL_2009_page_163 | 2 | 1 | 1 | 0.103 | 0.093 |
| EL_2010_page_140 | 2 | 1 | 1 | 0.141 | 0.094 |
| RE_2007_page_61 | 1 | 1 | 1 | 0.272 | 0.095 |
| SPGI_2017_page_64 | 1 | 1 | 1 | 0.151 | 0.099 |
| HOLX_2012_page_144 | 1 | 1 | 1 | 0.096 | 0.101 |
| AIG_2010_page_258 | 1 | 1 | 1 | 0.286 | 0.102 |
| RE_2010_page_125 | 2 | 1 | 1 | 0.176 | 0.103 |
| ED_2013_page_123 | 1 | 1 | 1 | 0.213 | 0.104 |
| PNW_2015_page_198 | 1 | 1 | 1 | 0.134 | 0.104 |
| GM_2010_page_167 | 1 | 1 | 1 | 0.308 | 0.105 |
| PG_2012_page_30 | 1 | 1 | 1 | 0.300 | 0.108 |
| PEAK_2006_page_110 | 1 | 1 | 1 | 0.164 | 0.108 |
| UHS_2015_page_133 | 1 | 1 | 1 | 0.262 | 0.111 |
| HOLX_2010_page_70 | 1 | 1 | 1 | 0.277 | 0.111 |
| IPGP_2018_page_89 | 1 | 1 | 1 | 0.135 | 0.114 |
| INCY_2007_page_90 | 2 | 1 | 1 | 0.111 | 0.114 |
| HBAN_2016_page_111 | 1 | 1 | 1 | 0.161 | 0.117 |
| HUM_2015_page_113 | 1 | 1 | 1 | 0.188 | 0.118 |
| NWS_2016_page_120 | 1 | 1 | 1 | 0.191 | 0.118 |
| BLK_2011_page_33 | 1 | 1 | 1 | 0.248 | 0.119 |
| ZBH_2003_page_69 | 1 | 1 | 1 | 0.246 | 0.120 |
| AIZ_2005_page_142 | 1 | 1 | 1 | 0.321 | 0.121 |
| PM_2017_page_77 | 1 | 1 | 1 | 0.183 | 0.122 |
| GD_2005_page_62 | 1 | 1 | 1 | 0.124 | 0.123 |
| FRT_2010_page_42 | 1 | 1 | 1 | 0.138 | 0.125 |
| AEE_2007_page_125 | 1 | 1 | 1 | 0.190 | 0.126 |
| MSCI_2012_page_76 | 1 | 1 | 1 | 0.151 | 0.128 |
| LKQ_2009_page_69 | 1 | 1 | 1 | 0.214 | 0.130 |
| ETR_2009_page_141 | 1 | 1 | 1 | 0.173 | 0.131 |
| RL_2008_page_49 | 1 | 1 | 1 | 0.237 | 0.132 |
| BBY_2008_page_27 | 2 | 1 | 1 | 0.251 | 0.132 |
| MA_2016_page_69 | 1 | 1 | 1 | 0.246 | 0.136 |
| ADBE_2011_page_118 | 1 | 1 | 1 | 0.381 | 0.138 |
| JKHY_2019_page_50 | 1 | 1 | 1 | 0.151 | 0.139 |
| TDG_2009_page_93 | 1 | 1 | 1 | 0.261 | 0.139 |
| PM_2015_page_106 | 1 | 1 | 1 | 0.136 | 0.141 |
| L_2007_page_168 | 1 | 1 | 1 | 0.275 | 0.142 |
| STX_2006_page_88 | 1 | 1 | 1 | 0.206 | 0.144 |
| AIG_2012_page_244 | 1 | 1 | 1 | 0.169 | 0.145 |
| HFC_2012_page_54 | 1 | 1 | 1 | 0.374 | 0.145 |
| SBAC_2006_page_86 | 1 | 1 | 1 | 0.247 | 0.148 |
| JPM_2006_page_108 | 1 | 1 | 1 | 0.249 | 0.149 |
| AMZN_2004_page_76 | 1 | 1 | 1 | 0.174 | 0.154 |
| KO_2013_page_150 | 1 | 1 | 1 | 0.176 | 0.158 |
| ADP_2008_page_35 | 1 | 1 | 1 | 0.172 | 0.161 |
| CHTR_2006_page_20 | 1 | 1 | 1 | 0.232 | 0.162 |
| CMI_2012_page_105 | 1 | 1 | 1 | 0.335 | 0.174 |
| VAR_2012_page_122 | 1 | 1 | 1 | 0.345 | 0.174 |
| GD_2004_page_48 | 1 | 1 | 1 | 0.195 | 0.175 |
| BAC_2016_page_172 | 1 | 1 | 1 | 0.268 | 0.177 |
| ATO_2019_page_30 | 1 | 1 | 1 | 0.226 | 0.183 |
| SLB_2015_page_72 | 1 | 1 | 1 | 0.255 | 0.199 |
| UNP_2013_page_71 | 1 | 1 | 1 | 0.431 | 0.200 |
| BMY_2018_page_55 | 1 | 1 | 1 | 0.269 | 0.201 |
| AIG_2018_page_280 | 1 | 1 | 1 | 0.213 | 0.203 |
| AEE_2017_page_49 | 1 | 1 | 1 | 0.298 | 0.214 |
| ZION_2017_page_111 | 1 | 1 | 1 | 0.256 | 0.243 |
| AON_2009_page_104 | 1 | 1 | 1 | 0.275 | 0.243 |
| IVZ_2017_page_133 | 1 | 1 | 1 | 0.502 | 0.246 |
| ADI_2010_page_51 | 1 | 1 | 1 | 0.451 | 0.258 |
| AMAT_2015_page_117 | 1 | 1 | 1 | 0.288 | 0.266 |
| MRO_2006_page_84 | 1 | 1 | 1 | 0.302 | 0.267 |
| V_2009_page_105 | 1 | 1 | 1 | 0.438 | 0.272 |
| SNPS_2013_page_76 | 1 | 1 | 1 | 0.524 | 0.272 |
| KMB_2010_page_18 | 1 | 1 | 1 | 0.299 | 0.274 |
| UNP_2012_page_66 | 1 | 1 | 1 | 0.435 | 0.285 |
| ETR_2011_page_370 | 1 | 1 | 1 | 0.308 | 0.286 |
| HWM_2016_page_140 | 1 | 1 | 1 | 0.267 | 0.298 |
| AIG_2012_page_271 | 1 | 1 | 1 | 0.474 | 0.333 |
| MKTX_2011_page_65 | 1 | 1 | 1 | 0.375 | 0.350 |
| NTRS_2017_page_94 | 1 | 1 | 1 | 0.555 | 0.377 |
| XEL_2013_page_98 | 1 | 1 | 1 | 0.455 | 0.388 |
| PRU_2005_page_83 | 1 | 1 | 1 | 0.497 | 0.400 |
| DISCA_2011_page_51 | 1 | 1 | 1 | 0.595 | 0.436 |
| BAC_2011_page_153 | 1 | 1 | 1 | 0.692 | 0.438 |
| FLS_2012_page_67 | 1 | 1 | 1 | 0.522 | 0.448 |
| MS_2013_page_56 | 1 | 1 | 1 | 0.694 | 0.613 |
