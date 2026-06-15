# oxide-pdf — Real-Corpus Validation Report

_Generated: 2026-06-15T11:15:56.233017+00:00 • qpdf: qpdf version 12.3.2 • oracle (PyMuPDF/pdfminer) available: True_

This is the project's first accuracy/robustness measurement on **real-world** PDFs (prior numbers used self-generated fixtures only). Oracles run locally as diff references only; **no PyMuPDF/oracle output is committed** — only similarity scores and content-free structural diff reasons.

## 1. Corpus

- **tier1** (committable, public-domain): 30 files, 26.9 MB total

Tier-1 provenance: all files are US-federal-government works (public domain, 17 U.S.C. §105) from IRS, GovInfo, CDC MMWR, NASA NTRS, USGS, and NIST — each recorded in `fixtures/MANIFEST.toml` (source/license/sha256/cleared_by/cleared_date). Tier-2 (PDF Association `pdf20examples`, CC BY-SA 4.0) is used for robustness only.

## 2. Open / Repair / Fail rate

- Opened: **30/30 (100.0%)**
- Reported as repaired: 0
- Failed to open: 0

## 3. Never-panic / Robustness

- **No aborts, no panics, no hangs** across all 30 inputs. Every open+extract ran in an isolated subprocess under a wall-clock timeout; all exited cleanly (exit 0).

## 4. Structural validity (qpdf --check on re-saved output)

- Sampled 12 opened PDFs → `doc.save()` → `qpdf --check`: **12/12 pass (100.0%)** (pass = qpdf reports no structural errors; warnings allowed).

| file | qpdf result |
|---|---|
| `cdc-mmwr-7251a1.pdf` | PASS — clean |
| `cdc-mmwr-7301a1.pdf` | PASS — clean |
| `cdc-mmwr-7302a1.pdf` | PASS — clean |
| `govinfo-cdoc110-50.pdf` | PASS — clean |
| `govinfo-hjres1.pdf` | PASS — clean |
| `govinfo-hr1.pdf` | PASS — clean |
| `govinfo-hr2.pdf` | PASS — clean |
| `govinfo-hr3056.pdf` | PASS — clean |
| `govinfo-hr815enr.pdf` | PASS — clean |
| `govinfo-s1.pdf` | PASS — clean |
| `irs-f1040.pdf` | PASS — clean |
| `irs-f1040sb.pdf` | PASS — clean |

## 5. Differential text accuracy vs PyMuPDF (headline) & pdfminer

Per-document similarity of `oxide_pdf` `get_text("text")` vs each oracle, on whitespace-normalized full-document text. Levenshtein = normalized edit similarity (sequence-level); Jaccard = word-set overlap (vocabulary-level).

| oracle | docs | Levenshtein mean | Lev. median | Jaccard mean | Jacc. median | ≥0.95 | ≥0.80 | <0.50 |
|---|---|---|---|---|---|---|---|---|
| **pymupdf** | 30 | **0.8228** | 0.884 | 0.9093 | 0.9567 | 5 | 19 | 1 |
| **pdfminer** | 30 | **0.6783** | 0.7996 | 0.9009 | 0.9309 | 4 | 15 | 6 |

**Headline (vs PyMuPDF / fitz):** mean Levenshtein **0.823**, median **0.884**, mean Jaccard **0.909** over 30 documents.

### Worst-case divergences vs pymupdf

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `irs-p501.pdf` | 0.387 | 0.882 | 197872 | 200596 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p502.pdf` | 0.537 | 0.886 | 114676 | 116058 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p15.pdf` | 0.544 | 0.937 | 301206 | 303270 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1120.pdf` | 0.658 | 0.911 | 23392 | 25922 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1065.pdf` | 0.676 | 0.917 | 22820 | 24813 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040sc.pdf` | 0.677 | 0.971 | 6350 | 6848 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hjres1.pdf` | 0.710 | 0.574 | 1493 | 1466 | moderate divergence (lev 0.71, jaccard 0.57) |
| `irs-f1040.pdf` | 0.738 | 0.905 | 9514 | 10156 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

### Worst-case divergences vs pdfminer

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `govinfo-hr1.pdf` | 0.105 | 0.700 | 227066 | 229815 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-s1.pdf` | 0.112 | 0.603 | 791947 | 802103 | 3196/9211 oxide tokens absent from oracle (spurious/mis-decoded); similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr3056.pdf` | 0.169 | 0.869 | 17695 | 17816 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr2.pdf` | 0.173 | 0.703 | 268593 | 271696 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p501.pdf` | 0.387 | 0.875 | 197872 | 206746 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040.pdf` | 0.475 | 0.904 | 9514 | 10158 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p15.pdf` | 0.531 | 0.932 | 301206 | 315342 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p502.pdf` | 0.532 | 0.878 | 114676 | 121105 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

## 6. Prioritized divergence causes (future diff-oracle fix tasks)

1. **Moderate divergence (mixed spacing/encoding)** — 15 doc(s). e.g. `cdc-mmwr-7251a1.pdf`, `govinfo-cdoc110-50.pdf`, `govinfo-hjres1.pdf`
2. **Reading-order / word-spacing differences (column/line segmentation vs fitz)** — 7 doc(s). e.g. `irs-f1040.pdf`, `irs-f1040sc.pdf`, `irs-f1065.pdf`
3. **Extra/spurious content — oxide emits text fitz does not (mis-decoded glyphs or duplicated content)** — 1 doc(s). e.g. `govinfo-s1.pdf`

---

_Methodology: each PDF is opened+extracted in an isolated subprocess (timeout per file) so a Rust panic/abort cannot crash the harness. qpdf qpdf version 12.3.2. Oracles: PyMuPDF (AGPL, local-only) primary; pdfminer.six (MIT) secondary. Similarity computed on normalized text via difflib SequenceMatcher (Levenshtein proxy) and token Jaccard._
