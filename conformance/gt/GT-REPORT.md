# pdfspine — Objective Ground-Truth Accuracy Report

_Generated: 2026-09-06T00:54:03.175402+00:00 • oracle (PyMuPDF/pdfminer) available: True_

Each extractor — **pdfspine**, **pymupdf** (fitz), and **pdfminer** — is scored against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not against another extractor. Cells show **mean / median**. Metrics: `lev` (edit similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order similarity). No PyMuPDF output is committed — only scores.

## 1. Headline — all docs

Corpus: **53** documents (53 with at least one extractor scored, 0 skipped).

| extractor | docs | lev | f1 | jaccard | order |
|---|---|---|---|---|---|
| **pdfspine** | 53 | 0.916 / 0.945 | 0.947 / 0.982 | 0.891 / 0.960 | 0.978 / 0.985 |
| pymupdf | 53 | 0.918 / 0.958 | 0.947 / 0.982 | 0.891 / 0.959 | 0.980 / 0.986 |
| pdfminer | 53 | 0.815 / 0.865 | 0.897 / 0.982 | 0.844 / 0.946 | 0.912 / 0.933 |

## 2. Objective match/exceed vs fitz (reading order)

Over **53** documents scored by both pdfspine and fitz against ground truth, on the `order` (reading-order) metric:

- **pdfspine ≥ fitz (match or exceed): 30/53 (56.6%)**
- pdfspine strictly beats fitz: 3
- fitz strictly beats pdfspine: 23

**Where pdfspine beats fitz vs ground truth:**

| doc | pdfspine order | fitz order | Δ |
|---|---|---|---|
| `PMC212319.pdf` | 0.997 | 0.996 | +0.001 |
| `32006L0112_PL.pdf` | 0.989 | 0.989 | +0.000 |
| `32006L0112_DE.pdf` | 0.990 | 0.989 | +0.000 |

**Where pdfspine loses to fitz vs ground truth (fix targets):**

