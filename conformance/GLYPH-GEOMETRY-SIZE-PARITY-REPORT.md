# Structured span size parity (G)

G is accepted within the structured-output scope: `dict`, `rawdict`, `json`, and
`rawjson` now publish `span.size == span.rendered_size`, while `declared_size`
retains the original `Tf` operand, including its sign. The single conversion
change is `serialize.rs::dict_span`; internal `Span.size`, layout, word assembly,
HTML/XHTML/XML, selection, and pdfspine texttrace retain their existing semantics.
This deliberately does not unify size across every format.

The rendered size represents the first glyph. It is not a per-character size or
an average. Use rawdict/rawjson `char.rendered_size` when individual glyph sizes
matter. The experiment substantially improves PyMuPDF parity but does not make
span grouping or every published span size identical to PyMuPDF.

## Frozen experiment and coverage

- Baseline: accepted F v2 (E serialization optimization and NaN guard included),
  `/tmp/pdfspine-glyph-F-env-v2/bin/python`, release wheel SHA256
  `4f8426b4cddff4ca2938a1338b4826121320b2538e95de351e2b6c7011a241b5`.
- G: isolated `/tmp/pdfspine-glyph-G`, branch `glyph-span-size-experiment`,
  HEAD `9912a2362a924cf976d81b8af84952731d5e0ad9` plus the shared F/E source
  snapshot and the structured conversion change. Release environment
  `/tmp/pdfspine-glyph-G-env/bin/python`, wheel SHA256
  `65189975eaedc11656771ec174060db40e5910f6279ef61280021e975ec5d3b5`.
- Oracle: `.venv-oracle`, PyMuPDF 1.28.2; Python 3.12.11. Rust 1.96.0,
  maturin 1.12.6, default release wheel features (`abi3`, `ocr`). Same Apple
  M4 Max machine as the [performance experiment](GLYPH-GEOMETRY-PERFORMANCE-REPORT.md).
- Fixed C corpus: 300 documents, the first **at most 20 pages per document**,
  totaling **1,887 pages**. This is not a claim of every page in long documents.
  `conformance/corpus-diff/corpus.txt` SHA256:
  `1bb1548f351c7187363a82b30228b8de908bc542002684e7daeb92bb9ebbccc8`.

`probe_span_size_parity.py` runs the three engines in separate interpreter
processes. It compares non-whitespace, non-synthetic characters, matching exact
Unicode and origins within 0.01 pt on each axis, after mapping PyMuPDF origins
through page rotation into the displayed coordinate frame. A match must be
unique in both directions; extraction order, span indices, and duplicate
characters are not used to resolve ambiguous matches. Each matched character
compares its containing pdfspine span size with its containing oracle span size.
Thus the metric is character-weighted, not a count of identically segmented spans.

There are 5,808,782 candidate characters, 5,811,119 oracle characters, and
**5,803,856 unique matches**: 99.9152% candidate coverage and 99.8750% oracle
coverage. The 4,926 unmatched candidate characters include 978 ambiguous ones;
7,263 oracle characters are unmatched. These exclusions are outside the parity
claim. No document exceptions or invalid numeric characters occurred.

## Measured size accuracy

The strict tolerance is `max(0.0001 pt, abs(oracle_size) * 0.00001)`.
Improved/worsened requires the absolute error change to exceed this tolerance.

| Character-weighted measure | F declared size | G rendered size |
|---|---:|---:|
| Within strict tolerance | 308,376 | 5,800,735 |
| Within 0.01 pt | 319,385 | 5,800,748 |
| Mean absolute error (pt) | 8.1822136685 | 0.0001148154 |
| Median absolute error (pt) | 8.5541000366 | 7.629394e-9 |
| P95 absolute error (pt) | 11 | 4.577637e-7 |
| Maximum absolute error (pt) | 121 | 0.6996062931 |
| Mean relative error | 83.7779% | 0.000925209% |
| Maximum relative error | 100% | 5.830052% |

5,495,174 matched characters improve, 308,367 are unchanged within tolerance,
and **315 worsen**. Exactly **29 characters lose strict-tolerance acceptance**;
exactly **29 lose 0.01 pt acceptance**. G still has 3,121 matches outside the strict
tolerance; the overwhelmingly improved majority does not eliminate these limits.

### All 315 worsening cases independently checked

The affected characters occur in only two OCR/scanned robustness documents:

| Document | Worsened characters | Distinct affected spans | Actual char.rendered_size within oracle tolerance | Maximum char error (pt) |
|---|---:|---:|---:|---:|
| govdocs1-00018.pdf | 126 | 30 | 126 | 7.681512e-7 |
| govdocs1-00053.pdf | 189 | 11 | 189 | 5.111694e-7 |

