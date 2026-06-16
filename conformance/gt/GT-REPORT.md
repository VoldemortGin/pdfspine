# oxide-pdf — Objective Ground-Truth Accuracy Report

_Generated: 2026-06-16T14:06:04.097373+00:00 • oracle (PyMuPDF/pdfminer) available: True_

Each extractor — **oxide_pdf**, **pymupdf** (fitz), and **pdfminer** — is scored against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not against another extractor. Cells show **mean / median**. Metrics: `lev` (edit similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order similarity). No PyMuPDF output is committed — only scores.

## 1. Headline — all docs

Corpus: **58** documents (58 with at least one extractor scored, 0 skipped).

| extractor | docs | lev | f1 | jaccard | order |
|---|---|---|---|---|---|
| **oxide_pdf** | 58 | 0.825 / 0.922 | 0.858 / 0.950 | 0.814 / 0.940 | 0.975 / 0.982 |
| pymupdf | 58 | 0.848 / 0.944 | 0.879 / 0.977 | 0.836 / 0.955 | 0.983 / 0.990 |
| pdfminer | 58 | 0.784 / 0.856 | 0.869 / 0.973 | 0.834 / 0.946 | 0.918 / 0.951 |

## 2. Objective match/exceed vs fitz (reading order)

Over **58** documents scored by both oxide_pdf and fitz against ground truth, on the `order` (reading-order) metric:

- **oxide ≥ fitz (match or exceed): 13/58 (22.4%)**
- oxide strictly beats fitz: 4
- fitz strictly beats oxide: 45

**Where oxide beats fitz vs ground truth:**

| doc | oxide order | fitz order | Δ |
|---|---|---|---|
| `32018R1725_FR.pdf` | 0.983 | 0.982 | +0.000 |
| `32018R1725_ES.pdf` | 0.980 | 0.980 | +0.000 |
| `32018R1725_BG.pdf` | 0.981 | 0.981 | +0.000 |
| `32018R1725_IT.pdf` | 0.981 | 0.981 | +0.000 |

**Where oxide loses to fitz vs ground truth (fix targets):**

| doc | oxide order | fitz order | Δ |
|---|---|---|---|
| `PMC212689.pdf` | 0.646 | 0.749 | -0.103 |
| `32011L0083_PL.pdf` | 0.967 | 0.989 | -0.022 |
| `32011L0083_BG.pdf` | 0.969 | 0.989 | -0.021 |
| `32011L0083_EL.pdf` | 0.974 | 0.990 | -0.016 |
| `32006L0112_DE.pdf` | 0.974 | 0.989 | -0.016 |
| `32011L0083_IT.pdf` | 0.975 | 0.990 | -0.015 |
| `32006L0112_IT.pdf` | 0.972 | 0.988 | -0.015 |
| `32014R0596_PL.pdf` | 0.945 | 0.960 | -0.015 |
| `32014R0596_DE.pdf` | 0.947 | 0.961 | -0.014 |
| `32011L0083_DE.pdf` | 0.974 | 0.988 | -0.014 |

## 3. Per-document scores

`lev` shown per extractor (o=oxide_pdf, f=fitz, p=pdfminer); `ord` = order metric.

