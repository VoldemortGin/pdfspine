# Glyph geometry performance report

Measured on 2026-09-05. **The geometry publication has a material rawdict cost.**
On the specified 118-page EUR-Lex document, the original geometry build increased
streamed rawdict elapsed time by 85.9% and retained rawdict peak RSS by 141.6%.
Two output-preserving Python serialization optimizations reduce retained rawdict
RSS from approximately 699 MiB to 430 MiB. Against the same-window pre-geometry
baseline, the optimized build still costs **59.0% more streamed rawdict time**,
**65.3% more retained rawdict time**, and **48.9% more retained rawdict RSS**.
This is measurement and mitigation evidence, not a zero-regression performance pass.

All geometry is still carried unconditionally. No field, native Python value
type, text flag, layout rule, or size semantic was removed or changed.

## Input, builds, and machine

- Input: `conformance/gt/corpus-eurlex/32006L0112_EN.pdf`, Council Directive
  2006/112/EC, fetched by the existing `fetch_eurlex.py` Cellar route.
  **118 pages, 486,142 bytes**; this individual document is not 2,816 pages.
- SHA-256: `f73bd86fde543c4d36677b971890c30bf6750fe2f9c4dab166fb75176ec5be8a`.
- The extracted stream has 380,721 text characters, 10,999 spans, and 370,347
  rawdict character entries, including 58,202 synthetic spaces. GT reference
  text has 355,678 characters; these counts do not assert extraction accuracy.
- Pre-geometry baseline: `aaee2a9968bd31ae7e68cb86650071c3015002a8`, which
  includes the two-column correlation-table fix and is an ancestor of
  `9912a2362a924cf976d81b8af84952731d5e0ad9`. The next source-changing commit
  after the baseline is the glyph-rendering work, so this comparison does not
  accidentally include the earlier two-column fix.
- Original geometry build: `9912a2362a924cf976d81b8af84952731d5e0ad9`.
  Timed optimized build: that commit plus the rawdict key and float sharing change
  in `crates/py-bindings/src/lib.rs`, source SHA-256
  `ea4829bf89ffdf9abcb2f8db5546d29c5146b871e245eac0ae4cd6c50eb6db17`.
  Later span/layout changes are outside these measurements.
  A subsequent review added a defensive NaN bypass to float sharing: NaNs are
  boxed independently because Python container equality may shortcut on object
  identity. The finite-coordinate corpus does not exercise that branch, but
  the branch itself was not included in the formal timing windows. The final
  reviewed source SHA-256 is
  `b770625996c1d7802bb561fcff0a85e5fad5a2ff0756ec6594de7e69fe799c90`;
  it was rebuilt, passed the same Clippy gate and 35 relevant Python tests.
- Hardware: Apple M4 Max, 16 logical CPUs, 128 GiB RAM, arm64; macOS 26.5.1
  (25F80). Python 3.12.11, Rust 1.96.0 (`ac68faa20`), maturin 1.12.6,
  PyO3 0.29.0, pdfspine 0.6.1.
- Both baseline and geometry wheels used `maturin build --release --locked`
  and the unchanged pyproject features (`pyo3/abi3-py311`, `ocr`), default
  `MACOSX_DEPLOYMENT_TARGET=11.0`. `Cargo.lock`, Cargo manifests, and build
  configuration are identical between the two commits. Lockfile SHA-256:
  `c872be048edc18ade55c5109f0ca18d1eedf0b590dd0af3c8fb37138ced2d112`.
- Baseline and current used detached `/tmp/pdfspine-glyph-perf-aaee2a9` and
  `/tmp/pdfspine-glyph-perf-9912a23` worktrees and independent target directories
  and environments. An initial attempt to share a target directory reused stale
  baseline metadata and failed compilation; it produced no current wheel or
  measurements. A fresh current target build succeeded. The optimization
  worktree `/tmp/pdfspine-glyph-perf-optimized` starts at the same current commit.
  No shared checkout or `.venv` was switched or rebuilt for these experiments.
