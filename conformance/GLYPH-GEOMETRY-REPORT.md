# pdfspine — Glyph Geometry Publication Verification Report

_Generated: 2026-09-05 • branch `worktree-agent-ab0626e7f9bd0c95d` • originally measured on base `75a1ace` (v0.6.1), re-verified after rebase onto `aaee2a9` (two-column correlation-table fix)_
_Oracle: PyMuPDF 1.28.2 (local diff reference only; no oracle output committed — only values we assert)_

Verification record for the glyph-geometry API expansion: the text layer now publishes each glyph's
full rendering geometry through `get_text`, so consumers no longer reverse-engineer a font size out of
a bbox nor repair rotated/sheared runs themselves.

**Scope of this report.** Commits `95f7205` (SVG backend uses the true `Trm`), `5b728d3` (interpreter
carries the geometry), `d05ff28` (published through `get_text`), plus the documentation commits.
**Not covered here** (still open at the time of writing): rebase onto the two-column table fix,
corpus/GT non-regression, performance measurement, and the span-aggregation tightening.

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

pdfspine's `span["size"]` is the *declared* `Tf` operand, so the two disagree whenever the scale lives
in `Tm` / `cm` / `Tz`. This report records the fact; the disposition (change `size`, or leave it and
direct consumers to the new `rendered_size`) is a separate decision backed by its own corpus run.

Two further oracle findings, recorded because they shaped the design:

- fitz's `rawdict` and `get_texttrace()` expose **no matrices at all**. The only place fitz surfaces a
  true quad is `get_text("xml")`'s `<char quad=...>`. So there was no existing fitz naming to match,
  and the key names are pdfspine's own.
- fitz uses **two different size semantics internally**: `rawdict` uses `sqrt(|det|)`, `get_texttrace()`
  uses `|(a, b)|`. They agree only for conformal matrices. pdfspine keeps them under distinct names
  rather than overloading one.

---

## 3. End-to-end read-back on a real PDF

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

## 4. Synthetic-stream read-back (the cases a real corpus rarely contains)

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
| Python — published keys | `PYGEO-001..008` | `python/tests/test_glyph_geometry.py` |
| Python — key-set assertions extended | `PYTEXT-003`, `PYTEXT-006` | `python/tests/test_text.py` |

All registered in `docs/test-case-catalog.md`. Expected values for the pure-matrix cases come from the
oracle table in §2, so these tests encode measured fitz behaviour rather than assumed behaviour.

---

## 6. Known open item

`DevGlyph::new` names the glyph cell's quad corners in the glyph's own y-up text space **before**
transforming, rather than calling `Quad::from_rect` on the cell and transforming the result. Taking the
corners from the rect first makes `ul` the descender corner, which lands visually at the *bottom* once
device space flips y — i.e. `ul`/`ll` come out inverted for upright text. `quad.rect()` is unaffected,
so `bbox` and every pre-existing output are byte-identical (the XML golden stayed green).

This has **not** been cross-checked against MuPDF's own corner convention on a rotated page. If a
future run finds fitz labels them differently, this is the single place to change.
