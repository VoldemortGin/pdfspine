# Table cell-structure GOLD GT — pdfspine `find_tables` vs FinTabNet.c (GriTS)

Harness: `/Users/linhan/workspace/pdfspine/conformance/gt/tables_diff.py` (`--gold`)  
Metric: **GriTS** (Grid Table Similarity, Smock et al. arXiv:2303.00716 / 2203.12555) — `conformance/gt/grits.py`.  
Dataset: **FinTabNet.c** — annotations `CDLA-Permissive-2.0`, source PDFs `CDLA-Permissive-1.0`.  
Provenance: annotations from `https://huggingface.co/datasets/bsmock/FinTabNet.c/resolve/main/FinTabNet.c-PDF_Annotations.tar.gz`; source PDFs from `https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/fintabnet/pdf/`.

## Why GriTS

GriTS scores cell **topology** (row/col spans) and cell **content** in one F-score framework with per-cell partial credit, is transpose- and position-invariant (the two properties an ideal TSR metric should have), and is the canonical metric for FinTabNet.c — so the number is directly comparable to published Table-Transformer results. We compute **GriTS_Top** (topology) and **GriTS_Con** (content) via the factored 2D-MSS heuristic, a faithful stdlib port of Microsoft's reference `grits.py` (numpy→lists, `fitz.Rect` IoU→plain arithmetic; no AGPL `fitz` is imported).

## Sample / provenance / license

- Sample requested: **30** pages; fetched **30** gold annotation pages (**40** structure-eligible gold tables).
- Source PDFs fetched: **0** / 30 (pdf status: `{'unreachable': 30}`).
- Annotations license: **CDLA-Permissive-2.0** (permissive; commercial reuse OK).
- Source-PDF license: **CDLA-Permissive-1.0** (permissive).
- Only permissively-licensed data is used. The data itself is gitignored (`conformance/gt/corpus-*/`); the committed deliverables are the fetcher, the metric, the harness mode, and this report.

## Status: BLOCKED on source PDFs (no number fabricated)

The FinTabNet.c **gold annotations were fetched successfully** and parse cleanly into GriTS cells (verified: self-GriTS = 1.0 on every parsed gold table). The **GriTS metric and the full scoring harness are implemented and self-tested**. What is missing is the **source PDFs**: the `find_tables` prediction step needs the original FinTabNet single-page PDFs, whose page coordinate system the gold `pdf_bbox`/`pdf_table_bbox` annotations live in.

### Exactly what is missing

- The `bsmock/FinTabNet.c` HF dataset ships **annotations only** (`FinTabNet.c-PDF_Annotations.tar.gz` = 77,437 JSONs, **zero PDFs**).
- The matching PDFs exist only in the original FinTabNet release at `https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/fintabnet/pdf/...` (inside `dax-fintabnet/1.0.0/fintabnet.tar.gz`).
- That host (`dax-cdn.cdn.appdomain.cloud`) is **unreachable from this environment**: TLS connection fails (verify error, `http=000`) on every retry, while HuggingFace and other hosts succeed — i.e. an environment-level egress restriction on that specific CDN host, not a transient error.
- Per-page fetch status in this run: `{'unreachable': 30}`.

### How to unblock (one of)

1. Run `conformance/gt/fetch_fintabnet.py` from a network that can reach `dax-cdn.cdn.appdomain.cloud`; it will fetch the per-page PDFs and the manifest will flip `pdf_status` to `ok`. Then re-run `tables_diff.py --gold` — it is ready and will emit the absolute GriTS numbers with no further code change.
2. Or download the full `fintabnet.tar.gz` once, untar its `pdf/` tree next to the annotations, and point the manifest's per-entry `pdf` at `pdf/<TICKER>/<YEAR>/page_<N>.pdf` (the `pdf_rel_path` already recorded in the manifest).

### What IS proven now (no PDFs needed)

- `grits.py` self-test: 7 known-answer cases pass (identity=1.0, content sensitivity, topology text-blindness, spanning-cell penalty, empty/shape mismatch, LCS).
- Gold parser validated on real FinTabNet.c annotations: structure-eligible tables parse to GriTS cells with spans; self-GriTS = 1.0 on all of them.
- The pdfspine prediction path (`Table.to_html()` → GriTS cells) is implemented and unit-checked against pdfspine's actual HTML output.
- The **full scoring pipeline is verified end-to-end** on a real pdfspine detection (CDC fixture, a 3×4 table): scoring its own output as gold gives GriTS_Top = 1.000 / GriTS_Con = 1.000 (matched IoU 1.0); a gold with one column removed drops to GriTS_Top ≈ 0.52 / GriTS_Con ≈ 0.67 — i.e. worker → HTML-parse → match → GriTS works and is sensitive to structural error.

This is the optional P3-5 task; per the PRD a clean blocked report (data unobtainable in-environment) is an acceptable deliverable. The harness will produce the absolute number unchanged the moment the PDFs are reachable.

### Gold sample ready to score (per-doc)

| document_id | gold tables | gold rows×cols (largest) | pdf_status |
|-------------|------------:|--------------------------|------------|
| AIG_2010_page_258 | 1 | 14×4 | unreachable |
| AIZ_2004_page_155 | 1 | 4×5 | unreachable |
| AMAT_2015_page_117 | 1 | 19×6 | unreachable |
| AMP_2015_page_94 | 3 | 9×9 | unreachable |
| BIIB_2008_page_47 | 2 | 7×5 | unreachable |
| BLK_2011_page_33 | 1 | 10×4 | unreachable |
| BMY_2018_page_55 | 1 | 9×4 | unreachable |
| CF_2015_page_32 | 2 | 8×9 | unreachable |
| CHTR_2006_page_20 | 1 | 12×3 | unreachable |
| ED_2013_page_123 | 1 | 9×5 | unreachable |
| ETR_2013_page_29 | 3 | 6×6 | unreachable |
| FLS_2012_page_67 | 1 | 21×4 | unreachable |
| FRT_2010_page_42 | 1 | 18×7 | unreachable |
| GPN_2002_page_48 | 1 | 6×2 | unreachable |
| HLT_2014_page_106 | 1 | 5×7 | unreachable |
| IRM_2010_page_116 | 2 | 5×5 | unreachable |
| JKHY_2019_page_50 | 1 | 16×3 | unreachable |
| JPM_2006_page_108 | 1 | 8×5 | unreachable |
| KIM_2010_page_125 | 1 | 2×2 | unreachable |
| MNST_2006_page_107 | 1 | 4×3 | unreachable |
| NEM_2008_page_65 | 1 | 5×3 | unreachable |
| NEM_2008_page_75 | 1 | 4×10 | unreachable |
| PM_2015_page_106 | 1 | 15×6 | unreachable |
| RE_2010_page_125 | 2 | 15×4 | unreachable |
| SBAC_2006_page_86 | 1 | 7×4 | unreachable |
| SPGI_2017_page_64 | 1 | 12×4 | unreachable |
| UNP_2012_page_64 | 3 | 7×4 | unreachable |
| VLO_2016_page_61 | 1 | 5×3 | unreachable |
| ZBH_2003_page_42 | 1 | 7×6 | unreachable |
| ZION_2017_page_111 | 1 | 21×3 | unreachable |
