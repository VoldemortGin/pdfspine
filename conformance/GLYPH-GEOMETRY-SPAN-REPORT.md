# Glyph geometry F — span-boundary calibration

_Measured 2026-09-05. Source base: `9912a2362a924cf976d81b8af84952731d5e0ad9`._

## Result

The frozen span-geometry rule splits 8,523 previously merged
glyph seams across 79 of the frozen 300 documents. It leaves all 300 corpus
extraction JSON documents and all 30 objective GT documents unchanged on their
existing text/word score surfaces. The span-count posterior and reviewed
boundary examples agree with the implemented cuts. A known structural effect
is fragmentation of long leader-dot runs; consumers that assign semantic
meaning to each span should account for that documented structural change.

## Rule

`build_line` now permits two adjacent real glyphs to share a visual span only
when the pre-existing font, declared-size, color, and effective-glyph-flag rules
pass and their geometry also matches. The geometry helper uses:

- `norm = max(rendered_size(previous), rendered_size(current))`;
- `linear = max(abs(delta a), abs(delta b), abs(delta c), abs(delta d)) / norm`;
- `baseline = abs((current.origin - previous.origin) dot
  normal(previous.dir)) / norm`;
- the existing line direction comparison, `dot(previous.dir, current.dir) >
  0.996`.

The frozen rule retains a seam when `linear <= 0.05 + 1e-9` and
`baseline <= 0.1 + 1e-9`. The separately named `1e-9` numerical slack keeps a
nominal 5% value represented as `0.05000000000000012` from changing sides. It
does not alter the corpus policy. If either rendered size is singular, the pair
merges only when both finite linear matrix parts are exactly equal and the
absolute baseline displacement is at most `1e-6`.

The helper compares adjacent origins, so a page translation cannot change the
decision. Advance along the writing direction is intentionally absent. The
gate does not change glyph collection, line clustering, glyph ordering,
effective-size recovery, tracking, synthesized spaces, words, or serialization.
Synthetic spaces are still inserted by the existing seam path after the real
glyph pair is classified.

## Why these cuts

The frozen pre-F corpus contained 6,212,741 seams within existing spans.
Several ordinary prose seams vary continuously through 1%, 2%, 5%, and 10% in
matrix components, so the earlier `0.005` proposal had no natural geometric
boundary and would over-fragment normal runs. The 5% linear cut is a conservative
policy for visually meaningful transform changes. The 10% baseline cut
retains a measured 1-point alignment difference in 10-point tabular text while
splitting the next reviewed location-label example at 0.111130. These are
empirical policy choices, not naturally separated modes in the distribution.

Three boundary crops were rendered to ignored files under
`/tmp/pdfspine-F-review` and independently inspected:

- `govdocs1-00018.pdf`, user page 2, “Gum, C. S. 1955…”: a nominal 5% matrix
  change in ordinary scanned references is retained;
- `MA_2016_page_69.pdf`, page 1, dot leader to dollar amount: baseline `0.1`
  is retained as one tabular row;
- `govdocs1-00017.pdf`, user page 3, “Sandy Pt.” to “Prisoner’s Hbr.”:
  baseline `0.111130` joins distinct map labels and is split.

The reproducible crop commands were:

```console
pdftoppm -f 2 -l 2 -singlefile -r 144 -x 110 -y 1050 -W 820 -H 210 -png \
  conformance/gt/corpus-robustness/govdocs1-00018.pdf \
  /tmp/pdfspine-F-review/linear-005
pdftoppm -f 1 -l 1 -singlefile -r 144 -x 680 -y 690 -W 420 -H 140 -png \
  conformance/gt/corpus-fintabnet/pdfs/MA_2016_page_69.pdf \
  /tmp/pdfspine-F-review/baseline-01
pdftoppm -f 3 -l 3 -singlefile -r 216 -x 1040 -y 1600 -W 440 -H 150 -png \
  conformance/gt/corpus-robustness/govdocs1-00017.pdf \
  /tmp/pdfspine-F-review/baseline-above-01
```

## Frozen 300-document posterior

The isolated candidate used
`/tmp/pdfspine-glyph-F-env-v2/bin/python`. Its wheel is
`/tmp/pdfspine-glyph-F-wheel-v2/pdfspine-0.6.1-cp311-abi3-macosx_11_0_arm64.whl`,
SHA-256
`4f8426b4cddff4ca2938a1338b4826121320b2538e95de351e2b6c7011a241b5`.

| measure | pre-F | post-F | delta |
|---|---:|---:|---:|
| nonempty spans | 398,591 | 407,114 | +8,523 |
| within-span glyph seams | 6,212,741 | 6,204,218 | -8,523 |
| documents with a span change | — | 79 / 300 | — |
| extraction JSON documents changed | — | 0 / 300 | — |

The equal and opposite span/seam deltas show that the gate only split existing
spans. The corpus comparison stayed at 137 over-split, 329 under-split, 130
mixed, 1,887 pages, 1,246 normalized-content-equal pages, and zero oracle or
pdfspine extraction errors.

