# Glyph geometry — span-size anchoring experiment

_Measured 2026-09-05. Read-only experiment for PRD-NEXT §0 queue item 4
("Experiment before changing span anchoring"). No product code was changed;
`git status` shows only this report as a new file. Experiment harness and raw
result JSON are archived under
`/Volumes/ExternalSSD/pdfspine-archive/anchor-experiment/` (paths at the end)._

## Result

The public span `size` currently equals the span's **first glyph**
`rendered_size`. On the frozen 300-document G corpus this produces
**5,495,174 improved / 308,367 unchanged / 315 worsened** matched characters
against the PyMuPDF span size, relative to the F declared-size (`Tf` operand)
baseline. This experiment reproduced that 315 twice (independently) and then
re-scored four alternative span-anchoring rules on the **same** matched
characters, plus an independent corpus the G parity run never used.

Recommendation: **keep the status quo (first-glyph anchoring).** No alternative
is worth a conversion change. The full argument is in
[Recommendation](#recommendation); the short form:

- Every alternative touches **only 2 of 300 documents**. All 315 worsened
  characters — and every character an alternative would move — live in two
  scanned/OCR robustness documents (`govdocs1-00018.pdf`,
  `govdocs1-00053.pdf`). On the independent born-digital corpus **every rule is
  identical and exact** (0 worsened, 360,390 / 360,390 within strict tolerance),
  because no born-digital span has internal size variation at all.
- The best alternative (**mode**) reduces worsened 315 → 145 and moves +1,288
  characters into strict tolerance — **0.022 % of the 5.8 M matched
  characters**. `median` and `mean` and `first_nonws` are worse than `mode` on
  at least one axis; `mean` actively regresses coverage (−4,566 within strict
  tolerance).
- The entire effect exists only because F merges genuinely different-size
  glyph runs in scanned text (accumulated adjacent-transform tolerance). The
  per-character `rendered_size` is already exact for these characters
  (`char.rendered_size` is the documented per-glyph escape hatch). The real
  lever is the F seam threshold, which the PRD forbids tuning without data —
  this experiment does not touch it.

If maintainers nonetheless decide to minimize the 315, adopt **mode** — it is
the only rule that regresses no metric — never `mean` or `median`.

## Status quo and what "315 worsened" measures

The single conversion `serialize.rs::dict_span` publishes
`span.size == span.rendered_size` for `dict`/`rawdict`/`json`/`rawjson`, where
`rendered_size` is the **first glyph's** effective size. `declared_size` keeps
the original `Tf` operand. Per-character `char.rendered_size` is exact and
unchanged.

`conformance/probe_span_size_parity.py` matches non-whitespace, non-synthetic
characters to a PyMuPDF oracle by exact Unicode and bidirectionally-unique
origin within 0.01 pt (oracle origins mapped through page rotation). For each
matched character it compares the **containing pdfspine span size** with the
**containing oracle span size**. With `expected` = oracle span size,
`tol = max(1e-4, |expected|·1e-5)`:

- `improved` — `|declared − expected| − |candidate − expected| > tol`
- `worsened` — `|candidate − expected| − |declared − expected| > tol`
- `unchanged` — otherwise.

The metric is **character-weighted** against the F declared baseline. "315
worsened" are characters where the F declared size happened to match the oracle
but the first-glyph size diverged from it.

## Reproduction of the 315

Two independent reproductions, both exact:

1. **Frozen probe, frozen data.** Ran the committed
   `probe_span_size_parity.py --reuse-workers` over the archived G work
   directory (`…/pdfspine-glyph-G-full-parity-fixed/{baseline,candidate,oracle}.jsonl`).
   Result: `matched 5,803,856 · improved 5,495,174 · unchanged 308,367 ·
   worsened 315 · strict-tolerance-lost 29 · within-strict-tolerance 5,800,735`
   — identical to `GLYPH-GEOMETRY-SIZE-PARITY-REPORT.md`. Per document:
   `govdocs1-00018.pdf` 126, `govdocs1-00053.pdf` 189.
2. **New harness, fresh candidate.** A fresh `pdfspine==0.7.1` extraction
   (Python 3.12.11) re-matched against the archived PyMuPDF 1.28.2 oracle. Its
   `first_glyph` column reproduced `5,495,174 / 308,367 / 315` and the 41 spans
   below. Char origins are byte-identical to the archived candidate (geometry is
   frozen across F/G), so the reused oracle matches the same character set.

## Method

A single pdfspine rawdict pass captures, per span, the values every anchoring
rule needs — all derived from data already in rawdict, so no rule requires a
product-code or wheel change:

| column | rule | definition |
|---|---|---|
| `declared` | F baseline | `span.declared_size` (the `Tf` operand) |
| `first_glyph` | **status quo** | `span.rendered_size` (first glyph) == `span.size` |
| `mode` | alt (a) | modal `char.rendered_size` over the span's content glyphs (binned to 1e-3 pt; ties → bin nearest the median) |
| `median` | alt (b) | median `char.rendered_size` over content glyphs |
| `mean` | alt (c) | arithmetic mean over content glyphs (each character weight 1 — i.e. character-count weighted) |
| `first_nonws` | alt (d) | first content glyph's `char.rendered_size` |

"Content glyphs" = characters in the span with `not synthetic and not
isspace()` (empty → all rules fall back to `first_glyph`). Matching, rotation
handling, tolerance, and the whitespace/synthetic exclusion are copied verbatim
from the frozen probe, so all rules are scored on exactly the same matched
character set. The oracle span size and the F `declared` baseline are held fixed;
only the candidate span size varies across the five columns.

Environments: candidate `pdfspine==0.7.1` + `ocrspine-models==0.0.3` on CPython
3.12.11; oracle PyMuPDF 1.28.2 on CPython 3.12.11 (the archived G oracle
version). For the G corpus the archived oracle JSONL is reused; for the
independent corpus PyMuPDF 1.28.2 was run fresh. pdfspine `0.7.1` is the shipped
G code (geometry unchanged from `0.7.0`; the commits after it are CI/coverage/doc
only).

## Corpora

**G corpus** — the frozen 300-document manifest,
`conformance/corpus-diff/corpus.txt` (reconstructed from the archived run; SHA256
`1bb1548f351c7187363a82b30228b8de908bc542002684e7daeb92bb9ebbccc8`, matching the
size-parity report), first ≤20 pages/doc, 1,887 pages. Composition: 132
fintabnet, 100 robustness (govdocs1), 41 eurlex, 9 arabic, 6 born, 3 cjk, plus 9
fixtures.

**Independent corpus** — 25 documents the G span-size parity run **never used**,
all present on disk:

- **7 PMC** scientific articles (`corpus-pmc/PMC*.pdf`) — an entire category
  absent from `corpus.txt` (it appears only in the separate D/GT scoring slice).
- **18 held-out fintabnet** financial-table pages
  (`corpus-fintabnet/…`) — same producer family, documents disjoint from the
  132 G used.

GovInfo and additional CJK/typeset material would require a network fetch and
are not on disk; PMC supplies the genuinely-novel independent category. 87
sampled pages, 360,514 non-whitespace/non-synthetic characters, 360,390 uniquely
matched to the oracle.

## Per-variant three-count tables

**G corpus (300 docs, 1,887 pages, 5,803,856 matched characters).** Counts are
against the F `declared` baseline; `Δwithin` and `Δworse` are versus the status
quo (`first_glyph`).

| rule | improved | unchanged | worsened | within strict-tol | Δwithin | Δworse | strict-tol lost |
|---|---:|---:|---:|---:|---:|---:|---:|
| **first_glyph (status quo)** | 5,495,174 | 308,367 | **315** | 5,800,735 | — | — | 29 |
| mode | 5,495,331 | 308,380 | **145** | 5,802,023 | **+1,288** | −170 | 21 |
| median | 5,495,357 | 308,340 | 159 | 5,801,794 | +1,059 | −156 | 38 |
| mean | 5,495,372 | 308,334 | 150 | 5,796,169 | **−4,566** | −165 | 42 |
| first_nonws | 5,495,178 | 308,367 | 311 | 5,800,711 | −24 | −4 | 29 |

Reading the table:

- **mode** is the only rule that improves both axes: it removes 170 of the 315
  worsened characters and adds 1,288 characters to strict tolerance, while
  lowering strict-tolerance-lost from 29 to 21. Its cost to the 5.49 M improved
  is nil (it is +157).
- **median** cuts worsened to 159 but *raises* strict-tolerance-lost to 38 — the
  median can sit between two glyph sizes and just miss tolerance for characters
  the first-glyph rule matched exactly.
- **mean** cuts worsened to 150 but **regresses** within-tolerance by 4,566: the
  arithmetic mean of a multi-size span matches *no* actual glyph, so it pushes
  many characters just outside strict tolerance. Reject.
- **first_nonws** ≈ status quo: in these documents the first glyph is almost
  never a leading space, so the two rules coincide.

All deltas are ≤ 0.022 % of the 5.8 M matched characters.

**Independent corpus (25 docs, 87 pages, 360,390 matched characters).**

| rule | improved | unchanged | worsened | within strict-tol |
|---|---:|---:|---:|---:|
| first_glyph / mode / median / mean / first_nonws | 349,484 | 10,906 | **0** | **360,390 / 360,390** |

Every rule is identical and exact. The reason is structural, not luck: across
all 360,514 content characters, **0 lie in a multi-size span** (`first_glyph ==
mode` for every character), even though 96.97 % have `rendered_size ≠ declared`
(the text matrix scales the nominal `Tf`, uniformly within each span). Where a
span has one size, all five rules return that size and match the oracle. The
anchoring choice is therefore inert on clean born-digital text — the alternatives
carry **zero regression risk** there.

## Attribution of the 315 (2 documents / 41 spans)

All 315 worsened characters occur in two scanned/OCR robustness documents. Exact
distinct span counts (by pdfspine span `seq`) reproduce the report's 41:

| document | worsened chars (first_glyph) | distinct spans | pages touched | after mode |
|---|---:|---:|---|---:|
| govdocs1-00018.pdf | 126 | 30 | 0, 1, 2 | 83 |
| govdocs1-00053.pdf | 189 | 11 | 0,1,4,6,11,12,16,18,19 | 62 |
| **total** | **315** | **41** | | **145** |

Span characteristics:

- **Fonts:** Times-Roman (36 spans / 287 chars) and Helvetica (5 spans / 28
  chars). No other font.
- **Not leader dots.** 0 of 41 spans are dot-leader runs; the worsened
  characters are ordinary prose letters (`a o n t e i d r s …`). This is a
  different phenomenon from the F leader-dot fragmentation — it is per-glyph
  affine wobble in scanned text.
- **Transform magnitude.** Within an affected span the content-glyph
  `rendered_size` spread is min 0.000, median **0.247 pt**, max **0.700 pt**;
  distinct rendered sizes per span median 2, max 7. Each adjacent glyph pair
  stays within F's 5 % linear tolerance, so the seam is retained, but the sizes
  drift monotonically across the merged run.

Worked example (the report's canonical case), `govdocs1-00018.pdf` page 2, span
`the 1950's. Colin Gum`: `declared = 12.0`, first-glyph = `12.6996`, per-glyph
`rendered_size` steps `12.6996 → 12.413 → 12.238 → 12.0`. `Gum`'s oracle size is
`12.0`, which the `declared` baseline hits exactly and the first-glyph size
misses by 0.6996 pt (its 5.83 % relative error). No single span-level constant
can be right for both `the` (oracle ≈ 12.6996) and `Gum` (oracle 12.0) — that is
precisely why the alternatives trade one end of the span for the other and move
only ~half the characters. Full per-span records:
`…/anchor-experiment/affected_spans.json`.

## Why the alternatives only move scanned OCR text

The 315 worsened characters, and every character any alternative would change,
are exactly the characters F placed in a **multi-size span**. F retains a seam
whenever each *adjacent* glyph-pair's linear matrix change is ≤ 5 %; a monotone
drift (e.g. 13.44 → 12.84 → 12.48 → 12.0) therefore accumulates well beyond 5 %
across the whole span while every step is individually permitted. In a
correctly-split world each size would be its own span and *every* rule —
including the status quo — would equal the oracle, as the independent corpus
demonstrates (0 multi-size spans → 0 worsened).

So the measured "benefit" of mode/median/mean is **mitigation of the F
tolerance-accumulation artifact**, not an independent gain from better anchoring.
Per PRD-NEXT §0 item 4, this is reported, not acted on: **the F thresholds were
not tuned**, and no F-threshold change is proposed here.

## Impact on word / text projections

None, by construction. Every rule is computed from `char.rendered_size`, which
is identical across all five columns (it is the same rawdict), and would change
only the serialized `span.size` number. `get_text("text")` and
`get_text("words")` do not read `span.size`. The frozen G evidence already
proves F and G share identical `text_sha256` / `words_sha256` across all 1,887
pages and identical rawdict after removing only `span.size`; any alternative that
rewrites the same one field inherits that invariance. Selection, layout, word
assembly, HTML/XHTML/XML, and texttrace are likewise untouched. This experiment
changed no product code, so the shipped projections are literally unchanged.

## Recommendation

**Keep the status quo (first-glyph anchoring). Do not adopt an alternative.**

1. **Negligible, contained effect.** The best alternative moves 0.022 % of
   matched characters, all inside 2 of 300 documents, both scanned/OCR. The
   independent born-digital corpus shows every rule is bit-for-bit identical and
   already exact.
2. **The exact size is already available.** `char.rendered_size` is exact for
   all 315 characters and is the documented per-glyph API; consumers that need
   per-character sizes do not depend on the span anchor.
3. **The lever is elsewhere.** The residual disagreement is an F seam-merging
   effect; the anchor only chooses which end of a mis-merged run to honor. The
   PRD forbids tuning F thresholds without data, and the data here does not
   justify it (splitting these runs would fragment scanned prose into
   per-word/near-per-glyph spans — an accepted-cost tradeoff already discussed in
   the F report).
4. **Simplicity.** First-glyph is O(1) per span and already shipped, documented,
   and gated. `mode` needs an O(n) per-span tally and a tie-break policy for a
   0.02 % effect.

If a span-size change is nonetheless mandated to minimize the 315, the ranked
choice is **mode** (315 → 145 worsened, +1,288 within strict tolerance, no metric
regresses) → `median` (raises strict-tolerance-lost) → *stop*. `mean` regresses
coverage and `first_nonws` is indistinguishable from the status quo; neither
should be adopted. More data (an independent **scanned/OCR** corpus) would be
required before trusting that `mode`'s ~half reduction generalizes beyond these
two documents.

## Reproduce

Frozen-data reproduction of the 315 (stdlib only, no engines):

```sh
python conformance/probe_span_size_parity.py --reuse-workers \
  --corpus conformance/corpus-diff/corpus.txt \
  --baseline-python /usr/bin/python3 --candidate-python /usr/bin/python3 \
  --oracle-python /usr/bin/python3 \
  --work-dir /Volumes/ExternalSSD/pdfspine-archive/pdfspine-glyph-G-full-parity-fixed \
  --output /tmp/stage0.json --max-pages 20
```

Alternative-anchoring experiment (harness archived alongside the data):

```sh
# 1. candidate extraction with all five anchoring columns (pdfspine 0.7.1, py3.12)
python anchor_experiment.py --worker pdfspine \
  --corpus gcorpus.txt --output candidate.jsonl --max-pages 20
# 2. score every rule against the reused PyMuPDF 1.28.2 oracle
python anchor_experiment.py \
  --work-dir <dir-containing candidate.jsonl> \
  --oracle-jsonl <archived G oracle.jsonl> \
  --output gcorpus_result.json --origin-tolerance 0.01
# independent corpus: same two steps with indep_corpus.txt and a fresh
#   `--worker fitz` oracle extraction (PyMuPDF 1.28.2).
```

## Artifacts and harness location

No product code changed; the experiment is a standalone harness, kept out of the
worktree working tree. Durable copies (persistent SSD):
`/Volumes/ExternalSSD/pdfspine-archive/anchor-experiment/`

| file | contents |
|---|---|
| `anchor_experiment.py` | worker + compare harness (SHA256 `76c0090e…2bd1d6a`) |
| `span_attrib.py` | exact 41-span attribution |
| `indep_mix.py`, `summarize.py` | independent multi-size check; summary table |
| `gcorpus_result.json` | G-corpus per-variant counts (SHA256 `a7a91601…de75e980a`) |
| `indep_result.json` | independent-corpus per-variant counts (SHA256 `5797e976…1db5e0fd`) |
| `affected_spans.json` | the 41 affected spans with font/spread/text |
| `stage0_reproduce.json` | frozen-probe reproduction of the 315 |
| `gcorpus.txt`, `indep_corpus.txt` | the two corpus manifests |

The pdfspine (`.venv-anchor-pdfspine`) and PyMuPDF (`.venv-anchor-oracle`) venvs
and the large candidate/oracle JSONL under
`/Volumes/ExternalSSD/tmp/anchor-experiment/` are scratch and are removed at task
end; the result JSON above is sufficient to reprint every number here.