| doc | pdfspine order | fitz order | Δ |
|---|---|---|---|
| `32008L0048_PL.pdf` | 0.957 | 0.982 | -0.025 |
| `32008L0048_BG.pdf` | 0.960 | 0.983 | -0.022 |
| `32008L0048_EL.pdf` | 0.963 | 0.983 | -0.020 |
| `32008L0048_DE.pdf` | 0.965 | 0.983 | -0.017 |
| `32011L0083_PL.pdf` | 0.985 | 0.989 | -0.004 |
| `PMC212689.pdf` | 0.746 | 0.749 | -0.004 |
| `32013R0575_DE.pdf` | 0.987 | 0.991 | -0.003 |
| `32006L0112_BG.pdf` | 0.988 | 0.991 | -0.002 |
| `32011L0083_DE.pdf` | 0.985 | 0.988 | -0.002 |
| `32013R0575_PL.pdf` | 0.992 | 0.994 | -0.002 |

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
| `1col.pdf` | manifest | 5120 | 0.926 | 0.926 | 0.918 | 1.000 | 1.000 | 0.992 |  |
| `2col.pdf` | manifest | 5120 | 0.991 | 0.991 | 0.654 | 1.000 | 1.000 | 0.660 |  |
| `2col-justified.pdf` | manifest | 5120 | 0.991 | 0.991 | 0.654 | 1.000 | 1.000 | 0.660 |  |
| `3col.pdf` | manifest | 5120 | 0.991 | 0.991 | 0.962 | 1.000 | 1.000 | 0.970 |  |
| `2col-with-header.pdf` | manifest | 5165 | 0.991 | 0.991 | 0.656 | 1.000 | 1.000 | 0.662 |  |
| `2col-narrow-gutter.pdf` | manifest | 5120 | 0.992 | 0.992 | 0.735 | 1.000 | 1.000 | 0.741 |  |
| `32016R0679_EL.pdf` | manifest | 401422 | 0.969 | 0.969 | 0.963 | 0.995 | 0.995 | 0.992 |  |
| `32011L0083_EL.pdf` | manifest | 115562 | 0.938 | 0.939 | 0.843 | 0.988 | 0.990 | 0.889 |  |
| `32014R0596_EL.pdf` | manifest | 245178 | 0.944 | 0.944 | 0.936 | 0.961 | 0.961 | 0.953 |  |
| `32019R0947_EL.pdf` | manifest | 102184 | 0.963 | 0.963 | 0.961 | 0.973 | 0.973 | 0.971 |  |
| `32006L0112_EL.pdf` | manifest | 392934 | 0.801 | 0.801 | 0.673 | 0.987 | 0.987 | 0.861 |  |
| `32019R0881_EL.pdf` | manifest | 238434 | 0.942 | 0.943 | 0.817 | 0.964 | 0.965 | 0.933 |  |
| `32018R1725_EL.pdf` | manifest | 281714 | 0.974 | 0.974 | 0.968 | 0.982 | 0.982 | 0.975 |  |
| `32016L0680_EL.pdf` | manifest | 196768 | 0.971 | 0.971 | 0.967 | 0.981 | 0.981 | 0.978 |  |
| `32013R0575_EL.pdf` | manifest | 1518240 | 0.906 | 0.908 | 0.784 | 0.993 | 0.995 | 0.860 |  |
| `32008L0048_EL.pdf` | manifest | 114752 | 0.953 | 0.972 | 0.866 | 0.963 | 0.983 | 0.876 |  |
| `32016R0679_BG.pdf` | manifest | 363722 | 0.964 | 0.964 | 0.958 | 0.994 | 0.994 | 0.991 |  |
| `32011L0083_BG.pdf` | manifest | 110036 | 0.960 | 0.961 | 0.865 | 0.988 | 0.989 | 0.891 |  |
| `32014R0596_BG.pdf` | manifest | 224043 | 0.941 | 0.941 | 0.931 | 0.959 | 0.959 | 0.948 |  |
| `32019R0947_BG.pdf` | manifest | 94714 | 0.958 | 0.958 | 0.956 | 0.971 | 0.971 | 0.969 |  |
| `32006L0112_BG.pdf` | manifest | 363029 | 0.962 | 0.965 | 0.824 | 0.988 | 0.991 | 0.885 |  |
| `32019R0881_BG.pdf` | manifest | 222753 | 0.938 | 0.940 | 0.784 | 0.962 | 0.964 | 0.929 |  |
| `32018R1725_BG.pdf` | manifest | 253921 | 0.972 | 0.972 | 0.965 | 0.981 | 0.981 | 0.974 |  |
| `32016L0680_BG.pdf` | manifest | 179407 | 0.967 | 0.967 | 0.962 | 0.980 | 0.980 | 0.976 |  |
| `32013R0575_BG.pdf` | manifest | 1388281 | 0.932 | 0.934 | 0.815 | 0.992 | 0.994 | 0.867 |  |
| `32008L0048_BG.pdf` | manifest | 104800 | 0.947 | 0.969 | 0.884 | 0.960 | 0.983 | 0.896 |  |
| `32016R0679_PL.pdf` | manifest | 364288 | 0.971 | 0.971 | 0.961 | 0.985 | 0.985 | 0.978 |  |
| `32011L0083_PL.pdf` | manifest | 113147 | 0.943 | 0.946 | 0.846 | 0.985 | 0.989 | 0.884 |  |
| `32014R0596_PL.pdf` | manifest | 223620 | 0.935 | 0.935 | 0.918 | 0.960 | 0.960 | 0.942 |  |
| `32019R0947_PL.pdf` | manifest | 105246 | 0.960 | 0.960 | 0.955 | 0.973 | 0.973 | 0.968 |  |
| `32006L0112_PL.pdf` | manifest | 360862 | 0.795 | 0.793 | 0.650 | 0.989 | 0.989 | 0.848 |  |
| `32019R0881_PL.pdf` | manifest | 219746 | 0.930 | 0.930 | 0.000 | 0.962 | 0.962 | 1.000 |  |
| `32018R1725_PL.pdf` | manifest | 247993 | 0.972 | 0.971 | 0.962 | 0.980 | 0.980 | 0.970 |  |
| `32016L0680_PL.pdf` | manifest | 178197 | 0.965 | 0.965 | 0.954 | 0.979 | 0.979 | 0.970 |  |
| `32013R0575_PL.pdf` | manifest | 1452732 | 0.906 | 0.908 | 0.797 | 0.992 | 0.994 | 0.872 |  |
| `32008L0048_PL.pdf` | manifest | 109310 | 0.945 | 0.970 | 0.909 | 0.957 | 0.982 | 0.921 |  |
| `32016R0679_DE.pdf` | manifest | 401659 | 0.966 | 0.966 | 0.953 | 0.986 | 0.986 | 0.977 |  |
| `32011L0083_DE.pdf` | manifest | 118109 | 0.913 | 0.915 | 0.864 | 0.985 | 0.988 | 0.933 |  |
| `32014R0596_DE.pdf` | manifest | 240166 | 0.933 | 0.933 | 0.923 | 0.961 | 0.961 | 0.951 |  |
| `32019R0947_DE.pdf` | manifest | 94344 | 0.959 | 0.959 | 0.953 | 0.972 | 0.972 | 0.966 |  |
| `32006L0112_DE.pdf` | manifest | 387815 | 0.805 | 0.801 | 0.670 | 0.990 | 0.989 | 0.870 |  |
| `32019R0881_DE.pdf` | manifest | 240245 | 0.925 | 0.926 | 0.033 | 0.961 | 0.962 | 0.907 |  |
| `32018R1725_DE.pdf` | manifest | 277561 | 0.973 | 0.970 | 0.965 | 0.981 | 0.981 | 0.973 |  |
| `32016L0680_DE.pdf` | manifest | 196727 | 0.969 | 0.969 | 0.959 | 0.981 | 0.981 | 0.972 |  |
| `32013R0575_DE.pdf` | manifest | 1439702 | 0.865 | 0.868 | 0.723 | 0.987 | 0.991 | 0.825 |  |
| `32008L0048_DE.pdf` | manifest | 114380 | 0.954 | 0.971 | 0.914 | 0.965 | 0.983 | 0.927 |  |

---

_Methodology: pdfspine extracted in an isolated subprocess (project venv) under a wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted via conformance/oracle_extract.py under the oracle venv. All three scored vs the same ground truth by conformance/gt/score.py. Multi-column reading order is the known weak spot; the `order` head-to-head is the objective match/exceed signal._
