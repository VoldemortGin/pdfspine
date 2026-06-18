# pdfspine — Objective Ground-Truth Accuracy Report

_Generated: 2026-06-16T03:35:38.527556+00:00 • oracle (PyMuPDF/pdfminer) available: True_

Each extractor — **pdfspine**, **pymupdf** (fitz), and **pdfminer** — is scored against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not against another extractor. Cells show **mean / median**. Metrics: `lev` (edit similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order similarity). No PyMuPDF output is committed — only scores.

## 1. Headline — all docs

Corpus: **12** documents (12 with at least one extractor scored, 0 skipped).

| extractor | docs | lev | f1 | jaccard | order |
|---|---|---|---|---|---|
| **pdfspine** | 12 | 0.189 / 0.147 | 0.219 / 0.147 | 0.182 / 0.126 | 0.650 / 0.815 |
| pymupdf | 12 | 0.509 / 0.697 | 0.530 / 0.701 | 0.419 / 0.528 | 0.976 / 0.997 |
| pdfminer | 12 | 0.489 / 0.624 | 0.528 / 0.702 | 0.418 / 0.528 | 0.950 / 0.967 |

## 2. Objective match/exceed vs fitz (reading order)

Over **12** documents scored by both pdfspine and fitz against ground truth, on the `order` (reading-order) metric:

- **pdfspine ≥ fitz (match or exceed): 5/12 (41.7%)**
- pdfspine strictly beats fitz: 0
- fitz strictly beats pdfspine: 7

**Where pdfspine loses to fitz vs ground truth (fix targets):**

| doc | pdfspine order | fitz order | Δ |
|---|---|---|---|
| `PMC212687.pdf` | 0.083 | 0.997 | -0.913 |
| `PMC176546.pdf` | 0.161 | 0.995 | -0.834 |
| `PMC193605.pdf` | 0.250 | 0.997 | -0.747 |
| `PMC193604.pdf` | 0.250 | 0.993 | -0.743 |
| `PMC176545.pdf` | 0.444 | 0.996 | -0.552 |
| `PMC212689.pdf` | 0.643 | 0.749 | -0.106 |
| `PMC212688.pdf` | 0.991 | 1.000 | -0.009 |

## 3. Per-document scores

`lev` shown per extractor (o=pdfspine, f=fitz, p=pdfminer); `ord` = order metric.

| doc | subset | gt chars | o lev | f lev | p lev | o ord | f ord | p ord | notes |
|---|---|---|---|---|---|---|---|---|---|
| `PMC176545.pdf` | manifest | 62501 | 0.002 | 0.791 | 0.767 | 0.444 | 0.996 | 0.966 |  |
| `PMC176546.pdf` | manifest | 19968 | 0.003 | 0.705 | 0.607 | 0.161 | 0.995 | 0.856 |  |
| `PMC176547.pdf` | manifest | 1908 | 0.120 | 0.120 | 0.118 | 1.000 | 1.000 | 1.000 |  |
| `PMC176548.pdf` | manifest | 2715 | 0.173 | 0.172 | 0.170 | 1.000 | 1.000 | 1.000 |  |
| `PMC193604.pdf` | manifest | 25748 | 0.002 | 0.690 | 0.642 | 0.250 | 0.993 | 0.924 |  |
| `PMC193605.pdf` | manifest | 34617 | 0.001 | 0.777 | 0.743 | 0.250 | 0.997 | 0.955 |  |
| `PMC193606.pdf` | manifest | 2880 | 0.175 | 0.175 | 0.173 | 1.000 | 1.000 | 1.000 |  |
| `PMC193607.pdf` | manifest | 3413 | 0.202 | 0.202 | 0.202 | 0.986 | 0.986 | 1.000 |  |
| `PMC212319.pdf` | manifest | 25700 | 0.757 | 0.753 | 0.675 | 0.996 | 0.996 | 0.893 |  |
| `PMC212687.pdf` | manifest | 48479 | 0.000 | 0.791 | 0.768 | 0.083 | 0.997 | 0.967 |  |
| `PMC212688.pdf` | manifest | 3731 | 0.222 | 0.224 | 0.221 | 0.991 | 1.000 | 1.000 |  |
| `PMC212689.pdf` | manifest | 21852 | 0.605 | 0.705 | 0.784 | 0.643 | 0.749 | 0.841 |  |

---

_Methodology: pdfspine extracted in an isolated subprocess (project venv) under a wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted via conformance/oracle_extract.py under the oracle venv. All three scored vs the same ground truth by conformance/gt/score.py. Multi-column reading order is the known weak spot; the `order` head-to-head is the objective match/exceed signal._