- Original installed environments remain frozen at
  `/tmp/pdfspine-glyph-perf-env-base` and `/tmp/pdfspine-glyph-perf-env-current`;
  candidate environments are `/tmp/pdfspine-glyph-perf-env-keys` and
  `/tmp/pdfspine-glyph-perf-env-optimized`.

Wheel SHA-256 values:

| Build | SHA-256 |
|---|---|
| Pre-geometry | `30c915249e4141b1a44c882d64cf77fe2458c8704756f0e4f2a4deb627f05660` |
| Original geometry | `c0edbb0883130e2fd38c720bd0ea5ec29f6c30317af97e346d4628d1ffffb2f6` |
| Key sharing | `bd009b67a3678cfe46f88d0d9c0dc08e8db096c427b5a1490e460e37fe714e3b` |
| Key + float sharing | `b1221ae7430bf8aac9a1ac6ae435769a9b30c269349c03faecd5e1a59c15386e` |
| Reviewed patch including NaN bypass (not formally timed) | `b3e4d5fa533cca966d4d4ec892ee66bbbcc65be3efc99f0673cb11605c549d0f` |

## Method

`conformance/bench_glyph_geometry.py` runs a fresh Python process per sample.
Import occurs before timing; the timer covers opening the PDF, extracting all
118 pages with default flags, and closing the document. Peak RSS is the entire
process high-water mark (`getrusage(RUSAGE_SELF).ru_maxrss`; Darwin bytes), so it
includes Python/native imports and temporary native extraction objects.

In **stream** mode each page result is deleted before the next extraction; the
measured time includes that release. In **retain** mode all page results remain
alive until after timing and RSS capture; releasing the retained result list
is deliberately excluded. Each process still builds a fresh document and page
model: this is not repeated extraction from a cached TextPage.

Every mode/policy/build has one discarded warmup followed by seven samples.
Build order alternates AB/BA between paired repetitions. C/D corpus/GT agents
paused their active work in each measurement window; ordinary desktop/OS
background activity was not stopped. These are warm filesystem-cache results
on one machine and one PDF, not cold-disk or exhaustive workload claims.

## Three build snapshots

Medians below give a compact view of the initial baseline and original geometry
window (A) and the final optimization window (D). **A and D are different
windows; use the following same-window delta tables for quantitative comparisons.**
Each cell is elapsed milliseconds / peak RSS MiB.

| Policy / output | Pre-geometry (A) | Original geometry (A) | Optimized geometry (D) |
|---|---:|---:|---:|
| stream / text | 99.80 / 38.20 | 105.51 / 41.53 | 103.00 / 41.52 |
| stream / dict | 106.77 / 38.47 | 122.31 / 42.23 | 119.12 / 42.31 |
| stream / rawdict | 205.97 / 43.88 | 382.86 / 55.27 | 320.13 / 50.00 |
| retain / text | 99.76 / 38.81 | 107.46 / 42.09 | 102.91 / 42.06 |
| retain / dict | 108.94 / 63.16 | 129.41 / 90.70 | 126.95 / 90.81 |
| retain / rawdict | 226.33 / 289.17 | 493.99 / 698.67 | 370.31 / 430.41 |

### Initial geometry vs pre-geometry (window A)

Window started 2026-09-05T08:17:04Z. Seven samples per cell; reported deltas use medians.

| Policy / output | Before ms | After ms | Δ ms (%) | Before MiB | After MiB | Δ MiB (%) |
|---|---:|---:|---:|---:|---:|---:|
| stream / text | 99.80 | 105.51 | +5.71 (+5.72%) | 38.20 | 41.53 | +3.33 (+8.71%) |
| stream / dict | 106.77 | 122.31 | +15.54 (+14.55%) | 38.47 | 42.23 | +3.77 (+9.79%) |
| stream / rawdict | 205.97 | 382.86 | +176.90 (+85.88%) | 43.88 | 55.27 | +11.39 (+25.96%) |
| retain / text | 99.76 | 107.46 | +7.71 (+7.73%) | 38.81 | 42.09 | +3.28 (+8.45%) |
| retain / dict | 108.94 | 129.41 | +20.47 (+18.79%) | 63.16 | 90.70 | +27.55 (+43.62%) |
| retain / rawdict | 226.33 | 493.99 | +267.66 (+118.26%) | 289.17 | 698.67 | +409.50 (+141.61%) |

