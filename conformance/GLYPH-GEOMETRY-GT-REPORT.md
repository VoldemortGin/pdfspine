# Glyph geometry D — objective ground-truth non-regression

_Measured 2026-09-05. Current: `9912a2362a924cf976d81b8af84952731d5e0ad9`.
Baseline: `aaee2a9968bd31ae7e68cb86650071c3015002a8`._

This report records the D corpus gate for the glyph-geometry branch. All results
below are fresh measurements against objective `gt_text` using
`conformance/gt/run_gt.py`; no extractor text or PyMuPDF output is committed.

## Verdict

The born-digital, CJK, Arabic, 352-page EUR-Lex, and restored historical PMC
subsets pass. All 30 documents have exactly the same pdfspine metric
dictionaries and extracted-character counts on `9912a23` as on the
post-two-column-fix baseline `aaee2a9`. A second run from the provisional F
span-geometry wheel is also exact against the same pre-F results for all 30.

## Frozen comparison environments

| role | source commit | Python | installed wheel SHA-256 |
|---|---|---|---|
| baseline | `aaee2a9968bd31ae7e68cb86650071c3015002a8` | `/tmp/pdfspine-glyph-perf-env-base/bin/python` (3.12.11) | `30c915249e4141b1a44c882d64cf77fe2458c8704756f0e4f2a4deb627f05660` |
| current | `9912a2362a924cf976d81b8af84952731d5e0ad9` | `/tmp/pdfspine-glyph-perf-env-current/bin/python` (3.12.11) | `c0edbb0883130e2fd38c720bd0ea5ec29f6c30317af97e346d4628d1ffffb2f6` |

Both report pdfspine `0.6.1` / `VersionBind == 0.6.1` and were kept read-only
through the D runs. The initial born/CJK/Arabic and EUR-Lex orchestration used
the repository Python 3.12.11; the later restored-PMC and post-F paired runs
used `/usr/bin/python3` 3.9.6. `run_gt.py` launched the selected pdfspine wheel
and oracle in isolated subprocesses. Each comparison pair used the same
orchestrator and scoring code.

The historical comparison oracle is a separate
`.venv-oracle-gt12414`: Python 3.14.6, PyMuPDF 1.24.14 backed by MuPDF 1.24.11,
and pdfminer.six 20240706. The base and current runs use the same oracle, so the
Python 3.14 host difference cannot bias their comparison. The repository's
quad-only `.venv-oracle` remains unchanged at PyMuPDF/MuPDF 1.28.2 on Python
3.12.11.

## Born, CJK, and Arabic results

Cells are `mean / median` from the current `9912a23` pdfspine run.

| subset | docs | lev | f1 | jaccard | order | exact vs `aaee2a9` |
|---|---:|---:|---:|---:|---:|---|
| born | 6 | 0.9803 / 0.9909 | 0.9803 / 0.9909 | 0.9652 / 0.9818 | 1.0000 / 1.0000 | PASS |
| CJK | 3 | 0.8617 / 0.8390 | 0.8617 / 0.8390 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | PASS |
| Arabic | 9 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | PASS |
| PMC | 7 | 0.7243 / 0.7536 | 0.7808 / 0.7793 | 0.6101 / 0.5664 | 0.9391 / 0.9962 | PASS |

“Exact” compares, for every document, its unrounded pdfspine score dictionary
(`lev`, `f1`, `jaccard`, `order`) and extracted-character count, plus the
pdfspine aggregate. The canonical JSON projections have matching SHA-256 on
base and current:

| subset | base projection | current projection |
|---|---|---|
| born | `d51fbbb5fe94d95e935852d4995bc88310d40433976eeb2163dcaec94ed78f99` | `d51fbbb5fe94d95e935852d4995bc88310d40433976eeb2163dcaec94ed78f99` |
| CJK | `4eb687d4d4ae9acdf27f1e62684d7ee9dbcc8d4d5cdd39ba8b6f9dededc992c1` | `4eb687d4d4ae9acdf27f1e62684d7ee9dbcc8d4d5cdd39ba8b6f9dededc992c1` |
| Arabic | `ccfb0c05610044e45f1fbf433305d9224736b601bffb972c8f30aaedb024c631` | `ccfb0c05610044e45f1fbf433305d9224736b601bffb972c8f30aaedb024c631` |

The older handoff shorthand “born `1.0000 / 0.9803`” is not a mean/median
pair. It is `order mean = 1.0000` followed by `lev mean = 0.9803`. The current
measurement agrees with both values and the existing
`conformance/gt/GT-REPORT-born.md`.

## Input fingerprints

Each corpus fingerprint is SHA-256 over sorted lines of
`<basename> <file-sha256>\n`, covering the manifest and all scored PDFs.

