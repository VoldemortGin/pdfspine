# `pdf-fonts/data` — bundled data provenance (PRD §6.5 #2 / §10.3)

This directory holds the *permissively-licensed* reference data the font-mapping
layer embeds via `include_str!`. The project thesis is **license cleanliness**:
every embedded byte must have a recorded, affirmatively-permissive license and a
recorded upstream source. License-uncertain data is **never** embedded.

## `glyphlist.txt` — Adobe Glyph List (AGL)

| field | value |
|---|---|
| **What** | Glyph-name → Unicode scalar mapping (`name;HHHH[ HHHH …]`), 2,864 entries. |
| **Upstream** | <https://github.com/adobe-type-tools/agl-aglfn> — `glyphlist.txt` |
| **Canonical URL** | <https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/master/glyphlist.txt> |
| **Version** | AGL 2.0 line set (`# Table version: 2.0`), Copyright 2002–2019 Adobe. |
| **License** | **BSD-3-Clause** (Adobe). Full text is the comment header of the file itself (retained verbatim — the BSD clause-1 source-retention requirement is satisfied by shipping the file unmodified, header included). SPDX: `BSD-3-Clause`. |
| **Fetched** | 2026-06-15 via `curl` from the canonical raw URL (no modification). |
| **Cleared by** | oxipdf maintainers — BSD-3-Clause is in the PRD §6.3 permitted set. |

The file is shipped **byte-for-byte unmodified**; the parser ignores `#` comment
lines, so the license header travels with the data. See `NOTICE` for the
attribution that must accompany binary distributions (BSD clause 2).

## `zapfdingbats.txt` — Adobe ZapfDingbats glyph list

| field | value |
|---|---|
| **What** | ZapfDingbats `aNN` glyph-name → Unicode (Dingbats block) mapping, 202 entries. |
| **Upstream** | <https://github.com/adobe-type-tools/agl-aglfn> — `zapfdingbats.txt` |
| **Canonical URL** | <https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/master/zapfdingbats.txt> |
| **License** | **BSD-3-Clause** (Adobe) — same file/header/copyright as `glyphlist.txt`. SPDX: `BSD-3-Clause`. |
| **Fetched** | 2026-06-15 via `curl` from the canonical raw URL (no modification). |
| **Cleared by** | oxipdf maintainers — BSD-3-Clause is in the PRD §6.3 permitted set. |

The `aNN` names do **not** appear in `glyphlist.txt`, so this is a
non-overlapping fallback table consulted after the AGL; it lets the
ZapfDingbats built-in encoding resolve to real Dingbats-block code points
(e.g. `a10` → U+2721).

## Core-14 AFM width metrics — **NOT bundled (documented gap)**

PRD §6.5 #2 requires the Core-14 AFM width tables to clear counsel with an
**affirmative recognized-permissive license before M2 font-mapping merges**, and
§8.5.2 defines the fallback if that clearance cannot be established.

For this milestone **no recognized-permissive (SPDX MIT/BSD/Apache) source for
Core-14 AFM width metrics was affirmatively established**: the classic Adobe
Core14 AFM distribution carries a *custom* redistribution notice, not a
recognized SPDX permissive license. Consistent with the license-cleanliness
thesis, **no AFM width data is embedded.**

Consequence (the implemented fallback, per §8.5.2): the Core-14 framework
(font-name normalization + a width-table lookup hook) exists, but the bundled
table is empty. Unembedded standard-14 fonts that lack a `/Widths` array fall
back to `/MissingWidth` (from the `/FontDescriptor`) and then to the notdef
width (0). When a clean permissive AFM source is cleared, the table can be
populated behind the same hook with no API change. Tracked as
`WIDTHS-CORE14-GAP` in `docs/test-case-catalog.md`.

## Core-14 standard advance widths (built-in table)

`src/std_widths.rs` ships a built-in **advance-width** table for the 14 standard
fonts (`standard_font_widths` / `StandardWidths::advance` / `string_advance`),
used by `insert_text` to place and advance Base-14 text.

- These are **factual font advance-width metrics** — the numeric `WX` values (in
  the 1000-unit em / glyph space) of the 14 standard typefaces named by ISO
  32000-1 §9.6.2.2. Numeric metric facts are **not copyrightable expression**;
  they are the published, fixed metrics of those typefaces, not a creative work.
- They are encoded as a compact built-in `[u16; 95]`-per-font table (one entry
  per WinAnsi printable ASCII code U+0020..=U+007E) plus a small per-font
  Latin-1 (U+00A0..=U+00FF) overlay, written directly in `std_widths.rs`. The
  values were **not copied from any AGPL/encumbered AFM source file**; they are
  the standard, widely-published metrics, cross-checked against the anchor
  values listed in the AFM spec (e.g. Helvetica space=278, `A`=667, `i`=222;
  Times-Roman space=250, `A`=722, `.`=250; Courier monospaced at 600).
- Symbol and ZapfDingbats (pictographic, rarely used by `insert_text`) carry a
  flat default only: Symbol default 600, ZapfDingbats default 788.

This **supersedes the "NOT bundled" note above only for the simple
advance-width use in `insert_text`.** The AGL-glyph-name `core14_width` hook in
`src/widths.rs` is unchanged and still returns `None` (the AFM glyph-keyed table
remains the documented `WIDTHS-CORE14-GAP`).
