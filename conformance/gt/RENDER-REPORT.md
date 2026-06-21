# pdfspine vs fitz — Rendering Differential

_Generated 2026-06-21T16:54:39+0800 · DPI 150 · 1 page(s)/doc · oracle_available=True · 95s_

**Method:** raw RGB sample buffers -> downsampled grayscale -> windowed SSIM + MAE (pure Python; no PNG decode)

SSIM is 0..1 (1 = identical). AA / hinting / sub-pixel differences mean an exact match is not expected; SSIM ≳0.90 indicates good visual parity.

## Verdict

AT/NEAR PARITY — mean SSIM 0.984. Renderer matches fitz closely (AA/hinting aside).

## Aggregate (overall)

| docs | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|
| 46 | 46 | 0 | 0.9841 | 0.9886 | 0.9918 |

## Per-corpus

| corpus | sampled/total | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|---|
| corpus-born | 6/6 | 6 | 0 | 0.9949 | 0.9947 | 0.9954 |
| corpus-eurlex | 10/40 | 10 | 0 | 0.9879 | 0.9882 | 0.9941 |
| corpus-robustness | 10/23 | 10 | 0 | 0.9767 | 0.9851 | 0.9902 |
| corpus-pmc | 10/12 | 10 | 0 | 0.991 | 0.9924 | 0.9915 |
| corpus | 10/30 | 10 | 0 | 0.9741 | 0.9786 | 0.989 |

## Worst ~10 divergences (lowest SSIM)

| corpus/doc | page | SSIM | MAE | pdfspine size | fitz size | Δw×Δh | cause guess |
|---|---|---|---|---|---|---|---|
| corpus/irs-f8843 | 0 | 0.9222 | 5.1 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus/irs-fw4 | 0 | 0.952 | 5.18 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00000 | 0 | 0.9544 | 2.93 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00012 | 0 | 0.955 | 3.16 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00014 | 0 | 0.9648 | 3.32 | 2550×1650 | 2550×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00005 | 0 | 0.9662 | 3.72 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00019 | 0 | 0.9687 | 3.32 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus/irs-p15 | 0 | 0.9722 | 3.46 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus/irs-p501 | 0 | 0.978 | 2.8 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus/cdc-mmwr-7251a1 | 0 | 0.9782 | 2.8 | 1275×1650 | 1275×1650 | 0×0 | good parity |
