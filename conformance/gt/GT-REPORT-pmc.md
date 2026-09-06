# pdfspine — Objective Ground-Truth Accuracy Report

_Generated: 2026-09-06T00:40:06.292037+00:00 • oracle (PyMuPDF/pdfminer) available: True_

Each extractor — **pdfspine**, **pymupdf** (fitz), and **pdfminer** — is scored against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not against another extractor. Cells show **mean / median**. Metrics: `lev` (edit similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order similarity). No PyMuPDF output is committed — only scores.

## 1. Headline — all docs

Corpus: **7** documents (7 with at least one extractor scored, 0 skipped).

| extractor | docs | lev | f1 | jaccard | order |
|---|---|---|---|---|---|
| **pdfspine** | 7 | 0.744 / 0.754 | 0.781 / 0.779 | 0.610 / 0.566 | 0.960 / 0.996 |
| pymupdf | 7 | 0.745 / 0.753 | 0.781 / 0.779 | 0.613 / 0.567 | 0.961 / 0.996 |
| pdfminer | 7 | 0.713 / 0.743 | 0.780 / 0.779 | 0.612 / 0.564 | 0.915 / 0.924 |

## 2. Objective match/exceed vs fitz (reading order)

Over **7** documents scored by both pdfspine and fitz against ground truth, on the `order` (reading-order) metric:

- **pdfspine ≥ fitz (match or exceed): 3/7 (42.9%)**
- pdfspine strictly beats fitz: 1
- fitz strictly beats pdfspine: 4

**Where pdfspine beats fitz vs ground truth:**

| doc | pdfspine order | fitz order | Δ |
|---|---|---|---|
| `PMC212319.pdf` | 0.997 | 0.996 | +0.001 |

**Where pdfspine loses to fitz vs ground truth (fix targets):**

| doc | pdfspine order | fitz order | Δ |
|---|---|---|---|
| `PMC212689.pdf` | 0.746 | 0.749 | -0.004 |
| `PMC193604.pdf` | 0.993 | 0.993 | -0.000 |
| `PMC176545.pdf` | 0.996 | 0.996 | -0.000 |
| `PMC212687.pdf` | 0.997 | 0.997 | -0.000 |

## 3. Per-document scores

`lev` shown per extractor (o=pdfspine, f=fitz, p=pdfminer); `ord` = order metric.

| doc | subset | gt chars | o lev | f lev | p lev | o ord | f ord | p ord | notes |
|---|---|---|---|---|---|---|---|---|---|
| `PMC176545.pdf` | manifest | 62501 | 0.791 | 0.791 | 0.767 | 0.996 | 0.996 | 0.966 |  |
| `PMC176546.pdf` | manifest | 19968 | 0.705 | 0.705 | 0.609 | 0.995 | 0.995 | 0.858 |  |
| `PMC193604.pdf` | manifest | 25748 | 0.689 | 0.690 | 0.642 | 0.993 | 0.993 | 0.924 |  |
| `PMC193605.pdf` | manifest | 34617 | 0.777 | 0.777 | 0.743 | 0.997 | 0.997 | 0.955 |  |
| `PMC212319.pdf` | manifest | 25700 | 0.754 | 0.753 | 0.675 | 0.997 | 0.996 | 0.893 |  |
| `PMC212687.pdf` | manifest | 48479 | 0.791 | 0.791 | 0.768 | 0.997 | 0.997 | 0.967 |  |
| `PMC212689.pdf` | manifest | 21852 | 0.700 | 0.705 | 0.784 | 0.746 | 0.749 | 0.841 |  |

---

_Methodology: pdfspine extracted in an isolated subprocess (project venv) under a wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted via conformance/oracle_extract.py under the oracle venv. All three scored vs the same ground truth by conformance/gt/score.py. Multi-column reading order is the known weak spot; the `order` head-to-head is the objective match/exceed signal._