Every affected character's own `char.rendered_size` agrees with the matched oracle
within tolerance. The errors are from the existing first-glyph span representative,
not failed origin matching or an erroneous rendered-size calculation. F compares
adjacent transforms, so a series of individually permitted changes can accumulate
across a span. G neither changes this rule nor silently anchors F to the first glyph.

The largest case is `govdocs1-00018.pdf`, page 2 (zero-based index 1), span text
`the 1950's. Colin Gum `. Its matrix x scale changes progressively
`13.44 -> 12.84 -> 12.48 -> 12.0`, with y scale 12. Each adjacent step fits F's
5% linear tolerance. The first glyph sets span size to 12.6996062931 pt, while
`Gum` has actual character size and oracle size 12 pt. `G` is at `(368, 313.6)`;
`u` and `m` are at `(376.664, 313.6)` and `(382.664, 313.6)`. This explains both
the maximum absolute error and the 5.83% relative error.

Another case is `govdocs1-00053.pdf`, page 12, `power...`: declared size 13,
span representative 12.82048835, and oracle size about 12.96099949. The actual
per-character rendered size remains accurate. Independent full affected-character
verification is recorded in `/tmp/pdfspine-G-worsened-analysis.json`.

## Regression and validation evidence

Across all 1,887 sampled pages, F and G have identical text and words hashes, and
identical rawdict hashes after removing **only text span `size`**. Geometry,
`declared_size`, character values, spans, order, and image data are retained in
that projection (image bytes are represented by a length and SHA256 marker).
There are zero `size != rendered_size` cases across all sampled rawdict spans.
Four-format output agreement was also checked on the first page of each of the
300 documents; the affine tests below check all four formats explicitly.

Independent C confirms 0/300 extraction JSON differences and unchanged word
metrics. Independent D confirms exact per-document page text arrays for the fixed
30 GT documents: 439 pages, 1,877,060 characters. Since the truth and deterministic
scorer are unchanged, their prior F GT scores remain exact. The content projection
SHA256 is `b458483dbb7562e4705fd4cb19c72e2a98fc2ab3d0e8c12be5e2935c75afd906`.

Candidate validation before landing:

- `cargo test -p pdf-text --test glyph_geometry --test serialize_golden --test serialize_unit --test serialize_property`: **61 passed**.
- Geometry/serialization/quad Python selection: **44 passed**; after transplantation,
  `python -m pytest python/tests/test_span_size_parity.py python/tests/test_rawdict_serialization.py python/tests/test_glyph_geometry.py -q`: **22 passed** using the isolated G wheel.
- `cargo clippy -p pdf-text --all-targets --features pdf-core/encryption -- -D warnings` and formatting passed. The encryption feature matches the actual wheel build; a preliminary encryption-disabled run encountered a pre-existing `pdf-core::get_object` cfg-specific `never_loop` warning.
- New PYSIZE-001 exercises `Tm`, `cm`, `Tz`, anisotropic scale, shear, negative
  `Tf`, and a singular transform. PYSIZE-002 pins unchanged markup/trace scope.
  PYSIZE-003 rejects wrong-Unicode and duplicate/ambiguous oracle matches.

The experiment and regression evidence justified landing the one-field conversion,
explicit API documentation, and tests into the shared tree. The shared environment
was not modified during the experiment. The final unified repository gate is tracked
in the [overall report](GLYPH-GEOMETRY-REPORT.md).

## Reproduce

```sh
python conformance/probe_span_size_parity.py \
  --corpus conformance/corpus-diff/corpus.txt \
  --baseline-python /tmp/pdfspine-glyph-F-env-v2/bin/python \
  --candidate-python /tmp/pdfspine-glyph-G-env/bin/python \
  --oracle-python .venv-oracle/bin/python \
  --work-dir /tmp/pdfspine-glyph-G-full-parity-fixed \
  --output /tmp/pdfspine-glyph-G-full-parity.json --max-pages 20
```

Full report JSON SHA256: `19ea03b71f0e3c1d282382ef624764c0d45a484f6fcd84883a063079e8653b0c`.
Full ignored artifacts are `/tmp/pdfspine-glyph-G-full-parity.json` and the three
JSONL files in the work directory. `--reuse-workers` recomputes the report from
those frozen outputs; it must only be used with the same corpus and page limit.
An initial pilot exposed JSON hashing of image bytes and was discarded; the final
complete run explicitly hashes image bytes and has zero exceptions.
