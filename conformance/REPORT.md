# oxide-pdf — Real-Corpus Validation Report

_Generated: 2026-06-16T09:59:31.421445+00:00 • qpdf: qpdf version 12.3.2 • oracle (PyMuPDF/pdfminer) available: True_

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
| **pymupdf** | 30 | **0.8946** | 0.9226 | 0.9705 | 0.9894 | 13 | 26 | 1 |
| **pdfminer** | 30 | **0.7132** | 0.7738 | 0.9643 | 0.995 | 4 | 13 | 5 |

**Headline (vs PyMuPDF / fitz):** mean Levenshtein **0.895**, median **0.923**, mean Jaccard **0.971** over 30 documents.

### Worst-case divergences vs pymupdf

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `nasa-ntrs-19950009349.pdf` | 0.490 | 0.736 | 54229 | 46619 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `cdc-mmwr-7251a1.pdf` | 0.774 | 0.946 | 38779 | 37586 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1099msc.pdf` | 0.778 | 0.957 | 14548 | 14480 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `usgs-fs20183024.pdf` | 0.799 | 0.964 | 23161 | 22729 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-s1.pdf` | 0.804 | 0.999 | 802706 | 785714 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hjres1.pdf` | 0.828 | 0.859 | 1521 | 1466 | moderate divergence (lev 0.83, jaccard 0.86) |
| `cdc-mmwr-7301a1.pdf` | 0.836 | 0.943 | 26205 | 25605 | moderate divergence (lev 0.84, jaccard 0.94) |
| `govinfo-hr2.pdf` | 0.844 | 0.997 | 271904 | 265732 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

### Worst-case divergences vs pdfminer

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `govinfo-hr3056.pdf` | 0.185 | 0.997 | 17831 | 17816 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr1.pdf` | 0.194 | 0.999 | 230026 | 229815 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-s1.pdf` | 0.197 | 0.999 | 802706 | 802103 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr2.pdf` | 0.261 | 0.999 | 271904 | 271696 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `nasa-ntrs-19950009349.pdf` | 0.424 | 0.722 | 54229 | 47121 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1065.pdf` | 0.562 | 0.988 | 24946 | 24954 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040.pdf` | 0.588 | 0.994 | 10157 | 10158 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f1040sb.pdf` | 0.614 | 1.000 | 3126 | 3126 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

## 6. Prioritized divergence causes (future diff-oracle fix tasks)

1. **Moderate divergence (mixed spacing/encoding)** — 9 doc(s). e.g. `cdc-mmwr-7301a1.pdf`, `cdc-mmwr-7302a1.pdf`, `govinfo-hjres1.pdf`
2. **Reading-order / word-spacing differences (column/line segmentation vs fitz)** — 6 doc(s). e.g. `cdc-mmwr-7251a1.pdf`, `govinfo-hr2.pdf`, `govinfo-s1.pdf`

---

_Methodology: each PDF is opened+extracted in an isolated subprocess (timeout per file) so a Rust panic/abort cannot crash the harness. qpdf qpdf version 12.3.2. Oracles: PyMuPDF (AGPL, local-only) primary; pdfminer.six (MIT) secondary. Similarity computed on normalized text via difflib SequenceMatcher (Levenshtein proxy) and token Jaccard._