### Key sharing vs original geometry (window B)

Window started 2026-09-05T08:23:16Z. Seven samples per cell; reported deltas use medians.

| Policy / output | Before ms | After ms | Δ ms (%) | Before MiB | After MiB | Δ MiB (%) |
|---|---:|---:|---:|---:|---:|---:|
| stream / rawdict | 374.83 | 317.72 | -57.11 (-15.24%) | 55.23 | 52.70 | -2.53 (-4.58%) |
| retain / rawdict | 489.79 | 372.06 | -117.73 (-24.04%) | 698.66 | 568.23 | -130.42 (-18.67%) |

### Float sharing vs key sharing (window C)

Window started 2026-09-05T08:27:34Z. Seven samples per cell; reported deltas use medians.

| Policy / output | Before ms | After ms | Δ ms (%) | Before MiB | After MiB | Δ MiB (%) |
|---|---:|---:|---:|---:|---:|---:|
| stream / rawdict | 315.16 | 315.29 | +0.14 (+0.04%) | 52.64 | 50.09 | -2.55 (-4.84%) |
| retain / rawdict | 372.35 | 368.30 | -4.05 (-1.09%) | 568.39 | 430.42 | -137.97 (-24.27%) |

### Final optimized geometry vs pre-geometry (window D)

Window started 2026-09-05T08:27:48Z. Seven samples per cell; reported deltas use medians.

| Policy / output | Before ms | After ms | Δ ms (%) | Before MiB | After MiB | Δ MiB (%) |
|---|---:|---:|---:|---:|---:|---:|
| stream / text | 96.06 | 103.00 | +6.94 (+7.23%) | 38.16 | 41.52 | +3.36 (+8.80%) |
| stream / dict | 104.49 | 119.12 | +14.63 (+14.00%) | 38.58 | 42.31 | +3.73 (+9.68%) |
| stream / rawdict | 201.34 | 320.13 | +118.78 (+59.00%) | 43.86 | 50.00 | +6.14 (+14.00%) |
| retain / text | 96.99 | 102.91 | +5.92 (+6.10%) | 38.73 | 42.06 | +3.33 (+8.59%) |
| retain / dict | 106.71 | 126.95 | +20.23 (+18.96%) | 63.11 | 90.81 | +27.70 (+43.90%) |
| retain / rawdict | 224.03 | 370.31 | +146.28 (+65.30%) | 289.14 | 430.41 | +141.27 (+48.86%) |

## Attribution and limits

The plain-text path still carries the enlarged Rust geometry structures, while
its streaming process RSS grows by about 3.4 MiB and elapsed time by about 7%.
That is evidence against a universal doubling of extraction memory: per-page
temporary structures are released, and document glyph counts cannot simply be
multiplied by the struct-size increment to predict peak RSS.

The original retained rawdict increase is much larger: +409.5 MiB, compared with
+11.4 MiB when results are discarded page by page. This contrast identifies
retained Python output objects as a major contributor. Source inspection shows
that the bridge constructs separate tuples and boxes their f64 values, and
originally converts every string key anew for every character. A first-page
probe over 100 characters found 100 different Python objects for each of the
seven multi-character keys; their `sys.getsizeof` sizes total 332 bytes per
character. The one-character `c` key is already reused by CPython. One matrix
costs 88 bytes for its tuple plus six 24-byte Python floats; one quad costs
104 bytes plus eight 24-byte floats. Allocator size classes add overhead beyond
these logical object sizes. Every synthetic-space entry also carries the keys
and geometry, so counting only painted glyphs understates output cost.

Two measured changes preserve all public keys and value types:

1. The rawdict branch interns its eight key strings once per span conversion
   and retains `Bound<'py, PyString>` references only for that call. No Rust
   static, process-global Python object, or cross-interpreter cache is added.
   The paired experiment lowers rawdict time by 15.2% streaming / 24.0%
   retaining, and retained RSS by 130.4 MiB.