| subset | manifest SHA-256 | PDFs | bytes (fingerprinted inputs) | corpus fingerprint |
|---|---|---:|---:|---|
| born | `283c9c47215a5ef6aa3fedc010c33512ee7bf809dd602cd1bdd51919a481d36b` | 6 | 256,388 | `4eb8e41db95334e4bc5e945444cc2b38e7d39ffe5bc2cef50d10ba04428e8587` |
| CJK | `c0aac423cbe8234422c748835f5a0bd6f2181f76cc6908f787fb03c906844ca3` | 3 | 206,659 | `da1e4c998328494c211f1efc28f354e91799421d00c76598b19e668cbf79d0f8` |
| Arabic | `d8f414bfb9a0563ae84cfba9fce7a0197cf6c14a9e12d2dee725cd34bd83adf3` | 9 | 119,768 | `f4c9ae462635f7b348aa88641ea691c0baa95c9bc3c42edfd0185878a36c9b89` |
| PMC | `3881f7fbdef6760dffa1ac5fe09781401c240284adce39b0666f24189d2eed1a` | 7 | 4,807,462 | `3971f9261bbf0322e83aedcfd83f2d50cebd705563906c56137bd52511143e11` |

The PMC fingerprint additionally covers all seven JATS `.nxml` ground-truth
files. The other three manifests carry their ground truth inline.

## Oracle stdout repair

PyMuPDF 1.28.2 prints a deprecation warning to stdout when imported through its
old `fitz` compatibility module. That prefix made `run_gt.py` reject the
otherwise valid oracle JSON as `oracle bad json`. `conformance/oracle_extract.py`
now imports the supported module as `import pymupdf as fitz`, preserving the
local alias while keeping stdout JSON-only. Direct extraction parses as JSON
with both PyMuPDF 1.24.14 and 1.28.2, and `run_gt.py --selftest` passes. The
initial warning-polluted runs were discarded.

## Commands

The three subsets were each run once per frozen wheel with this shape (the
`subset` and output names varied):

```console
.venv/bin/python conformance/gt/run_gt.py \
  --manifest "$PWD/conformance/gt/corpus-$subset/manifest.json" \
  --report "/tmp/pdfspine-gt-D-$revision/$subset.md" \
  --json "/tmp/pdfspine-gt-D-$revision/$subset.json" \
  --python "$frozen_python" \
  --oracle-python "$PWD/.venv-oracle-gt12414/bin/python" \
  --timeout 120
```

The content-free score JSON and generated reports remain under `/tmp`; the
public corpus assets remain gitignored.

## EUR-Lex and PMC public subsets

### EUR-Lex: five-document historical Greek slice

The public Cellar fetch completed with a 40-entry manifest, SHA-256
`5606ded075d7cf82e8527d737926eb961c9110d74044ef9cc7d2a7680a399eed`.
D did not run all 2,816 pages twice. It selected the five Greek documents that
appear in the historical 40-document report:

| document | pages | pdfspine lev | pdfspine order |
|---|---:|---:|---:|
| `32016R0679_EL.pdf` | 88 | 0.9691 | 0.9950 |
| `32011L0083_EL.pdf` | 25 | 0.9377 | 0.9881 |
| `32014R0596_EL.pdf` | 61 | 0.9443 | 0.9606 |
| `32006L0112_EL.pdf` | 118 | 0.8009 | 0.9869 |
| `32018R1725_EL.pdf` | 60 | 0.9741 | 0.9815 |

This slice is 5/40 documents and 352/2,816 pages (12.5% by either measure),
retains the corpus's non-Latin purpose, and covers all five CELEX families in
the historical report. Its PDF plus text assets total 8,694,761 bytes. The
sorted PDF/text input fingerprint is
`5aa7e1298d39e334f4be1f239d81728794cd0c87dc499c47809413a2766b850b`.

Current aggregate scores are `lev 0.9252 / 0.9443`, `f1 0.9670 / 0.9830`,
`jaccard 0.9381 / 0.9681`, and `order 0.9824 / 0.9869` (mean / median).
The canonical per-document pdfspine projection SHA-256 is
`711cb10daffaec03b8c3efab1a4e3e45b671a0cdbc0595f92f24e1d111161723`
on both `aaee2a9` and `9912a23`: exact non-regression.