The largest changes were 605 spans in `govdocs1-00053.pdf` (leader dots), 492
in `govdocs1-00064.pdf`, 473 in `govdocs1-00018.pdf`, 409 in
`govdocs1-00056.pdf`, and 293 in `nasa-ntrs-19950009349.pdf`. Direct lookup in
the post-F output also confirmed that the NASA page 10 formula seam at linear
`1.333458`, hidden-OCR baseline seam at `1.215095`, and leader-dot-to-space seam
at `0.866951` split, while ordinary NASA prose at `0.049757` and the table seam
at baseline `0.1` remained merged.

A direct flattened-glyph projection across all 1,887 measured pages and
6,812,936 characters is byte-identical before and after F. The projection
covers character value, `seq`, order, origin, bbox, matrix, quad, rendered size,
and synthetic status. The 8,523 new boundaries were classified as 7,289 at a
space, 565 letter-to-letter, 40 at a dot, 4 digit-to-digit, and 625 other. They
split 6,394 old spans; 7,607 resulting pieces contain one glyph.

The manual word-fragmentation audit found 420 alphabetic words crossing a new
span boundary and three whose every character became its own span. Two are OCR
or formula noise: mixed-Cyrillic `КnОm` in `govdocs1-00056.pdf` page 1 and
`sitm` in `govdocs1-00067.pdf` page 0. The third is the normal abbreviation
`HPA` in the heading `300–100-HPA-LAYER` in `govdocs1-00073.pdf` page 5. Its
source geometry alternates from the H/A matrix `(8, 0, 0, -7.5)` and rendered
size `7.746` to the P matrix `(10, 0, 0, -10)` and rendered size `10`, a linear
difference around `0.225`. Splitting `H` / `P` / `A` therefore follows the
visual-geometry rule. No stable same-geometry ordinary word was found split one
span per character. This is a real structural consequence, not a zero-event
claim.

| glyphs in span | pre-F spans | post-F spans | delta |
|---|---:|---:|---:|
| 1 | 137,829 | 145,436 | +7,607 |
| 2 | 38,459 | 38,163 | -296 |
| 3–5 | 51,262 | 51,906 | +644 |
| 6–10 | 59,001 | 59,445 | +444 |
| 11–20 | 29,450 | 29,610 | +160 |
| 21–50 | 29,966 | 30,197 | +231 |
| 51+ | 52,624 | 52,357 | -267 |

## Objective GT non-regression

The same 30 objective documents were extracted before and after F with the
pinned PyMuPDF 1.24.14 / MuPDF 1.24.11 oracle. Every unrounded per-document
pdfspine metric dictionary, extracted-character count, and subset aggregate is
exactly equal:

| subset | docs | post-F lev mean / median | post-F order mean / median | exact vs pre-F |
|---|---:|---:|---:|---|
| born | 6 | 0.9803 / 0.9909 | 1.0000 / 1.0000 | PASS |
| CJK | 3 | 0.8617 / 0.8390 | 1.0000 / 1.0000 | PASS |
| Arabic | 9 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | PASS |
| EUR-Lex historical Greek slice | 5 | 0.9252 / 0.9443 | 0.9824 / 0.9869 | PASS |
| PMC historical clean slice | 7 | 0.7243 / 0.7536 | 0.9391 / 0.9962 | PASS |

See `conformance/GLYPH-GEOMETRY-GT-REPORT.md` for source fingerprints,
environment details, full metric columns, and the documented EUR-Lex scope.

## Tests

Nine public layout fixtures, `LAYOUT-SPAN-006` through `LAYOUT-SPAN-014`, cover
scale/Tz/shear, small rotation, baseline movement, ordinary and superscript
runs, ligatures, translation, singular matrices, a synthetic space across a
new span seam, and the 5% numerical boundary. Three private helper tests cover
slightly different directions under large page translation, the 10% baseline
boundary, and one-sided singular/nearly-singular pairs. Assertions preserve
flattened text, words, character order, and bbox where applicable.

```console
RUSTC="$(rustup which --toolchain 1.96.0 rustc)" \
RUSTDOC="$(rustup which --toolchain 1.96.0 rustdoc)" \
  rustup run 1.96.0 cargo test -p pdf-text
cargo fmt --all -- --check
```

The complete `pdf-text` crate run passed 282 tests with zero failures. Formatting
and `git diff --check` also passed. A separate reviewer found no blocking issue
after the translation-invariance and one-sided-singular cases were added.

The final post-G workspace gate also passed: 1,702 Rust tests passed with one
explicit profiling test ignored; the rebuilt Python extension passed 805 tests
and doctests with 63 environment/asset skips; workspace clippy denied warnings;
and Ruff, mypy, cargo-deny, and the repository drift guards passed. No F/G
failure was skipped. Full command output is preserved under
`/tmp/pdfspine-final-gates-20260905/`.