2. A small bounded cache within each character shares immutable Python floats
   when their `f64::to_bits()` values match across origin, bbox, matrix, quad,
   and rendered size. It distinguishes signed zero, makes no same-span matrix
   assumption, and never shares mutable dictionaries or lists. The paired
   experiment saves a further 138.0 MiB retained RSS. Streaming time changes
   by +0.04%, within noise; the ~1.1% retained-time reduction is also too small
   relative to sample variation to claim a speed improvement. This candidate
   is retained for its clear memory reduction without a measurable slowdown.

There is an irreducible representational increase under the present API:
additional dictionary entries and geometry tuples must exist for every raw
character entry. This does **not** establish that all remaining time or RSS
is unavoidable. No allocation-stack profiler or native-only breakdown was run;
the residual contribution of Rust structure copies, Python dictionary growth,
tuple allocation, boxing, and allocator slack is not fully apportioned.
Possible follow-up experiments include reusing immutable span metadata and
reducing intermediate TextPage-to-TextDict copies, with exact-value verification
and fresh measurements. They are not part of this implementation.

The original rawdict regression was severe for applications retaining whole
documents, and the optimized +59% streaming time / +49% retained memory remain
material. The unconditional-geometry decision is preserved; this report does
not manufacture a performance pass or introduce a textflags opt-out.

## Verification

- `cargo clippy -p py-bindings --release --all-features -- -D warnings`: passed.
- `cargo fmt --all`, `git diff --check`, and Ruff on the benchmark and new
  Python tests: passed.
