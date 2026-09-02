# pdfspine — Real-Corpus Validation Report

_Generated: 2026-09-02T07:05:59.483258+00:00 • qpdf: qpdf version 12.3.2 • oracle (PyMuPDF/pdfminer) available: True_

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

Per-document similarity of `pdfspine` `get_text("text")` vs each oracle, on whitespace-normalized full-document text. Levenshtein = normalized edit similarity (sequence-level); Jaccard = word-set overlap (vocabulary-level).

| oracle | docs | Levenshtein mean | Lev. median | Jaccard mean | Jacc. median | ≥0.95 | ≥0.80 | <0.50 |
|---|---|---|---|---|---|---|---|---|
| **pymupdf** | 30 | **0.9612** | 0.9928 | 0.9887 | 0.9988 | 22 | 30 | 0 |
| **pdfminer** | 30 | **0.8325** | 0.8358 | 0.9685 | 0.9946 | 4 | 22 | 0 |

**Headline (vs PyMuPDF / fitz):** mean Levenshtein **0.961**, median **0.993**, mean Jaccard **0.989** over 30 documents.

### Worst-case divergences vs pymupdf

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `govinfo-hr2.pdf` | 0.819 | 1.000 | 265732 | 265732 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr3056.pdf` | 0.864 | 0.995 | 17410 | 17411 | moderate divergence (lev 0.86, jaccard 0.99) |
| `govinfo-s1.pdf` | 0.868 | 1.000 | 785714 | 785714 | moderate divergence (lev 0.87, jaccard 1.00) |
| `govinfo-hr1.pdf` | 0.876 | 1.000 | 224915 | 224915 | moderate divergence (lev 0.88, jaccard 1.00) |
| `cdc-mmwr-7301a1.pdf` | 0.888 | 1.000 | 25606 | 25605 | moderate divergence (lev 0.89, jaccard 1.00) |
| `irs-f4868.pdf` | 0.905 | 0.997 | 20960 | 20965 | moderate divergence (lev 0.91, jaccard 1.00) |
| `cdc-mmwr-7302a1.pdf` | 0.909 | 1.000 | 21680 | 21680 | moderate divergence (lev 0.91, jaccard 1.00) |
| `nasa-ntrs-19950009349.pdf` | 0.928 | 0.857 | 46351 | 46619 | (close) |

### Worst-case divergences vs pdfminer

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `irs-f1065.pdf` | 0.572 | 0.981 | 24812 | 24954 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040.pdf` | 0.604 | 0.911 | 10149 | 10158 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040sb.pdf` | 0.626 | 0.989 | 3126 | 3126 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1120.pdf` | 0.704 | 0.995 | 25922 | 26004 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040sc.pdf` | 0.711 | 0.995 | 6848 | 6848 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-s1.pdf` | 0.735 | 0.999 | 785714 | 802103 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr2.pdf` | 0.770 | 0.998 | 265732 | 271696 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr1.pdf` | 0.784 | 0.999 | 224915 | 229815 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

## 6. Prioritized divergence causes (future diff-oracle fix tasks)

1. **Moderate divergence (mixed spacing/encoding)** — 6 doc(s). e.g. `cdc-mmwr-7301a1.pdf`, `cdc-mmwr-7302a1.pdf`, `govinfo-hr1.pdf`
2. **Reading-order / word-spacing differences (column/line segmentation vs fitz)** — 1 doc(s). e.g. `govinfo-hr2.pdf`

---

_Methodology: each PDF is opened+extracted in an isolated subprocess (timeout per file) so a Rust panic/abort cannot crash the harness. qpdf qpdf version 12.3.2. Oracles: PyMuPDF (AGPL, local-only) primary; pdfminer.six (MIT) secondary. Similarity computed on normalized text via difflib SequenceMatcher (Levenshtein proxy) and token Jaccard._