| doc | subset | gt chars | o lev | f lev | p lev | o ord | f ord | p ord | notes |
|---|---|---|---|---|---|---|---|---|---|
| `1col.pdf` | manifest | 5120 | 0.919 | 0.925 | 0.918 | 0.993 | 0.999 | 0.992 |  |
| `2col.pdf` | manifest | 5120 | 0.918 | 0.991 | 0.654 | 0.995 | 1.000 | 0.660 |  |
| `2col-justified.pdf` | manifest | 5120 | 0.920 | 0.991 | 0.654 | 0.997 | 1.000 | 0.660 |  |
| `3col.pdf` | manifest | 5120 | 0.763 | 0.991 | 0.962 | 0.993 | 1.000 | 0.970 |  |
| `2col-with-header.pdf` | manifest | 5165 | 0.918 | 0.991 | 0.656 | 0.996 | 1.000 | 0.662 |  |
| `2col-narrow-gutter.pdf` | manifest | 5120 | 0.887 | 0.992 | 0.735 | 0.994 | 1.000 | 0.741 |  |
| `PMC176545.pdf` | manifest | 62501 | 0.788 | 0.791 | 0.767 | 0.995 | 0.996 | 0.966 |  |
| `PMC176546.pdf` | manifest | 19968 | 0.699 | 0.705 | 0.607 | 0.995 | 0.995 | 0.856 |  |
| `PMC176547.pdf` | manifest | 1908 | 0.120 | 0.120 | 0.118 | 1.000 | 1.000 | 1.000 |  |
| `PMC176548.pdf` | manifest | 2715 | 0.173 | 0.172 | 0.170 | 1.000 | 1.000 | 1.000 |  |
| `PMC193604.pdf` | manifest | 25748 | 0.664 | 0.690 | 0.642 | 0.986 | 0.993 | 0.924 |  |
| `PMC193605.pdf` | manifest | 34617 | 0.777 | 0.777 | 0.743 | 0.997 | 0.997 | 0.955 |  |
| `PMC193606.pdf` | manifest | 2880 | 0.175 | 0.175 | 0.173 | 1.000 | 1.000 | 1.000 |  |
| `PMC193607.pdf` | manifest | 3413 | 0.202 | 0.202 | 0.202 | 0.986 | 0.986 | 1.000 |  |
| `PMC212319.pdf` | manifest | 25700 | 0.750 | 0.753 | 0.675 | 0.996 | 0.996 | 0.893 |  |
| `PMC212687.pdf` | manifest | 48479 | 0.789 | 0.791 | 0.768 | 0.996 | 0.997 | 0.967 |  |
| `PMC212688.pdf` | manifest | 3731 | 0.222 | 0.224 | 0.221 | 0.991 | 1.000 | 1.000 |  |
| `PMC212689.pdf` | manifest | 21852 | 0.607 | 0.705 | 0.784 | 0.646 | 0.749 | 0.841 |  |
| `32016R0679_EL.pdf` | manifest | 401422 | 0.967 | 0.969 | 0.963 | 0.995 | 0.995 | 0.992 |  |
| `32011L0083_EL.pdf` | manifest | 115562 | 0.924 | 0.939 | 0.843 | 0.974 | 0.990 | 0.889 |  |
| `32014R0596_EL.pdf` | manifest | 245178 | 0.932 | 0.944 | 0.936 | 0.948 | 0.961 | 0.953 |  |
| `32006L0112_EL.pdf` | manifest | 392934 | 0.765 | 0.801 | 0.672 | 0.980 | 0.987 | 0.859 |  |
| `32018R1725_EL.pdf` | manifest | 281714 | 0.973 | 0.974 | 0.968 | 0.982 | 0.982 | 0.975 |  |
| `32016R0679_BG.pdf` | manifest | 363722 | 0.962 | 0.964 | 0.958 | 0.994 | 0.994 | 0.991 |  |
| `32011L0083_BG.pdf` | manifest | 110036 | 0.940 | 0.961 | 0.865 | 0.969 | 0.989 | 0.891 |  |
| `32014R0596_BG.pdf` | manifest | 224043 | 0.927 | 0.941 | 0.931 | 0.945 | 0.959 | 0.948 |  |
| `32006L0112_BG.pdf` | manifest | 363029 | 0.885 | 0.965 | 0.824 | 0.980 | 0.991 | 0.885 |  |
| `32018R1725_BG.pdf` | manifest | 253921 | 0.971 | 0.972 | 0.965 | 0.981 | 0.981 | 0.974 |  |
| `32016R0679_PL.pdf` | manifest | 364288 | 0.967 | 0.971 | 0.961 | 0.983 | 0.985 | 0.978 |  |
| `32011L0083_PL.pdf` | manifest | 113147 | 0.925 | 0.946 | 0.846 | 0.967 | 0.989 | 0.884 |  |
| `32014R0596_PL.pdf` | manifest | 223620 | 0.920 | 0.935 | 0.918 | 0.945 | 0.960 | 0.942 |  |
| `32006L0112_PL.pdf` | manifest | 360862 | 0.746 | 0.793 | 0.651 | 0.975 | 0.989 | 0.848 |  |
| `32018R1725_PL.pdf` | manifest | 247993 | 0.970 | 0.971 | 0.962 | 0.980 | 0.980 | 0.970 |  |
| `32016R0679_DE.pdf` | manifest | 401659 | 0.963 | 0.966 | 0.953 | 0.985 | 0.986 | 0.977 |  |
| `32011L0083_DE.pdf` | manifest | 118109 | 0.901 | 0.915 | 0.863 | 0.974 | 0.988 | 0.932 |  |
| `32014R0596_DE.pdf` | manifest | 240166 | 0.918 | 0.933 | 0.923 | 0.947 | 0.961 | 0.951 |  |
| `32006L0112_DE.pdf` | manifest | 387815 | 0.750 | 0.801 | 0.670 | 0.974 | 0.989 | 0.869 |  |
| `32018R1725_DE.pdf` | manifest | 277561 | 0.971 | 0.970 | 0.965 | 0.981 | 0.981 | 0.973 |  |
| `32016R0679_FR.pdf` | manifest | 406952 | 0.967 | 0.969 | 0.963 | 0.993 | 0.993 | 0.991 |  |
| `32011L0083_FR.pdf` | manifest | 117998 | 0.942 | 0.954 | 0.849 | 0.978 | 0.990 | 0.881 |  |
| `32014R0596_FR.pdf` | manifest | 246411 | 0.932 | 0.943 | 0.930 | 0.950 | 0.962 | 0.948 |  |
| `32006L0112_FR.pdf` | manifest | 384771 | 0.766 | 0.802 | 0.652 | 0.984 | 0.987 | 0.836 |  |
| `32018R1725_FR.pdf` | manifest | 284674 | 0.973 | 0.975 | 0.969 | 0.983 | 0.982 | 0.976 |  |
| `32016R0679_ES.pdf` | manifest | 383243 | 0.963 | 0.966 | 0.959 | 0.989 | 0.991 | 0.987 |  |
| `32011L0083_ES.pdf` | manifest | 117891 | 0.932 | 0.944 | 0.875 | 0.979 | 0.990 | 0.918 |  |
| `32014R0596_ES.pdf` | manifest | 243947 | 0.929 | 0.941 | 0.931 | 0.947 | 0.958 | 0.947 |  |
| `32006L0112_ES.pdf` | manifest | 403726 | 0.768 | 0.800 | 0.678 | 0.982 | 0.991 | 0.867 |  |
| `32018R1725_ES.pdf` | manifest | 266719 | 0.970 | 0.972 | 0.966 | 0.980 | 0.980 | 0.973 |  |
| `32016R0679_IT.pdf` | manifest | 381808 | 0.966 | 0.968 | 0.961 | 0.994 | 0.995 | 0.991 |  |
| `32011L0083_IT.pdf` | manifest | 114320 | 0.924 | 0.939 | 0.812 | 0.975 | 0.990 | 0.855 |  |
| `32014R0596_IT.pdf` | manifest | 241499 | 0.931 | 0.944 | 0.936 | 0.947 | 0.960 | 0.951 |  |
| `32006L0112_IT.pdf` | manifest | 382929 | 0.756 | 0.791 | 0.642 | 0.972 | 0.988 | 0.825 |  |
| `32018R1725_IT.pdf` | manifest | 262509 | 0.973 | 0.974 | 0.966 | 0.981 | 0.981 | 0.973 |  |
| `32016R0679_EN.pdf` | manifest | 352335 | 0.966 | 0.968 | 0.961 | 0.991 | 0.992 | 0.989 |  |
| `32011L0083_EN.pdf` | manifest | 103938 | 0.960 | 0.974 | 0.829 | 0.978 | 0.991 | 0.843 |  |
| `32014R0596_EN.pdf` | manifest | 216311 | 0.940 | 0.947 | 0.938 | 0.955 | 0.962 | 0.952 |  |
| `32006L0112_EN.pdf` | manifest | 355678 | 0.756 | 0.787 | 0.622 | 0.982 | 0.985 | 0.807 |  |
| `32018R1725_EN.pdf` | manifest | 243557 | 0.972 | 0.973 | 0.967 | 0.982 | 0.982 | 0.976 |  |

---

_Methodology: oxide_pdf extracted in an isolated subprocess (project venv) under a wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted via conformance/oracle_extract.py under the oracle venv. All three scored vs the same ground truth by conformance/gt/score.py. Multi-column reading order is the known weak spot; the `order` head-to-head is the objective match/exceed signal._
