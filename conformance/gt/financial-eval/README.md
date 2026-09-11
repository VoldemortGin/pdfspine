# Financial table draft candidate v1

This is a frozen, **unreviewed annotation-derived** candidate: 40 pages / 60 tables,
with 10 development pages / 15 tables and 30 evaluation pages / 45 tables. All
40 issuers are distinct across both partitions. It does not replace the existing
source-annotation scoring input, establish reviewed HTML gold, or satisfy human
acceptance. Machine integrity checks are not a human or AI visual review.

All 150 source pages / 186 tables have been used in historical benchmarks. The
30-page evaluation partition is **not an untouched holdout**. Three known pages
previously used for targeted tuning (ADBE_2011_page_118, ADI_2010_page_51,
AMP_2015_page_94) are fixed in development. A false `prior_targeted_tuning` only
means that a page is not one of those three known cases.

## Selection and provenance

`select.py` reads only source annotations, PDF bytes for hashes, and the source
manifest; it does not read predictions, failure scores, or model outputs.
`v1/selection.json` freezes all 150 page identities, source hashes, features,
seed, minimum targets, and selected order. Development is chosen first. Each
subsequent candidate must have an unused issuer, is ranked by the sum of integer
normalized outstanding feature deficits (`deficit * 2520 // target`), then by
SHA-256 of seed, partition, and document ID separated by NUL, then document ID.
Feature targets and achieved counts are reproducible from that file. This is a
feature-enriched stress subset, not a population-representative random sample.

Features cover multiple tables, row/column spans, multi-row annotated column
headers, currency symbols, parenthesized numbers, blank cells, tables with at
least 30 rows, and disagreement between the two source text fields. These
annotations do not establish whether a table is visually borderless.

Original PDF and annotation licenses are separate: PDF assets are identified as
**CDLA-Permissive-1.0**, corrected FinTabNet.c annotations as
**CDLA-Permissive-2.0**, as recorded in the original source manifest. The URLs,
original decommissioned PDF source, source IDs, original train/validation/test
split, table index and source line are retained in the new manifest/sidecars.
This candidate does not relicense those source assets. Derived HTML and cell
sidecars retain the annotation provenance; no PDF or raw annotation archive is
redistributed here. SHA-256 binds the cached inputs; it is not independent
certification of an upstream download or historical legal provenance.

## Text, HTML and review contract

Each cell sidecar preserves the complete original cell, including raw index
order, both text keys (and their presence), geometry, and annotation flags. The
JSON text key wins **when present, including an empty string**; PDF text is used
only when that key is absent. Normalization is Unicode NFC plus whitespace
collapse/strip. Currency, signs, parentheses and decimal punctuation are not
parsed or rewritten. Empty text geometry arrays are retained as missing text
geometry; nonempty geometry must contain four finite numbers.

Contiguous span indices are sorted only for derived placement. For example,
AMP's original `[8, 7]` is retained and canonically placed as columns 7–8.
Duplicates, noncontiguous spans, overlapping cells and grid holes fail generation.
HTML escapes text, emits explicit empty cells, and uses one `tbody`; only a
source column-header flag produces `th`. Projected row headers are preserved as
a data attribute. No `thead` or inferred header partition is invented.

`v1/manifest.json` references the original annotation JSON as the diagnostic
scoring input. Each selected table also references its HTML and cells sidecar
with hashes. `review-ledger.json` binds the selection hash and every table's PDF,
annotation, HTML and sidecar hashes. Human review is `unreviewed`, with null
reviewer/date and no corrections; AI visual review is `not_performed`.
Machine topology/hash validation is recorded independently. This immutable v1
schema never promotes itself to reviewed gold: `verify --acceptance` fails.
A later reviewed revision needs an explicit reviewed-data contract and retained
review history, rather than relabeling or regenerating this candidate.

## Reproduce or verify without a model

Use Python 3.12+ from the repository root. Generation and full `verify` require
the existing full `corpus-fintabnet` cache (150 PDFs, 150 annotation JSONs and
its original manifest). `verify-selected` and evaluation harness consumption
require only the 40 selected PDFs, their 40 annotation JSONs, and all 123
candidate files; unselected source files and the old source manifest are not
required for that frozen-dataset check.
There are no downloads, model inference, renderer calls or network requests in
this script. Asset paths in the candidate are relative to `v1/`; a normal cache
lives at `conformance/gt/corpus-fintabnet`. Large cached inputs are gitignored and
must be provided in a fresh checkout.

```sh
python conformance/gt/financial-eval/select.py verify conformance/gt/financial-eval/v1
# Validate only the frozen selected dataset for harness consumption:
python conformance/gt/financial-eval/select.py verify-selected conformance/gt/financial-eval/v1
# Or bind the same hashed assets from another cache location:
python conformance/gt/financial-eval/select.py verify conformance/gt/financial-eval/v1 \
  --corpus-root '/path with spaces/corpus-fintabnet'
# Generate into a NEW directory, then compare it to v1:
python conformance/gt/financial-eval/select.py generate /tmp/new-financial-draft \
  --corpus-root '/path with spaces/corpus-fintabnet'
python conformance/gt/financial-eval/test_select.py
```

Generation refuses an existing output directory **before writing anything**.
Both verification modes are read-only and check every selected derived hash,
selection order from frozen features, and HTML derivation. Full `verify` also
checks every declared source asset in the 150-page pool and recomputes all
features. `verify-selected` checks only selected source assets, so an absent
unselected PDF does not make the 40-page evaluation incomplete. Missing or
changed assets required by the chosen mode fail that whole verification; no denominator reduction or fallback download is allowed.
For full-pool reproduction, the source manifest's original byte hash is also
frozen, so preserve that file
rather than rewriting its historical absolute paths. The generator rebinds
asset paths using the provided cache root; it never follows those old absolute
paths. A missing cache is an explicit setup failure.

If a reviewer edits HTML or the ledger, verification reports the mismatch and
leaves those edits intact. Do not overwrite that directory to make the check
pass. Generate elsewhere for comparison and preserve review work in a new
version. Outputs generated at an external directory still use the canonical
relative cache references, so pass `--corpus-root` when verifying there.
