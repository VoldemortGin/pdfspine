# `pdf-fonts/fonts/symbols` — bundled symbol-font provenance

This directory holds the **permissive (SIL OFL 1.1) Noto** font programs the
renderer falls back to when a PDF names one of the two *pictographic* standard-14
fonts — **Symbol** or **ZapfDingbats** — **without** embedding a `/FontFile*`
program. They supply the missing glyph **outlines** so non-embedded Symbol /
ZapfDingbats text renders real glyphs instead of appearing blank; advance-width
metrics stay authoritative via `src/std_widths.rs` (the substitute never changes
spacing).

The mirror of the Latin-text fallback (`../liberation/`, which covers
Helvetica / Times / Courier): Liberation has no Symbol/ZapfDingbats glyphs, so
those two base-14 fonts were previously a documented residual. These OFL Noto
faces close that gap. The glyph **shapes** are Noto's, not Adobe's URW
Symbol/Dingbats outlines (which are AGPL and intentionally not used), so a
full-page SSIM against a URW-based renderer on these specific pages is expected
to be `< 1.0`; the **semantics** (the correct glyph at the correct position) are
preserved.

Unlike the mapping data in `../../data/` (numeric facts under BSD-3-Clause),
these are real glyph-outline programs and are therefore licensed under the
**SIL Open Font License, Version 1.1**. They are embedded into the library via
`include_bytes!` (`src/liberation.rs`).

## The 3 bundled faces and their roles

| File | Role | Standard-14 font served |
|---|---|---|
| `NotoSansMath-Regular.ttf` | **Symbol** primary (Greek + math operators + arrows) | Symbol |
| `NotoSansSymbols2-Regular.ttf` | **ZapfDingbats** primary; **Symbol** supplement (`bullet`) | ZapfDingbats, Symbol |
| `NotoSansSymbols-Regular.ttf` | **ZapfDingbats** supplement (the five `cross*` dingbats `U+271D–U+2721`) | ZapfDingbats |

The two symbolic fonts each resolve a glyph through a small face chain (primary
then supplement); see `src/liberation.rs` (`symbol_faces` / `zapf_faces`).

## Coverage of the Symbol / ZapfDingbats glyph repertoires

The tables in `src/encodings.rs` map each Symbol / ZapfDingbats character code to
a glyph name, and `data/glyphlist.txt` + `data/zapfdingbats.txt` map that name to
Unicode. The bundled fonts are verified against the exact Unicode point sets
those tables produce:

- **ZapfDingbats: 94 / 94 (full).** `NotoSansSymbols2` covers 89; the five
  `U+271D–U+2721` cross dingbats are supplied by `NotoSansSymbols`.
- **Symbol: 95 / 97.** `NotoSansMath` covers 92; `bullet` (`U+2022`) is supplied
  by `NotoSansSymbols2`; `Ohm` (`U+2126`) and `micro` (`U+00B5`) resolve through
  their canonical Greek equivalents `Omega` (`U+03A9`) and `mu` (`U+03BC`), which
  `NotoSansMath` carries. The two residuals are:
  - `Euro` (`U+20AC`, Symbol code `0xA0`) — not in the bundled Noto symbol faces;
  - `radicalex` (`U+F8E5`, a private-use radical-extension line) — present in no
    Unicode font. Both are rare in Symbol-encoded content and render `.notdef`
    (blank), exactly as for any unmapped code.

| field | value |
|---|---|
| **Upstream (Math)** | <https://github.com/notofonts/math> (mirrored at <https://github.com/notofonts/notofonts.github.io>, path `fonts/NotoSansMath/unhinted/ttf/`) |
| **Upstream (Symbols / Symbols 2)** | <https://github.com/notofonts/symbols> (mirrored at <https://github.com/notofonts/notofonts.github.io>, path `fonts/NotoSansSymbols{,2}/unhinted/ttf/`) |
| **License** | **SIL Open Font License, Version 1.1** (SPDX: `OFL-1.1`). The full license text is retained verbatim in `LICENSE` (this directory). The OFL body is byte-identical across the Noto projects; the bundled copy is the `notofonts/symbols` `OFL.txt`. |
| **Copyright** | Copyright 2022 The Noto Project Authors (<https://github.com/notofonts/symbols>) — for `NotoSansSymbols`/`NotoSansSymbols2`. Copyright 2022 Google LLC. All Rights Reserved. — for `NotoSansMath`. |
| **Fetched** | 2026-06-21 via `curl` from the canonical `notofonts.github.io` mirror (unhinted TTF builds, no modification). |
| **Cleared by** | pdfspine maintainers — OFL-1.1 is a recognized-permissive, redistributable font license; the AGPL URW StandardSymbolsPS/Dingbats were deliberately rejected. |

## SHA-256 checksums (as bundled, byte-for-byte unmodified)

```
b127e84699212b6b2ef50aff58e0ebebeec04ffe6db1b9eb9e209c8c3d97b4aa  NotoSansMath-Regular.ttf
c4a0a80f0041ce4be81e2478faad22776d23edb98ae3f0d19bd37044820ecf9d  NotoSansSymbols2-Regular.ttf
6eea9cb4cd39269ea9f95ba5c2735f80ae74049dfc9e1a7c932a5cfc8f0c3030  NotoSansSymbols-Regular.ttf
b118dd41337806a5d4797052c77caf3bd096aed783e5eb21b4d11154351e1ac0  LICENSE
```

## Residual (not covered)

`Euro` (`U+20AC`) and `radicalex` (`U+F8E5`) in the Symbol repertoire — see the
coverage note above. These two codes render `.notdef`; everything else in both
repertoires renders a real glyph.