- Isolated candidate: 34 relevant Python tests passed. After safely applying
  the same Rust source and the new test file to the shared tree, 35 passed
  (the shared tree also contains the user's newer quad probe test).
- New serialization tests cover heterogeneous scale/shear matrices, singular
  matrices, per-character numeric values, tuple/float/int/bool types, independent
  mutable outputs, and native dict/rawdict equality with the independently
  implemented Rust JSON serializer at its documented decimal precision.
- An additional all-118-page check compared length-framed canonical JSON
  payload SHA-256 values for text/dict/rawdict before and after optimization.
  Every field and float value, including signed zero in JSON representation,
  was preserved exactly. Tuple/float types were separately checked for all
  geometry fields; character dictionaries within spans remained distinct.
  Pre-geometry/current payloads also match exactly after removing only the
  newly published geometry/order keys. The all-page check is local evidence,
  distinct from the GT accuracy work and from the new permanent unit tests.

Exact all-page payload hashes, original geometry = optimized geometry:

| Output | SHA-256 |
|---|---|
| text | `a7ced42acc3617b7c15a0018586b52abbcd1472b78bcb233253bbb6e3db6375f` |
| dict | `fde708095284436a02038f26ee48890d2054d8f3aba5d1d203627dd0bb6e6f65` |
| rawdict | `43d4851805043bd80c9b997d0b91791d4dd4ae0ea4ddfa29f48bee5d89edf46b` |

## Reproduction and durable samples

Build each revision in an isolated worktree using the same release options:

```sh
git worktree add --detach /tmp/pdfspine-perf-base aaee2a9
git worktree add --detach /tmp/pdfspine-perf-current 9912a23
uv venv /tmp/pdfspine-perf-base-env --python python3.12
uv venv /tmp/pdfspine-perf-current-env --python python3.12
# Within each worktree, use a DIFFERENT target and wheel directory:
(cd /tmp/pdfspine-perf-base && env PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_DIR=/tmp/pdfspine-perf-base-target maturin build --release --locked --interpreter /tmp/pdfspine-perf-base-env/bin/python --out /tmp/pdfspine-perf-base-wheels)
uv pip install --python /tmp/pdfspine-perf-base-env/bin/python --no-deps /tmp/pdfspine-perf-base-wheels/pdfspine-0.6.1-cp311-abi3-macosx_11_0_arm64.whl
# Repeat for current, then build the optimization patch in its own worktree/env.
python conformance/bench_glyph_geometry.py \
  --pdf /absolute/path/32006L0112_EN.pdf \
  --baseline-python /tmp/pdfspine-perf-base-env/bin/python \
  --current-python /tmp/pdfspine-perf-current-env/bin/python \
  --runs 7 --output /tmp/glyph-performance.json
```

The full machine-readable samples from this session remain locally under
`/tmp/pdfspine-glyph-performance-{results,keys,floats,final}.json`; the benchmark
writes fresh metadata, warmups, execution order, elapsed/RSS samples, median,
range and median absolute deviation (MAD). They are not required for reading
this report: all timed sample values are preserved below.

No outlier was removed. For example, final retained rawdict has one 387.5 ms
sample, while its median is 370.3 ms and MAD is 0.73 ms. Original rawdict
time/RSS changes and both candidate memory savings are much larger than their
within-cell noise. Seven repetitions on one PDF do not establish behavior on
every document, nor are inter-window differences treated as paired estimates.


### Samples: results

| Policy / output / build | Elapsed ms (7 samples) | Peak RSS MiB (7 samples) |
|---|---|---|
| stream / text / baseline | 99.414, 99.797, 102.057, 100.423, 98.987, 96.745, 107.808 | 38.172, 38.203, 38.234, 38.250, 38.109, 38.172, 39.734 |
| stream / text / current | 104.855, 105.159, 105.509, 105.109, 106.733, 105.994, 108.674 | 41.531, 41.500, 41.516, 41.531, 41.641, 41.453, 44.359 |
| stream / dict / baseline | 106.868, 106.664, 106.752, 107.664, 106.769, 107.720, 105.226 | 38.641, 38.469, 38.172, 38.406, 38.609, 38.594, 38.266 |
| stream / dict / current | 123.149, 121.308, 127.954, 122.305, 131.692, 121.386, 121.765 | 42.172, 42.141, 45.109, 42.219, 42.234, 42.344, 42.344 |
| stream / rawdict / baseline | 206.212, 207.110, 204.947, 203.220, 206.930, 205.047, 205.968 | 43.875, 44.016, 43.797, 43.781, 43.859, 43.969, 43.984 |
| stream / rawdict / current | 389.373, 380.498, 386.146, 383.232, 382.863, 381.789, 381.753 | 55.328, 55.312, 55.125, 55.219, 55.266, 55.156, 55.266 |
| retain / text / baseline | 100.556, 99.325, 99.756, 98.899, 100.366, 96.266, 99.813 | 38.406, 38.844, 38.828, 38.828, 38.438, 38.578, 38.812 |
| retain / text / current | 105.446, 106.146, 107.463, 116.507, 109.414, 108.246, 106.467 | 41.984, 42.094, 42.203, 44.828, 41.906, 41.891, 42.219 |
| retain / dict / baseline | 110.632, 108.935, 111.188, 108.445, 106.793, 109.101, 107.557 | 63.234, 63.156, 62.938, 63.047, 63.250, 63.344, 63.141 |
| retain / dict / current | 132.795, 129.409, 133.750, 129.491, 126.736, 129.255, 128.267 | 90.703, 90.578, 90.781, 90.609, 90.781, 90.703, 90.859 |
| retain / rawdict / baseline | 223.972, 224.054, 228.178, 226.327, 225.849, 228.320, 227.784 | 289.203, 291.203, 289.172, 289.453, 289.031, 289.125, 289.047 |
| retain / rawdict / current | 491.040, 493.526, 495.306, 493.991, 492.656, 495.130, 494.490 | 698.500, 698.672, 698.578, 698.547, 698.781, 698.688, 698.703 |

### Samples: keys

| Policy / output / build | Elapsed ms (7 samples) | Peak RSS MiB (7 samples) |
|---|---|---|
| stream / rawdict / baseline | 374.239, 374.826, 375.895, 374.003, 376.823, 375.569, 373.946 | 55.234, 55.312, 55.234, 55.219, 55.250, 55.266, 55.078 |
| stream / rawdict / current | 318.309, 317.081, 318.836, 315.567, 317.717, 316.993, 319.422 | 52.703, 52.562, 52.500, 54.047, 52.750, 52.656, 53.516 |
| retain / rawdict / baseline | 485.069, 490.466, 488.657, 491.215, 487.317, 491.181, 489.790 | 698.656, 702.781, 698.625, 698.719, 698.844, 698.594, 698.625 |
| retain / rawdict / current | 370.905, 370.471, 372.932, 371.369, 372.783, 372.064, 372.194 | 568.234, 568.188, 568.016, 568.391, 568.438, 568.203, 571.312 |

### Samples: floats

| Policy / output / build | Elapsed ms (7 samples) | Peak RSS MiB (7 samples) |
|---|---|---|
| stream / rawdict / baseline | 315.020, 314.989, 317.392, 315.297, 319.964, 315.155, 313.184 | 52.406, 52.797, 52.641, 56.297, 52.672, 52.562, 52.516 |
| stream / rawdict / current | 316.284, 315.290, 315.020, 319.178, 312.957, 314.308, 316.487 | 49.734, 49.766, 50.141, 50.000, 50.141, 50.094, 50.094 |
| retain / rawdict / baseline | 369.626, 368.926, 373.876, 370.466, 375.280, 376.800, 372.350 | 568.250, 569.688, 568.391, 568.438, 568.328, 568.422, 568.312 |
| retain / rawdict / current | 364.385, 366.197, 368.583, 368.127, 368.297, 371.818, 371.179 | 430.422, 430.438, 430.516, 430.406, 430.438, 430.406, 430.422 |

### Samples: final

| Policy / output / build | Elapsed ms (7 samples) | Peak RSS MiB (7 samples) |
|---|---|---|
| stream / text / baseline | 97.868, 96.203, 93.814, 93.225, 97.030, 95.946, 96.062 | 38.188, 37.953, 38.078, 38.219, 38.172, 38.062, 38.156 |
| stream / text / current | 102.219, 102.977, 104.180, 103.111, 102.687, 104.076, 103.004 | 41.516, 41.594, 41.578, 41.531, 41.391, 41.516, 41.453 |
| stream / dict / baseline | 104.208, 104.746, 104.738, 104.166, 101.733, 104.488, 104.726 | 38.609, 38.578, 38.500, 38.578, 38.469, 38.656, 38.422 |
| stream / dict / current | 120.817, 119.119, 119.112, 119.249, 119.595, 118.486, 118.857 | 42.359, 42.297, 42.312, 42.328, 42.297, 42.297, 42.328 |
| stream / rawdict / baseline | 199.883, 199.671, 202.442, 201.343, 202.977, 200.754, 203.776 | 43.812, 43.828, 44.047, 44.094, 43.859, 43.781, 44.031 |
| stream / rawdict / current | 321.244, 320.126, 318.748, 321.076, 320.746, 316.626, 317.320 | 50.000, 50.016, 49.953, 50.078, 49.953, 49.938, 50.047 |
| retain / text / baseline | 97.299, 96.791, 96.989, 96.676, 97.966, 94.060, 97.240 | 38.812, 38.734, 38.703, 38.750, 38.828, 38.656, 38.688 |
| retain / text / current | 103.290, 103.355, 102.298, 103.051, 102.327, 102.299, 102.905 | 42.047, 45.125, 42.062, 41.969, 42.094, 42.000, 42.156 |
| retain / dict / baseline | 106.731, 106.165, 104.993, 108.325, 106.754, 103.915, 106.713 | 63.172, 63.047, 62.844, 63.109, 62.875, 63.172, 63.125 |
| retain / dict / current | 127.249, 126.973, 126.926, 126.946, 127.567, 126.771, 126.780 | 90.812, 90.859, 90.812, 90.688, 94.125, 90.844, 90.766 |
| retain / rawdict / baseline | 226.049, 224.349, 223.983, 225.620, 224.028, 222.888, 221.640 | 289.406, 289.141, 289.156, 289.047, 289.484, 288.906, 288.828 |
| retain / rawdict / current | 370.895, 365.527, 370.310, 368.259, 369.583, 370.413, 387.480 | 430.406, 430.453, 430.500, 430.141, 430.312, 430.344, 433.172 |

## Final E/F/G regression gate (after the unified test gate)

At 2026-09-05 09:17:53 UTC, with C/D heavy work paused, the same EUR-Lex
118-page input was measured in three fresh-process AB/BA pairs per mode/policy,
with one discarded warmup per build. This short gate compares the frozen
optimized E environment (including the final NaN guard) with the final shared
E/F/G build. It checks for a new large regression; it does not replace the
seven-pair pre-geometry comparison or establish zero cost.

- E baseline: `/tmp/pdfspine-glyph-perf-env-optimized/bin/python`, final guarded
  wheel SHA256 `b3e4d5fa533cca966d4d4ec892ee66bbbcc65be3efc99f0673cb11605c549d0f`.
- Final candidate: `/Users/linhan/startup/spine/pdfspine/.venv/bin/python`,
  Python 3.12.11, release module `python/pdfspine/_core.abi3.so`, 29,167,488 bytes,
  SHA256 `26b9ee079e0eec73d3c7f42f7baee5dc7e03384019fa8944151ed6ae47ced029`.
  The unified Rust and Python gates had finished before timing began.

| Policy / output | E median ms | E/F/G median ms | Elapsed delta | E RSS MiB | E/F/G RSS MiB | RSS delta |
|---|---:|---:|---:|---:|---:|---:|
| stream / text | 102.076 | 102.528 | +0.44% | 41.297 | 41.688 | +0.95% |
| stream / dict | 118.140 | 120.575 | +2.06% | 42.156 | 42.359 | +0.48% |
| stream / rawdict | 312.097 | 312.691 | +0.19% | 50.156 | 51.188 | +2.06% |
| retain / text | 101.718 | 105.020 | +3.25% | 42.016 | 42.031 | +0.04% |
| retain / dict | 125.463 | 128.933 | +2.77% | 90.688 | 90.969 | +0.31% |
| retain / rawdict | 366.214 | 368.964 | +0.75% | 430.453 | 430.750 | +0.07% |

No additional large regression appears in this gate: elapsed medians change by
+0.19% to +3.25%, rawdict retained RSS by +0.30 MiB, and stream rawdict RSS by
+1.03 MiB. Three pairs cannot distinguish every small implementation cost from
scheduling/allocator variation; for example, one E stream-rawdict RSS sample was
53.17 MiB while its other two were about 50 MiB. Do not interpret the smallest
deltas as proven improvements or regressions. The final candidate includes the
NaN guard, but this comparison does not isolate its cost because both sides
already include it.

The material cost versus pre-geometry remains: the earlier seven-pair same-window
optimized comparison measured +59% rawdict stream time and +48.86% retained
rawdict RSS (+65.3% retained elapsed). The final gate does not erase those
findings and does not justify an all-green performance claim.

Reproduce with `bench_glyph_geometry.py --runs 3` and the two interpreter paths
above. Raw artifact: `/tmp/pdfspine-glyph-performance-final-EFG.json`, SHA256
`e543c5e665d9e925bdc0666df3e37eaa90b9c9eefdc161719aa872851ea9844e`.

| Policy / output / build | Elapsed ms (3 samples) | Peak RSS MiB (3 samples) |
|---|---|---|
| stream / text / baseline | 101.613, 102.076, 102.490 | 41.531, 41.172, 41.297 |
| stream / text / current | 102.023, 102.528, 104.273 | 41.688, 41.703, 41.547 |
| stream / dict / baseline | 118.140, 117.438, 118.169 | 42.078, 42.219, 42.156 |
| stream / dict / current | 120.434, 120.575, 121.673 | 42.406, 42.359, 42.328 |
| stream / rawdict / baseline | 311.986, 312.381, 312.097 | 50.156, 53.172, 50.062 |
| stream / rawdict / current | 314.342, 312.025, 312.691 | 51.078, 51.188, 51.250 |
| retain / text / baseline | 101.548, 101.814, 101.718 | 42.141, 42.016, 41.844 |
| retain / text / current | 104.331, 105.486, 105.020 | 42.031, 41.969, 42.281 |
| retain / dict / baseline | 125.926, 125.070, 125.463 | 90.688, 90.625, 90.828 |
| retain / dict / current | 128.933, 125.239, 129.345 | 90.984, 90.719, 90.969 |
| retain / rawdict / baseline | 369.822, 365.768, 366.214 | 430.531, 430.312, 430.453 |
| retain / rawdict / current | 365.586, 369.085, 368.964 | 430.734, 432.266, 430.750 |
