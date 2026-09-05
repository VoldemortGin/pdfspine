# pdfspine — Glyph Geometry Publication Verification Report

_Generated: 2026-09-05 • branch `worktree-agent-ab0626e7f9bd0c95d` • originally measured on base `75a1ace` (v0.6.1), re-verified after rebase onto `aaee2a9` (two-column correlation-table fix)_
_Oracle: PyMuPDF 1.28.2 (local diff reference only; no oracle output committed — only values we assert)_

Verification record for the glyph-geometry API expansion: the text layer now publishes each glyph's
full rendering geometry through `get_text`, so consumers no longer reverse-engineer a font size out of
a bbox nor repair rotated/sheared runs themselves.

**Scope of this report.** Rebased commits `86598b3` (SVG backend uses the true `Trm`), `ef51694`
(interpreter carries the geometry), `75bccd3` (published through `get_text`), plus the subsequent
corpus, performance, span-boundary, and rendered-size work. The original geometry gates below remain
the publication baseline; §§8–11 record the continuation evidence.

---

## 1. Gates

| gate | result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **0 warnings, 0 errors** |
| `cargo test --workspace --all-features` | **1687 passed / 0 failed** |
| `maturin develop --release --uv` | ok (`pdfspine-0.6.1-cp311-abi3-macosx_11_0_arm64`) |
| `pytest python/tests` | **780 passed / 66 skipped / 0 failed** |

No `-A` lint escapes were used. Baseline before this work was 1666 Rust tests.

### 1.1 Re-verification after the rebase onto `aaee2a9`

The branch was rebased onto main's two-column correlation-table fix (`aaee2a9`), which touches the same
file this work touches (`crates/pdf-text/src/layout.rs`). A clean textual rebase does not prove the two
changes agree behaviourally, so the suite was re-run on the rebased tree:

| check | result |
|---|---|
| `cargo test --workspace --all-features` | **1690 passed / 0 failed** |
| `LAYOUT-ORDER-004` two-column record grid reads row-major | ok |
| `LAYOUT-ORDER-005` two-column prose stays column-major | ok |
| `LAYOUT-ORDER-006` fully paired columns stay column-major | ok |
| `GLYPHGEO-001..015` | 15/15 ok |

The three counts reconcile exactly: **1682** (this work on base `75a1ace`) → **1687** (after the
remaining `GLYPHGEO-010..015` and `PYGEO-*` landed) → **1690** on the rebased tree, the difference being
precisely the three `LAYOUT-ORDER-004/005/006` cases `aaee2a9` adds. Nothing was lost or silently
skipped in the rebase.

