# ADR 0006: Panic isolation and geometry sanitizing at external engine boundaries

- Status: Accepted
- Date: 2026-09-23
- Applies to: `pdf-ocr` engine adapters (currently the `paddle-ocr` bridge to
  `ocrspine`), any future in-process Rust inference engine adapter, and every
  floating-point sort comparator inside the pdfspine workspace

## Context

At rev `e810a9c`, `ocrspine` sorted detection boxes with a comparator that
embedded a same-line tolerance (`src/paddle/detect.rs:345-355` at that rev):
boxes whose vertical centers differed by at most half the smaller height were
compared by `x0`, all others by center y. That relation is not transitive.
Rust ≥ 1.81 `sort_by` may detect a non-total order and panic ("user-provided
comparison function does not correctly implement a total order"). PyO3 turns
the panic into `pyo3_runtime.PanicException`, which derives from
`BaseException`, so `except Exception` does not catch it; users saw OCR abort
the calling thread. Only `engine="paddle"` was affected.

The root cause was fixed upstream in `ocrspine` `041958a`: a strict total key
`(center y, x0)` sort followed by greedy line grouping
(`ocrspine/src/paddle/detect.rs:353-378`). pdfspine v0.11.1 (commit `c20d382`,
`CHANGELOG.md` `[0.11.1]`) pins that rev (`crates/pdf-ocr/Cargo.toml:40`) and
adds the boundary defenses recorded here. The `OcrEngine` contract already
states that implementors must never panic (`crates/pdf-ocr/src/engine.rs:49`);
this ADR records how pdfspine enforces that contract for code it does not own.

## Decision

1. **Contain engine panics at the adapter boundary.** A call into an external
   Rust engine is wrapped in `std::panic::catch_unwind` and a panic becomes
   `Error::Unsupported` carrying the panic message
   (`crates/pdf-ocr/src/paddle/mod.rs:67-81`). Its `kind()` is `"unsupported"`
   (`crates/pdf-ocr/src/error.rs:51`), which the bindings map to
   `PdfUnsupportedError` (`crates/py-bindings/src/lib.rs:114,133`), so the panic
   never reaches Python as `PanicException`.

   Unwinding is safe here because `ocrspine` holds no lock while it infers. Its
   runnable caches are `Mutex<HashMap<..>>` plus a `OnceLock`
   (`ocrspine/src/paddle/model.rs:187-190`); each guard lives only for one
   `get` or `insert`, and the expensive model build runs outside the lock
   (`model.rs:214-232`). The failing sort ran outside any guard, so a panic
   cannot poison a cache, and an interrupted `OnceLock` initialization simply
   stays unset. Per-box recognition uses rayon `par_iter`
   (`ocrspine/src/paddle/mod.rs:87-112`); rayon re-raises a worker panic on the
   calling thread, where `catch_unwind` observes it. `AssertUnwindSafe` is
   justified on that basis: the closure only borrows `&self.inner` and the
   input image, and the partially built result is discarded. The same pattern
   already guards third-party codecs (`crates/pdf-image/src/codecs/jbig2.rs:107`,
   `ccitt.rs:158`, `jpx.rs:41`).

   Costs: the default panic hook still prints the panic message to stderr; the
   affected page's OCR fails with an error rather than degrading to partial
   words; and containment works only while the workspace keeps the default
   `panic = "unwind"` (`Cargo.toml:192-194` sets none). It does not fix the
   engine bug.

2. **Sanitize engine geometry before it enters pdfspine.** `sanitize_words`
   drops any word whose bbox coordinate or confidence is not finite
   (`crates/pdf-ocr/src/paddle/mod.rs:96-107`, test at `:184-195`). Dropping
   is chosen over clamping: a NaN or infinite box has no meaningful position,
   and clamping would invent geometry that downstream clustering, table
   detection and text layers would treat as real evidence.

3. **Every floating-point sort comparator in pdfspine is a total order.** Use
   `f64::total_cmp` / `f32::total_cmp`, chained with `Ordering::then` for
   multi-key orders. `partial_cmp(..).unwrap_or(Ordering::Equal)` is not
   allowed in new or edited code: with NaN present it is not transitive. A
   tolerance must never live inside a comparator; when grouping by tolerance is
   needed, sort by a strict key first and then group the sorted sequence, as
   `ocrspine` `041958a` does. `crates/pdf-api/src/image_table.rs:226-231`, `:354`
   and `:393` were converted in `c20d382`.

4. **Root causes are fixed upstream.** An engine bug is fixed in the repository
   that owns the engine, and pdfspine bumps its pinned rev. The boundary
   defenses are defense in depth, not a substitute for that fix; a contained
   panic is still treated as a bug to report upstream.

## Alternatives rejected

- **Fix upstream only.** This leaves every future engine defect able to
  terminate a caller's thread with an exception most Python code cannot catch.
  pdfspine controls when it upgrades an engine, not what the engine contains.
- **Convert `PanicException` globally in the PyO3 layer.** PyO3 already turns
  panics into `PanicException` at each binding; remapping all of them to
  `PdfError` would mean wrapping every exported method, and would relabel
  pdfspine's own invariant violations as ordinary recoverable errors. The
  adapter knows which component failed, can name it in the message, and can
  apply the domain mapping (engine unavailable or failed → `unsupported`),
  consistent with the existing codec wrappers.
- **Clamp non-finite coordinates.** This fabricates positions; see decision 2.
- **`panic = "abort"`.** An abort ends the whole Python process instead of one
  call, and it disables `catch_unwind`, so no containment would be possible.

## Scope

Any new in-process Rust engine adapter (OCR, layout or table inference beyond
the Tesseract CLI adapter, which runs out of process) must apply decisions 1
and 2 at its boundary, and must satisfy decision 3 in pdfspine-side code that
orders its output. The unwind-safety analysis in decision 1 must be redone for
each engine: if an engine holds a lock or other shared mutable state across a
call that can panic, containment is not safe without further review.

## Consequences and follow-up

No test currently forces a panic through the PaddleOCR adapter; the `ocrspine`
fix removes the only known trigger. No lint enforces decision 3; it is enforced
by review.

Known legacy: as of `5e0dfa1`, 17 `partial_cmp(..).unwrap_or(std::cmp::Ordering::Equal)`
sites remain in 15 sort comparators (two `tables.rs` comparators use it for
both keys):

| File | Sites |
|---|---|
| `crates/pdf-text/src/tables.rs` | 13 (lines 525, 530, 562, 573, 730, 731, 960, 994, 998, 1106, 1179, 1184, 1211) |
| `crates/pdf-api/src/image_table.rs` | 2 (lines 644, 718) |
| `crates/pdf-markdown/src/layout.rs` | 1 (line 983) |
| `crates/pdf-typeset/src/table.rs` | 1 (line 108) |

Some sites sort values that are finite by construction (`image_table.rs:718`
filters non-finite values first), but the pattern remains forbidden in edited
code.
Convert these to `total_cmp` when the surrounding code is next changed, or in
a dedicated slice with output-equivalence checks. This ADR does not change them.
