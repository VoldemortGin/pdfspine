# `pdf-fonts/fonts/liberation` — bundled font provenance

This directory holds the **Liberation** font programs the renderer falls back to
when a PDF names a standard-14 font (Helvetica / Times / Courier, or the
metric-compatible aliases Arial / Times New Roman / Courier New) **without**
embedding a `/FontFile*` program. They supply the missing glyph **outlines** so
non-embedded body text renders instead of appearing blank; advance-width metrics
stay authoritative via `src/std_widths.rs` (the substitute never changes
spacing).

Unlike the mapping data in `../../data/` (numeric facts under BSD-3-Clause),
these are real glyph-outline programs and are therefore licensed under the
**SIL Open Font License, Version 1.1** (a recognized-permissive, redistributable
font license). They are embedded into the library via `include_bytes!`
(`src/liberation.rs`).

## The 12 bundled faces

| File | Substitutes (base-14 family · alias) |
|---|---|
| `LiberationSans-Regular.ttf` | Helvetica · Arial |
| `LiberationSans-Bold.ttf` | Helvetica-Bold · Arial Bold |
| `LiberationSans-Italic.ttf` | Helvetica-Oblique · Arial Italic |
| `LiberationSans-BoldItalic.ttf` | Helvetica-BoldOblique · Arial Bold Italic |
| `LiberationSerif-Regular.ttf` | Times-Roman · Times New Roman |
| `LiberationSerif-Bold.ttf` | Times-Bold · Times New Roman Bold |
| `LiberationSerif-Italic.ttf` | Times-Italic · Times New Roman Italic |
| `LiberationSerif-BoldItalic.ttf` | Times-BoldItalic · Times New Roman Bold Italic |
| `LiberationMono-Regular.ttf` | Courier · Courier New |
| `LiberationMono-Bold.ttf` | Courier-Bold · Courier New Bold |
| `LiberationMono-Italic.ttf` | Courier-Oblique · Courier New Italic |
| `LiberationMono-BoldItalic.ttf` | Courier-BoldOblique · Courier New Bold Italic |

Liberation Sans / Serif / Mono are **metric-compatible** with Arial / Times New
Roman / Courier New — the standard substitutes for the Helvetica / Times /
Courier base-14 families — so substituting them preserves layout.

| field | value |
|---|---|
| **Upstream** | <https://github.com/liberationfonts/liberation-fonts> |
| **Release** | `2.1.5` (the `liberation-fonts-ttf-2.1.5.tar.gz` release asset) |
| **Canonical URL** | <https://github.com/liberationfonts/liberation-fonts/releases/tag/2.1.5> (asset `liberation-fonts-ttf-2.1.5.tar.gz`) |
| **License** | **SIL Open Font License, Version 1.1** (SPDX: `OFL-1.1`). The full license text is retained verbatim in `LICENSE` (this directory), copied byte-for-byte from the release tarball. |
| **Copyright** | Digitized data copyright (c) 2010 Google Corporation, with Reserved Font Name Arimo, Tinos and Cousine. Copyright (c) 2012 Red Hat, Inc., with Reserved Font Name Liberation. Original designer: Steve Matteson (Ascender, Inc.). |
| **Fetched** | 2026-06-19 via `gh release`/`curl` from the canonical release asset (no modification). |
| **Cleared by** | pdfspine maintainers — OFL-1.1 is a recognized-permissive, redistributable font license. |

## SHA-256 checksums (as bundled, byte-for-byte unmodified)

```
bd62a0672d0b9b6710b01df434c80ad54fa5f0835207eb7b17b7a761463067bb  LiberationMono-Bold.ttf
79451f3c09fe25116098853b7a2ca6e2436220ccc11af022979adbcf195be130  LiberationMono-BoldItalic.ttf
605c01c711b44480a7508d349dfbf3264e81fa43d69e61cfa7d10b86e764c4d1  LiberationMono-Italic.ttf
f2b83c763e8afd21709333370bed4774337fae82267937e2b5aea7e2fbd922c1  LiberationMono-Regular.ttf
788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173  LiberationSans-Bold.ttf
698da70fc191cc5f33ad4d6d3fe830fe4624b898ea2e3169955928b7c491f1ee  LiberationSans-BoldItalic.ttf
e5bae5c4cde31f22142753855f4f8fb86da6ff39955ed3c0a11248b0d16948b0  LiberationSans-Italic.ttf
76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8  LiberationSans-Regular.ttf
d754ba427cfe0bca54ae052384baa8f842da5bd6550ad4da024ac441e7a7d5ce  LiberationSerif-Bold.ttf
f17db8af71e24d2066b587546021d4f0b296be389512b658dec3c09affeb11a7  LiberationSerif-BoldItalic.ttf
0e3dea9f8d613e006ccfa62201f33e265d19167bd0907725c3e145368b04fc2e  LiberationSerif-Italic.ttf
058ea80864aef09a23f45cbec2bb5400bc3dfbdea01c3f10538a21fcb497fb74  LiberationSerif-Regular.ttf
```

## Residual (not covered)

Liberation does **not** cover the two pictographic base-14 fonts **Symbol** and
**ZapfDingbats**; `liberation_fallback` returns `None` for them, leaving them a
documented residual (no substitute, but no regression either).
