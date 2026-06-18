# pdfspine — Objective Ground-Truth Accuracy Report

_Generated: 2026-06-16T03:32:25.872111+00:00 • oracle (PyMuPDF/pdfminer) available: True_

Each extractor — **pdfspine**, **pymupdf** (fitz), and **pdfminer** — is scored against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not against another extractor. Cells show **mean / median**. Metrics: `lev` (edit similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order similarity). No PyMuPDF output is committed — only scores.

## 1. Headline — all docs

Corpus: **6** documents (6 with at least one extractor scored, 0 skipped).

| extractor | docs | lev | f1 | jaccard | order |
|---|---|---|---|---|---|
| **pdfspine** | 6 | 0.530 / 0.487 | 0.854 / 0.883 | 0.717 / 0.744 | 0.610 / 0.551 |
| pymupdf | 6 | 0.980 / 0.991 | 0.980 / 0.991 | 0.965 / 0.982 | 1.000 / 1.000 |
| pdfminer | 6 | 0.763 / 0.696 | 0.980 / 0.991 | 0.965 / 0.982 | 0.781 / 0.702 |

## 2. Objective match/exceed vs fitz (reading order)

Over **6** documents scored by both pdfspine and fitz against ground truth, on the `order` (reading-order) metric:

- **pdfspine ≥ fitz (match or exceed): 0/6 (0.0%)**
- pdfspine strictly beats fitz: 0
- fitz strictly beats pdfspine: 6

**Where pdfspine loses to fitz vs ground truth (fix targets):**

| doc | pdfspine order | fitz order | Δ |
|---|---|---|---|
| `3col.pdf` | 0.409 | 1.000 | -0.591 |
| `2col.pdf` | 0.549 | 1.000 | -0.451 |
| `2col-justified.pdf` | 0.550 | 1.000 | -0.450 |
| `2col-with-header.pdf` | 0.553 | 1.000 | -0.447 |
| `2col-narrow-gutter.pdf` | 0.604 | 1.000 | -0.397 |
| `1col.pdf` | 0.995 | 0.999 | -0.004 |

## 3. Per-document scores

`lev` shown per extractor (o=pdfspine, f=fitz, p=pdfminer); `ord` = order metric.

| doc | subset | gt chars | o lev | f lev | p lev | o ord | f ord | p ord | notes |
|---|---|---|---|---|---|---|---|---|---|
| `1col.pdf` | manifest | 5120 | 0.918 | 0.925 | 0.918 | 0.995 | 0.999 | 0.992 |  |
| `2col.pdf` | manifest | 5120 | 0.484 | 0.991 | 0.654 | 0.549 | 1.000 | 0.660 |  |
| `2col-justified.pdf` | manifest | 5120 | 0.486 | 0.991 | 0.654 | 0.550 | 1.000 | 0.660 |  |
| `3col.pdf` | manifest | 5120 | 0.286 | 0.991 | 0.962 | 0.409 | 1.000 | 0.970 |  |
| `2col-with-header.pdf` | manifest | 5165 | 0.488 | 0.991 | 0.656 | 0.553 | 1.000 | 0.662 |  |
| `2col-narrow-gutter.pdf` | manifest | 5120 | 0.515 | 0.992 | 0.735 | 0.604 | 1.000 | 0.741 |  |

---

_Methodology: pdfspine extracted in an isolated subprocess (project venv) under a wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted via conformance/oracle_extract.py under the oracle venv. All three scored vs the same ground truth by conformance/gt/score.py. Multi-column reading order is the known weak spot; the `order` head-to-head is the objective match/exceed signal._
