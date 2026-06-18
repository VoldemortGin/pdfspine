# pdfspine vs fitz — Rendering Differential

_Generated 2026-06-17T16:05:33+0800 · DPI 150 · 1 page(s)/doc · oracle_available=True · 218s_

**Method:** raw RGB sample buffers -> downsampled grayscale -> windowed SSIM + MAE (pure Python; no PNG decode)

SSIM is 0..1 (1 = identical). AA / hinting / sub-pixel differences mean an exact match is not expected; SSIM ≳0.90 indicates good visual parity.

## Verdict

CLOSE — mean SSIM 0.945. Broadly faithful with localized differences.

## Aggregate (overall)

| docs | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|
| 46 | 46 | 0 | 0.9447 | 0.9862 | 0.9876 |

## Per-corpus

| corpus | sampled/total | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|---|
| corpus-born | 6/6 | 6 | 0 | 0.9949 | 0.9947 | 0.9954 |
| corpus-eurlex | 10/40 | 10 | 0 | 0.9431 | 0.9861 | 0.9907 |
| corpus-pmc | 10/12 | 10 | 0 | 0.991 | 0.9923 | 0.9915 |
| corpus-robustness | 10/23 | 10 | 0 | 0.8431 | 0.9653 | 0.9757 |
| corpus | 10/30 | 10 | 0 | 0.9713 | 0.9779 | 0.9877 |

## Worst ~10 divergences (lowest SSIM)

| corpus/doc | page | SSIM | MAE | pdfspine size | fitz size | Δw×Δh | cause guess |
|---|---|---|---|---|---|---|---|
| corpus-eurlex/32006L0112_ES | 0 | 0.5271 | 11.68 | 1240×1754 | 1241×1754 | -1×0 | pdfspine drew much less ink (+12 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00000 | 0 | 0.5407 | 11.48 | 1275×1650 | 1275×1650 | 0×0 | pdfspine drew much less ink (+9 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00019 | 0 | 0.5583 | 17.47 | 1275×1650 | 1275×1650 | 0×0 | pdfspine drew much less ink (+17 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00003 | 0 | 0.7131 | 10.27 | 1650×1275 | 1650×1275 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus-robustness/govdocs1-00014 | 0 | 0.8249 | 6.27 | 2550×1650 | 2550×1650 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus-robustness/govdocs1-00012 | 0 | 0.8891 | 4.52 | 1275×1650 | 1275×1650 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus/irs-f8843 | 0 | 0.9161 | 5.25 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus/irs-fw4 | 0 | 0.9487 | 5.3 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus/irs-p15 | 0 | 0.9629 | 4.96 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00005 | 0 | 0.9653 | 3.75 | 1275×1650 | 1275×1650 | 0×0 | good parity |
