# oxide-pdf vs fitz — Rendering Differential

_Generated 2026-06-17T14:27:52+0800 · DPI 150 · 1 page(s)/doc · oracle_available=True · 187s_

**Method:** raw RGB sample buffers -> downsampled grayscale -> windowed SSIM + MAE (pure Python; no PNG decode)

SSIM is 0..1 (1 = identical). AA / hinting / sub-pixel differences mean an exact match is not expected; SSIM ≳0.90 indicates good visual parity.

## Verdict

PARTIAL — mean SSIM 0.891. Recognizable but with systematic gaps.

## Aggregate (overall)

| docs | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|
| 46 | 46 | 0 | 0.891 | 0.9853 | 0.9641 |

## Per-corpus

| corpus | sampled/total | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|---|
| corpus-born | 6/6 | 6 | 0 | 0.9949 | 0.9947 | 0.9954 |
| corpus-eurlex | 10/40 | 10 | 0 | 0.9431 | 0.9861 | 0.9907 |
| corpus-pmc | 10/12 | 10 | 0 | 0.861 | 0.9903 | 0.9718 |
| corpus-robustness | 10/23 | 10 | 0 | 0.7263 | 0.8891 | 0.8875 |
| corpus | 10/30 | 10 | 0 | 0.9711 | 0.9776 | 0.9877 |

## Worst ~10 divergences (lowest SSIM)

| corpus/doc | page | SSIM | MAE | oxide size | fitz size | Δw×Δh | cause guess |
|---|---|---|---|---|---|---|---|
| corpus-robustness/govdocs1-00018 | 0 | -0.1745 | 226.91 | 1196×1579 | 1196×1580 | 0×-1 | oxide drew much more ink (-216 gray) — over-dark / fill or color差异 |
| corpus-eurlex/32006L0112_ES | 0 | 0.5271 | 11.68 | 1240×1754 | 1241×1754 | -1×0 | oxide drew much less ink (+12 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00000 | 0 | 0.5407 | 11.48 | 1275×1650 | 1275×1650 | 0×0 | oxide drew much less ink (+9 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00019 | 0 | 0.5583 | 17.47 | 1275×1650 | 1275×1650 | 0×0 | oxide drew much less ink (+17 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC193606 | 0 | 0.6383 | 15.55 | 1275×1669 | 1275×1669 | 0×0 | oxide drew much less ink (+9 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC193607 | 0 | 0.6383 | 15.55 | 1275×1669 | 1275×1669 | 0×0 | oxide drew much less ink (+9 gray) — missing glyphs / body text not rendered |
| corpus-pmc/PMC212688 | 0 | 0.6383 | 15.55 | 1275×1669 | 1275×1669 | 0×0 | oxide drew much less ink (+9 gray) — missing glyphs / body text not rendered |
| corpus-robustness/govdocs1-00003 | 0 | 0.7131 | 10.27 | 1650×1275 | 1650×1275 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus-pmc/PMC212689 | 0 | 0.7532 | 11.43 | 1275×1669 | 1275×1669 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus-robustness/govdocs1-00014 | 0 | 0.8249 | 6.27 | 2550×1650 | 2550×1650 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