The handoff's full-corpus numbers map to `lev mean / median =
0.9287 / 0.9486` and `order mean / median = 0.9811 / 0.9852`. They are a
different 40-document aggregate, so this five-document aggregate must not be
numerically compared to them. The same-input `aaee2a9` run is the acceptance
baseline. The selected documents' ground-truth character counts exactly match
the historical report, ruling out a changed-text explanation for their scores.

Selection used the new repeatable `--include-id` option. It matches manifest
`id`/`name` or PDF basename/stem and returns exit 1 before extraction if any
requested id is absent. With no option, the prior all-entry behavior is
unchanged. The self-test, an unknown-id CLI check, and a one-document real run
all pass.

### PMC: seven-document historical clean subset

The first recovery attempt found that both legacy locations for each
commercial/full CSV now return HTTP 404:

```text
/pub/pmc/deprecated/oa_comm_use_file_list.csv
/pub/pmc/deprecated/oa_file_list.csv
/pub/pmc/oa_file/oa_comm_use_file_list.csv
/pub/pmc/oa_file/oa_file_list.csv
```

The replacement fetch path uses the anonymous `pmc-oa-opendata` S3 bucket and
per-version metadata. It restored exactly the seven clean historical IDs:
`PMC176545`, `PMC176546`, `PMC193604`, `PMC193605`, `PMC212319`, `PMC212687`,
and `PMC212689`. Each resolved uniquely to article version `.1`, carries CC BY
metadata, and passed the metadata MD5 check for both PDF and JATS XML.

Fresh current scores are `lev 0.7243 / 0.7536`, `f1 0.7808 / 0.7793`,
`jaccard 0.6101 / 0.5664`, and `order 0.9391 / 0.9962` (mean / median). This
reproduces the handoff's rounded `order 0.939 / 0.996`. The base and current
per-document pdfspine score dictionaries, extracted-character counts, and
aggregate are exact for all seven documents.

## F prerequisite

The historical-slice scope of D is satisfied for all 30 measured documents.
The full 40-document EUR-Lex aggregate was deliberately not rerun; the five
historical Greek documents are the documented, input-fingerprinted slice.

The post-F run used Python 3.12.11 from
`/tmp/pdfspine-glyph-F-env-v2/bin/python`, installed from wheel SHA-256
`4f8426b4cddff4ca2938a1338b4826121320b2538e95de351e2b6c7011a241b5`.
The wheel includes the frozen adjacent-glyph span geometry gate plus the
concurrently reviewed serialization-only performance work. Against the pre-F
`9912a23` result JSON, every document's unrounded pdfspine metric dictionary and
extracted-character count, and every subset aggregate, are exactly equal for
born 6, CJK 3, Arabic 9, EUR-Lex 5, and PMC 7. Thus F changes span structure
without changing the flattened text or this objective GT score surface.

## Post-G text parity

The accepted G change publishes `dict`/`rawdict` span `size` from rendered
geometry while keeping the internal declared size unchanged. A direct frozen-F
versus isolated-G extraction compared the actual page text arrays for the same
30 documents: 0/30 differed across 439 pages and 1,877,060 characters. The
canonical per-document content projection SHA-256 is
`b458483dbb7562e4705fd4cb19c72e2a98fc2ab3d0e8c12be5e2935c75afd906`;
the content-free record is `/tmp/pdfspine-gt-G-text-parity.json`. Because the
ground truth and deterministic scoring function did not change, the exact F GT
scores above carry forward to G without rerunning the unchanged oracle text or
long sequence comparisons.

## Final shared-environment gate

After G landed, the shared Python 3.12.11 environment was rebuilt with Maturin
release mode. The installed module is
`python/pdfspine/_core.abi3.so`, 29,167,488 bytes, SHA-256
`26b9ee079e0eec73d3c7f42f7baee5dc7e03384019fa8944151ed6ae47ced029`.

| check | final result | measured wall time |
|---|---|---:|
| `cargo fmt --all -- --check` | clean | <1 s |
| workspace clippy, all targets/features, warnings denied | clean | 8 s |
| workspace Rust tests, all features | 1,702 passed, 0 failed, 1 explicit profiling test ignored | 151 s |
| `cargo deny check` | passed; configured dependency/license notices were informational | 3 s |
| `maturin develop --release --uv` | success | 22 s |
| Python tests with warnings as errors and doctests | 805 passed, 0 failed, 63 skipped | 5 s |
| Ruff format/check, mypy | clean; mypy checked 10 source files | <1 s |
| four repository drift/manifest guards | passed | <1 s |

The first Maturin invocation exited before building because inherited
`CONDA_PREFIX` and the intended `VIRTUAL_ENV` were both set. Unsetting
`CONDA_PREFIX` made the recorded release build succeed. The first plain Python
run also exposed three test-only oracle subprocesses that still imported the
deprecated `fitz` name; PyMuPDF 1.28.2 prefixed their stdout, causing one string
and two JSON assertions to fail (794 passed, 63 skipped, 3 failed). Changing
only those three subprocess imports to `import pymupdf as fitz` preserved their
assertions. The plain retry passed 797/63, and the final warnings-error/doctest
gate passed 805/63. No failure was skipped. Complete logs are under
`/tmp/pdfspine-final-gates-20260905/`.
