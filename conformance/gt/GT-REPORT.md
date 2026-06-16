# oxide-pdf — Objective Ground-Truth Accuracy Report

_Generated: 2026-06-16T09:41:26.495741+00:00 • oracle (PyMuPDF/pdfminer) available: True_

Each extractor — **oxide_pdf**, **pymupdf** (fitz), and **pdfminer** — is scored against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not against another extractor. Cells show **mean / median**. Metrics: `lev` (edit similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order similarity). No PyMuPDF output is committed — only scores.

## 1. Headline — all docs

Corpus: **18** documents (18 with at least one extractor scored, 0 skipped).

| extractor | docs | lev | f1 | jaccard | order |
|---|---|---|---|---|---|
| **oxide_pdf** | 18 | 0.618 / 0.747 | 0.640 / 0.756 | 0.527 / 0.559 | 0.975 / 0.993 |
| pymupdf | 18 | 0.666 / 0.765 | 0.680 / 0.786 | 0.601 / 0.592 | 0.984 / 0.999 |
| pdfminer | 18 | 0.581 / 0.655 | 0.679 / 0.786 | 0.601 / 0.591 | 0.894 / 0.961 |

## 2. Objective match/exceed vs fitz (reading order)

Over **18** documents scored by both oxide_pdf and fitz against ground truth, on the `order` (reading-order) metric:

- **oxide ≥ fitz (match or exceed): 3/18 (16.7%)**
- oxide strictly beats fitz: 1
- fitz strictly beats oxide: 15

**Where oxide beats fitz vs ground truth:**

| doc | oxide order | fitz order | Δ |
|---|---|---|---|
| `PMC193607.pdf` | 0.990 | 0.986 | +0.004 |

**Where oxide loses to fitz vs ground truth (fix targets):**

| doc | oxide order | fitz order | Δ |
|---|---|---|---|
| `PMC212689.pdf` | 0.677 | 0.749 | -0.072 |
| `PMC193604.pdf` | 0.977 | 0.993 | -0.016 |
| `PMC176545.pdf` | 0.985 | 0.996 | -0.011 |
| `PMC176546.pdf` | 0.985 | 0.995 | -0.010 |
| `PMC212688.pdf` | 0.991 | 1.000 | -0.009 |
| `3col.pdf` | 0.993 | 1.000 | -0.007 |
| `PMC193605.pdf` | 0.990 | 0.997 | -0.007 |
| `1col.pdf` | 0.993 | 0.999 | -0.006 |
| `2col-narrow-gutter.pdf` | 0.994 | 1.000 | -0.006 |
| `PMC212687.pdf` | 0.991 | 0.997 | -0.005 |

## 3. Per-document scores

`lev` shown per extractor (o=oxide_pdf, f=fitz, p=pdfminer); `ord` = order metric.

| doc | subset | gt chars | o lev | f lev | p lev | o ord | f ord | p ord | notes |
|---|---|---|---|---|---|---|---|---|---|
| `1col.pdf` | manifest | 5120 | 0.919 | 0.925 | 0.918 | 0.993 | 0.999 | 0.992 |  |
| `2col.pdf` | manifest | 5120 | 0.918 | 0.991 | 0.654 | 0.995 | 1.000 | 0.660 |  |
| `2col-justified.pdf` | manifest | 5120 | 0.920 | 0.991 | 0.654 | 0.997 | 1.000 | 0.660 |  |
| `3col.pdf` | manifest | 5120 | 0.764 | 0.991 | 0.962 | 0.993 | 1.000 | 0.970 |  |
| `2col-with-header.pdf` | manifest | 5165 | 0.918 | 0.991 | 0.656 | 0.996 | 1.000 | 0.662 |  |
| `2col-narrow-gutter.pdf` | manifest | 5120 | 0.887 | 0.992 | 0.735 | 0.994 | 1.000 | 0.741 |  |
| `PMC176545.pdf` | manifest | 62501 | 0.747 | 0.791 | 0.767 | 0.985 | 0.996 | 0.966 |  |
| `PMC176546.pdf` | manifest | 19968 | 0.665 | 0.705 | 0.607 | 0.985 | 0.995 | 0.856 |  |
| `PMC176547.pdf` | manifest | 1908 | 0.112 | 0.120 | 0.118 | 1.000 | 1.000 | 1.000 |  |
| `PMC176548.pdf` | manifest | 2715 | 0.162 | 0.172 | 0.170 | 0.998 | 1.000 | 1.000 |  |
| `PMC193604.pdf` | manifest | 25748 | 0.639 | 0.690 | 0.642 | 0.977 | 0.993 | 0.924 |  |
| `PMC193605.pdf` | manifest | 34617 | 0.747 | 0.777 | 0.743 | 0.990 | 0.997 | 0.955 |  |
| `PMC193606.pdf` | manifest | 2880 | 0.168 | 0.175 | 0.173 | 1.000 | 1.000 | 1.000 |  |
| `PMC193607.pdf` | manifest | 3413 | 0.187 | 0.202 | 0.202 | 0.990 | 0.986 | 1.000 |  |
| `PMC212319.pdf` | manifest | 25700 | 0.749 | 0.753 | 0.675 | 0.996 | 0.996 | 0.893 |  |
| `PMC212687.pdf` | manifest | 48479 | 0.774 | 0.791 | 0.768 | 0.991 | 0.997 | 0.967 |  |
| `PMC212688.pdf` | manifest | 3731 | 0.212 | 0.224 | 0.221 | 0.991 | 1.000 | 1.000 |  |
| `PMC212689.pdf` | manifest | 21852 | 0.634 | 0.705 | 0.784 | 0.677 | 0.749 | 0.841 |  |

---

_Methodology: oxide_pdf extracted in an isolated subprocess (project venv) under a wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted via conformance/oracle_extract.py under the oracle venv. All three scored vs the same ground truth by conformance/gt/score.py. Multi-column reading order is the known weak spot; the `order` head-to-head is the objective match/exceed signal._
