# oxide-pdf vs fitz — Rendering Differential

_Generated 2026-06-17T00:14:28+0800 · DPI 150 · 1 page(s)/doc · oracle_available=True · 162s_

**Method:** raw RGB sample buffers -> downsampled grayscale -> windowed SSIM + MAE (pure Python; no PNG decode)

SSIM is 0..1 (1 = identical). AA / hinting / sub-pixel differences mean an exact match is not expected; SSIM ≳0.90 indicates good visual parity.

## Verdict

DIVERGENT — mean SSIM 0.582. Substantial rendering differences. 1 doc(s) render near-blank in oxide while fitz draws content (renderer failure).

## Aggregate (overall)

| docs | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|
| 46 | 46 | 0 | 0.582 | 0.6016 | 0.9236 |

## Per-corpus

| corpus | sampled/total | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|---|
| corpus-born | 6/6 | 6 | 0 | 0.6458 | 0.6449 | 0.9046 |
| corpus-eurlex | 10/40 | 10 | 0 | 0.6428 | 0.6806 | 0.964 |
| corpus-robustness | 10/23 | 10 | 0 | 0.5889 | 0.6822 | 0.8732 |
| corpus-pmc | 10/12 | 10 | 0 | 0.3971 | 0.4335 | 0.918 |
| corpus | 10/30 | 10 | 0 | 0.6611 | 0.5319 | 0.9505 |

## Worst ~10 divergences (lowest SSIM)

| corpus/doc | page | SSIM | MAE | oxide size | fitz size | Δw×Δh | cause guess |
|---|---|---|---|---|---|---|---|
| corpus-robustness/govdocs1-00018 | 0 | -0.1745 | 226.91 | 1196×1579 | 1196×1580 | 0×-1 | oxide drew much more ink (-216 gray) — over-dark / fill or color差异 |
| corpus-pmc/PMC212687 | 0 | 0.3046 | 24.41 | 1238×1631 | 1238×1632 | 0×-1 | oxide drew much less ink (+24 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC176546 | 0 | 0.3117 | 23.34 | 1238×1631 | 1238×1632 | 0×-1 | oxide drew much less ink (+23 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC193604 | 0 | 0.3195 | 25.18 | 1238×1631 | 1238×1632 | 0×-1 | oxide drew much less ink (+25 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC176545 | 0 | 0.3274 | 25.4 | 1238×1631 | 1238×1632 | 0×-1 | oxide drew much less ink (+25 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00005 | 0 | 0.3345 | 21.85 | 1275×1650 | 1275×1650 | 0×0 | oxide drew much less ink (+20 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC193605 | 0 | 0.3436 | 23.24 | 1238×1631 | 1238×1632 | 0×-1 | oxide drew much less ink (+23 gray) — missing glyphs / body text not rendered |
| corpus/irs-f1099msc | 0 | 0.4135 | 22.23 | 1275×1650 | 1275×1650 | 0×0 | oxide drew much less ink (+22 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC193606 | 0 | 0.4335 | 19.1 | 1275×1669 | 1275×1669 | 0×0 | oxide drew much less ink (+19 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC193607 | 0 | 0.4335 | 19.1 | 1275×1669 | 1275×1669 | 0×0 | oxide drew much less ink (+19 gray) — missing glyphs / body text not rendered |