**Why the two changes do not interact:** `aaee2a9` alters *region selection* (adding an
`is_two_column_record_grid` path beside `is_table_dominant`), while this work alters *glyph geometry
transport* (`DevGlyph`'s carried matrices/quad) and *field population* (`build_line`). They sit on
different decision paths within the same file.

The run completed across all 152 test targets (exit code 0). The last of them, py-bindings' `_core`
unittests, contributes 0 tests -- it is a PyO3 cdylib with no unit tests.

---

## 2. Oracle probes — what PyMuPDF's `span["size"]` actually is

Hand-assembled content streams, single `/Helvetica` glyph per page, read back through
`page.get_text("rawdict")` on PyMuPDF 1.28.2.

| probe | `Trm` linear part | fitz `span["size"]` |
|---|---|---|
| `BT /F1 12 Tf 100 700 Td (Hi) Tj ET` | (12, 0, 0, 12) | 12 |
| `BT /F1 1 Tf 12 0 0 12 100 700 Tm …` | (12, 0, 0, 12) | **12** (not the declared 1) |
| `2 0 0 2 0 0 cm` + `/F1 12 Tf` | (24, 0, 0, 24) | **24** (not the declared 12) |
| `Tm 20 0 0 10` | (20, 0, 0, 10) | **14.142136** = √200 |
| `/F1 12 Tf 50 Tz` | (6, 0, 0, 12) | **8.485281** = √72 |
| skew `Tm 12 0 6 12` | (12, 0, 6, 12) | 12 (unchanged) |
| discriminator (3, 4, −8, 6) | (3, 4, −8, 6) | **7.070711** |

The discriminator row settles it: 7.070711 is `sqrt(|3·6 − 4·(−8)|)` and matches no other candidate
formula. **fitz reports the rendered size, `sqrt(|det|)` — MuPDF's `fz_matrix_expansion`.**

Before G, pdfspine's `span["size"]` was the *declared* `Tf` operand, so the two disagreed whenever the
scale lived in `Tm` / `cm` / `Tz`. G's measured disposition is recorded in §11.

Two further oracle findings, recorded because they shaped the design:

- fitz's `rawdict` and `get_texttrace()` expose **no matrices at all**. The only place fitz surfaces a
  true quad is `get_text("xml")`'s `<char quad=...>`. So there was no existing fitz naming to match,
  and the key names are pdfspine's own.
- fitz uses **two different size semantics internally**: `rawdict` uses `sqrt(|det|)`, `get_texttrace()`
  uses `|(a, b)|`. They agree only for conformal matrices. pdfspine keeps them under distinct names
  rather than overloading one.

---

## 3. Original publication read-back on a real PDF (pre-G)

`fixtures/born/pangrams.pdf`, page 0 (612×792, no `/Rotate`), `get_text("rawdict")`:

```
block[0]  {'number': 0, 'type': 0, 'bbox': (72.0, 62.4, 393.444, 116.4), 'seq': 0}
line[0]   {'wmode': 0, 'dir': (1.0, 0.0), 'bbox': (72.0, 62.4, 312.768, 74.4),
           'number': 0, 'seq': 0}

span[0]   size 12.0            declared_size 12.0     rendered_size 12.0
          font 'Helvetica'     flags 0                color 0
          ascender 0.8         descender -0.2
          origin      (72.0, 72.0)
          bbox        (72.0, 62.4, 312.768, 74.4)
          matrix      (12.0, 0.0, 0.0, -12.0, 72.0, 72.0)
          text_matrix (1.0, 0.0, 0.0, 1.0, 72.0, 720.0)
          ctm         (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
          dir         (1.0, 0.0)
          quad        (72.0, 62.4, 312.768, 62.4, 72.0, 74.4, 312.768, 74.4)
          seq         0

chars[0]  c 'T'   origin (72.0, 72.0)   bbox (72.0, 62.4, 79.332, 74.4)
          matrix  (12.0, 0.0, 0.0, -12.0, 72.0, 72.0)
          quad    (72.0, 62.4, 79.332, 62.4, 72.0, 74.4, 79.332, 74.4)
          rendered_size 12.0   seq 0   synthetic False
```

**Invariants, all holding on this document** (span and char level both):

1. `(0,0) · matrix == origin` → `(72.0, 72.0)` ✅
2. bounding rect of `quad` == `bbox` ✅ (1e-14 float noise on the bbox side; `isclose` passes)
3. `matrix == params · text_matrix · ctm · page_transform`, where
   `params = [Tfs·Th, 0, 0, Tfs, 0, Trise]` → composed `(12.0, 0.0, 0.0, -12.0, 72.0, 72.0)` ✅
4. `rendered_size == sqrt(|det(matrix)|) == 12.0` ✅
5. `get_text("xml")`'s `<char quad>` equals the `rawdict` char `quad` value for value ✅

`page.transformation_matrix` on this page measures `(1.0, 0.0, 0.0, -1.0, -0.0, 792.0)`.

---

## 4. Original synthetic-stream read-back (pre-G)

| content stream | observed |
|---|---|
| `BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET` | `size == declared_size == 1.0`, **`rendered_size == 12.0`**, `matrix == (12,0,0,-12,100,92)`, `text_matrix == (12,0,0,12,100,700)`; char `H` `quad == (100, 82.4, 106, 82.4, 100, 94.4, 106, 94.4)` |
| skew `Tm 12 0 6 12` | char `matrix == (12.0, 0.0, 6.0, -12.0, 100.0, 92.0)`, `quad == (104.8, 82.4, 110.8, 82.4, 98.8, 94.4, 104.8, 94.4)` — a true parallelogram, `ll.x − ul.x == −6` |
| rotate 90° `Tm 0 12 -12 0` | `dir == (0.0, -1.0)`, `matrix == (0.0, -12.0, -12.0, -0.0, 100.0, 92.0)`, `text_matrix == (0.0, 12.0, -12.0, 0.0, 100.0, 700.0)` |

The first row is the point of the whole change: a `Tf 1` stream with the scale in `Tm` reports
`declared_size 1.0` alongside `rendered_size 12.0`, so a consumer reads one key instead of inferring
a size from box height.

---

## 5. Test cases landed

| suite | IDs | file |
|---|---|---|
| Rust — glyph geometry | `GLYPHGEO-001..015` | `crates/pdf-text/tests/glyph_geometry.rs` |
| Rust — SVG render matrix | `SVGTRM-001..004` | `crates/pdf-render/tests/svg.rs` |
| Python — published keys | `PYGEO-001..009` | `python/tests/test_glyph_geometry.py` |
| Python — key-set assertions extended | `PYTEXT-003`, `PYTEXT-006` | `python/tests/test_text.py` |

All registered in `docs/test-case-catalog.md`. Expected values for the pure-matrix cases come from the
oracle table in §2, so these tests encode measured fitz behaviour rather than assumed behaviour.

---

## 6. Quad corner convention — resolved

`DevGlyph::new` names the glyph cell's quad corners in the glyph's own y-up text space **before**
transforming, rather than calling `Quad::from_rect` on the cell and transforming the result. Taking the
corners from the rect first makes `ul` the descender corner, which lands visually at the *bottom* once
device space flips y — i.e. `ul`/`ll` come out inverted for upright text. `quad.rect()` is unaffected,
so `bbox` and every pre-existing output are byte-identical (the XML golden stayed green).

This was cross-checked against PyMuPDF 1.28.2 with upright, sheared and 90°
content-stream-rotated glyphs. MuPDF uses the same convention: name the corners
in the glyph's y-up frame, then transform them. For the 90° case, its
`ul -> ur` edge follows the rotated baseline upward and `ul -> ll` points right;
it does not relabel the final points by visual position. `PYGEO-009` pins this
topology. The reproducible two-engine probe is
`conformance/probe_glyph_quad_corners.py`.

The engines' absolute coordinates differ because their Helvetica fallback
vertical metrics differ, so the probe compares normalized edge directions, not
coordinate equality.

The same probe found a separate coordinate-basis difference for page-dictionary
`/Rotate`: PyMuPDF 1.28.2 text XML stays in unrotated page coordinates, while
pdfspine applies its page transform. This is now documented as a known rotated-
page difference; it does not change the confirmed corner-label convention.

## 7. Continuation branch

The continuation checkout is synced to the remote feature tip `9912a23`.
`aaee2a9` is an ancestor of that tip, and `git range-diff` confirms the old and
rebased nine-commit glyph series are patch-equivalent. Task B is complete; the
post-rebase gate evidence is recorded in §1.1.

The local continuation branch is `glyph-geometry-continue`. The earlier state in
which corpus assets and caches were absent has since been resolved; the exact,
current evidence follows.

## 8. Corpus and objective GT gates

The new trackable 300-document manifest records repository-relative paths,
source IDs, sizes, and SHA-256 values. Its portable selection fingerprint is
`87804b5a316632e3920ec198f24bdc26c50131a6344bd51dd522a858963e82c0`.
On its 1,887 pages, `aaee2a9` and `9912a23` produced 0/300 different extraction
JSON documents; post-F and post-G also remain 0/300 different on the same
word/text/font surface. The fixed PyMuPDF 1.28.2 comparison is over 137, under
329, mixed 130, with zero extraction errors.

This is a new, reproducible baseline. It does not reproduce the historical
`over 334/83, under 332/160`, whose private file list, fingerprint, oracle
version, and slash-column meaning were not retained.

The objective GT gate covers 30 fixed documents: born 6, CJK 3, Arabic 9,
five historical EUR-Lex Greek documents, and seven restored historical PMC
clean documents. Every unrounded pdfspine metric, extracted-character count,
and aggregate is exact across the baseline/current/post-F revisions. The
EUR-Lex result covers 5/40 documents and 352/2,816 pages; it must not be called
the full 40-document rerun. PMC was recovered from the replacement official
anonymous S3 metadata and reproduces the prior order mean/median
`0.9391/0.9962`.

See `GLYPH-GEOMETRY-CORPUS-REPORT.md` and
`GLYPH-GEOMETRY-GT-REPORT.md` for manifests, fingerprints, commands, and full
metrics.

## 9. Measured performance cost

On the fixed 118-page EUR-Lex sample, the original geometry build increased
streamed rawdict elapsed time by 85.9% and retained rawdict peak RSS by 141.6%.
Output-preserving Python key and immutable-float sharing reduced retained
rawdict RSS from about 699 MiB to 430 MiB. Against the same-window pre-geometry
baseline, the optimized build still costs 59.0% more streamed rawdict time,
65.3% more retained rawdict time, and 48.9% more retained rawdict RSS. Geometry
remains unconditional as requested. This is a material measured cost, not a
zero-regression performance claim. Full method and paired samples are in
`GLYPH-GEOMETRY-PERFORMANCE-REPORT.md`.

A final same-input check of the landed F/G tree against the optimized E snapshot
found +0.19% streamed and +0.75% retained rawdict time, with +1.031 MiB / +0.297
MiB RSS; the largest elapsed increase in any measured mode was 3.25%. This shows
that F/G add no new large regression; it does not erase the pre-geometry
residual costs above.

## 10. Span-boundary result

The calibrated F rule retains a real-glyph seam only when its normalized linear
difference is at most `0.05 + 1e-9`, normalized baseline difference at most
`0.1 + 1e-9`, and direction dot greater than `0.996`; singular pairs use the
documented strict fallback. It splits 8,523 former seams in 79/300 documents.
All 6,812,936 flattened character geometry records remain byte-identical.

The structural cost is explicit: 420 alphabetic runs cross a new span boundary,
and three become one span per character. Two are OCR/formula noise; the third is
the real abbreviation `HPA`, whose glyph transforms alternate materially.
Leader-dot runs also fragment. The result meets the requested visual-span rule,
but should not be described as having no over-fragmentation. See
`GLYPH-GEOMETRY-SPAN-REPORT.md`.

## 11. Rendered `span["size"]` decision

G changes structured `DictSpan.size` to the span's first-glyph
`rendered_size`; internal `Span.size` keeps the declared `Tf` value so layout
and the existing HTML/XML semantics do not change. Across 5,803,856 glyphs
uniquely matched to PyMuPDF by Unicode and origin, 5,495,174 improve, 308,367
are unchanged, and 315 worsen. Mean absolute error falls from 8.1822137 pt to
0.000114815 pt; the maximum residual is 0.6996063 pt (5.83005%). The 315
residuals occur only in GovDocs1 documents 18 and 53: F deliberately permits
small within-span geometry variation, while one span-level size necessarily
uses its first glyph. Candidate `size` equals candidate `rendered_size` for all
measured spans, and F/G text, words, and rawdict projections other than span
`size` are identical on all 1,887 pages. This is a large parity correction with
a measured residual, not perfect per-glyph parity. Full matching rules, tolerance
counts, tests, and reproduction commands are in
`GLYPH-GEOMETRY-SIZE-PARITY-REPORT.md`.

## 12. Final unified gate

After G landed, the repository-wide gate passed: `cargo fmt` clean; clippy with
`-D warnings` clean; workspace all-features **1,702 passed / 0 failed / 1 explicit
profiling test ignored**; shared Maturin release build successful; Python with
`-W error --doctest-modules` **805 passed / 63 skipped / 0 failed**; Ruff
format/check, mypy, cargo-deny, and four drift guards clean.

The first Python attempt had three failures caused solely by PyMuPDF 1.28 writing
a deprecation warning to stdout for the old `fitz` compatibility import. The
three subprocess entry points now import `pymupdf as fitz`; the full rerun passed
without adding skips. This final gate supersedes the earlier publication and
post-rebase counts in §1 while preserving them as historical evidence.
