# Objective table-extraction GT — pdfspine vs fitz

Objective (known-cell-grid) table accuracy, complementing `tables_diff.py` (which
uses fitz only as a pseudo-oracle). FinTabNet — the planned objective table GT —
is CDN-unreachable, so the corpus is manufactured locally with a perfect grid
ground truth.

## Reproduce

```sh
.venv/bin/python conformance/gt/born_tables.py        # regenerate corpus-tables/ (gitignored)
.venv/bin/python conformance/gt/score_tables_gt.py    # score pdfspine vs fitz vs truth
```

- `born_tables.py` — self-contained generator (pure-Python PDF writer, Helvetica;
  no fitz, no reportlab). 8 tables: bordered/borderless × 2–5 cols × finance /
  long-text / many-rows. Truth = the exact cell grid + border style, in
  `corpus-tables/manifest.json`.
- `score_tables_gt.py` — runs each engine in an isolated subprocess (pdfspine in
  the project venv, fitz in `.venv-oracle`; fitz never enters our interpreter),
  picks the border-appropriate strategy (`lines` ruled / `text` whitespace), and
  scores cell-level F1 against the truth.

## Result (2026-06)

| table type | n | pdfspine | fitz |
|---|---|---|---|
| bordered | 6 | **1.000** | 1.000 |
| borderless | 2 | **1.000** | 0.250 |
| **overall** | 8 | **1.000** | **0.812** |

pdfspine ≥ fitz on **8/8**; **strictly surpasses on the 2 borderless tables**.

## Why pdfspine wins on borderless tables

For a whitespace-aligned (no ruling lines) table, fitz's `strategy="text"`
recovers the table *structure* but drops every data row — it returns only the
header, e.g. `[['Name','Age','City'], ['','','']]`. pdfspine's `text` strategy
fills the full grid: `[['Name','Age','City'], ['Alice','30','NYC'], …]`.

This is an existing pdfspine capability advantage — **no code change** was needed.
Note: both engines need an explicit `strategy="text"` for borderless tables; the
default (`lines`) detects neither.
