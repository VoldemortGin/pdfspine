# pdfspine vs fitz — Rendering Differential

_Generated 2026-09-05T21:30:12-0700 · DPI 150 · 1 page(s)/doc · oracle_available=True · 58s_

**Method:** raw RGB sample buffers -> downsampled grayscale -> windowed SSIM + MAE (pure Python; no PNG decode)

SSIM is 0..1 (1 = identical). AA / hinting / sub-pixel differences mean an exact match is not expected; SSIM ≳0.90 indicates good visual parity.

## Notes (2026-09-05 refresh)

- **Sampling pool drifted since the 2026-06-21 report.** The corpus fixtures changed (corpus-robustness 23→100 docs, corpus-pmc 12→7, corpus 30→27 — three CDC MMWR files can no longer be fetched from cdc.gov), so seed 1234 selected 43 documents here versus the 46 in the old report. The aggregate **means are therefore not directly comparable** across the two reports; the SSIM **medians are** (0.9879 here vs 0.9886 then).
- **This refresh covers the per-glyph coverage-mask cache (commit 23429fd) and the `Arc<Mask>` copy-on-write clip (af728f0).** On the same 43-document sample, SSIM mean/median are bit-for-bit identical before and after the optimizations (0.9471 / 0.9879), and every per-page change is within ±0.001. The one exception is corpus-robustness/govdocs1-00056 at −0.0012, from a 1/4-px sub-pixel phase quantization that shifts glyph AA edges; under an 8×8 phase experiment it returns to −0.0001, confirming positioning noise rather than an arithmetic error. The nine still-present documents from the old report's worst 10 were re-rendered page-by-page (baseline `.so` vs optimized); all stay within ±0.001:

  | doc | before | after | Δ |
  |---|---|---|---|
  | corpus/irs-f8843 | 0.9161 | 0.9171 | +0.0010 |
  | corpus/irs-fw4 | 0.9487 | 0.9488 | +0.0001 |
  | corpus-robustness/govdocs1-00000 | 0.9533 | 0.9532 | -0.0001 |
  | corpus-robustness/govdocs1-00012 | 0.9549 | 0.9548 | -0.0001 |
  | corpus-robustness/govdocs1-00005 | 0.9653 | 0.9655 | +0.0002 |
  | corpus-robustness/govdocs1-00014 | 0.9666 | 0.9665 | -0.0001 |
  | corpus-robustness/govdocs1-00019 | 0.9687 | 0.9678 | -0.0009 |
  | corpus/irs-p15 | 0.9716 | 0.9717 | +0.0001 |
  | corpus/irs-p501 | 0.9774 | 0.9775 | +0.0001 |

- **The one near-blank render is pre-existing and unrelated to this change.** corpus-robustness/govdocs1-00074 (SSIM 0.2654) renders identically on the baseline `.so`; it is a standing renderer issue, tracked separately.

## Verdict

CLOSE — mean SSIM 0.947. Broadly faithful with localized differences. 1 doc(s) render near-blank in pdfspine while fitz draws content (renderer failure).

## Aggregate (overall)

| docs | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|
| 43 | 43 | 0 | 0.9471 | 0.9879 | 0.9795 |

## Per-corpus

| corpus | sampled/total | compared | errors | SSIM mean | SSIM median | MAE-sim mean |
|---|---|---|---|---|---|---|
| corpus-born | 6/6 | 6 | 0 | 0.9949 | 0.995 | 0.9956 |
| corpus-eurlex | 10/40 | 10 | 0 | 0.9873 | 0.9883 | 0.9933 |
| corpus-robustness | 10/100 | 10 | 0 | 0.8757 | 0.9739 | 0.9562 |
| corpus-pmc | 7/7 | 7 | 0 | 0.9905 | 0.9928 | 0.9918 |
| corpus | 10/27 | 10 | 0 | 0.9191 | 0.9762 | 0.9707 |

## Worst ~10 divergences (lowest SSIM)

| corpus/doc | page | SSIM | MAE | pdfspine size | fitz size | Δw×Δh | cause guess |
|---|---|---|---|---|---|---|---|
| corpus-robustness/govdocs1-00074 | 0 | 0.2654 | 83.88 | 1275×1650 | 1275×1650 | 0×0 | pdfspine near-blank — renderer drew (almost) nothing |
| corpus-robustness/govdocs1-00088 | 0 | 0.6647 | 9.23 | 1275×1650 | 1275×1650 | 0×0 | pdfspine drew much less ink (+9 gray) — missing glyphs / body text not rendered |
| corpus/usgs-fs20183024 | 0 | 0.7607 | 42.15 | 1275×1650 | 1275×1650 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus/govinfo-hjres1 | 0 | 0.7781 | 8.26 | 1275×1650 | 1275×1650 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus/govinfo-hr2 | 0 | 0.8026 | 7.14 | 1275×1650 | 1275×1650 | 0×0 | moderate divergence — partial glyph/vector/AA differences |
| corpus/irs-fw4 | 0 | 0.9488 | 5.23 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00000 | 0 | 0.9532 | 2.93 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00004 | 0 | 0.9616 | 4.19 | 1275×1650 | 1275×1650 | 0×0 | good parity |
| corpus-robustness/govdocs1-00014 | 0 | 0.9665 | 2.93 | 2550×1650 | 2550×1650 | 0×0 | good parity |
| corpus/irs-f4868 | 0 | 0.9668 | 4.1 | 1275×1650 | 1275×1650 | 0×0 | good parity |
