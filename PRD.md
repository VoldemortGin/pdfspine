# Product Requirements Document
# oxide-pdf — An MIT-Licensed, Pure-Rust Reimplementation of PyMuPDF

---

## 1. Title, Summary & Document Meta

**Title:** `oxide-pdf` — An MIT-Licensed, Mostly-From-Scratch Rust Reimplementation of PyMuPDF (`fitz`), with a Rust-Native Core API and a `fitz`-Compatible Python Shim

**Working name:** `oxide-pdf` (final name TBD; must avoid "PyMuPDF", "MuPDF", "fitz", "Artifex", "Ghostscript" in the package/crate/domain name — nominative use in docs only).

**One-paragraph summary.** PyMuPDF is the fastest and most capable PDF toolkit in the Python ecosystem — rendering, text/layout extraction, editing, annotation, redaction, and generation in one library — but it is dual-licensed **AGPL-3.0 or commercial-from-Artifex**, which makes it legally radioactive for closed-source products and SaaS, and there is no third party from whom a license can be purchased. `oxide-pdf` removes that landmine: a **pure-Rust, MIT-licensed** PDF engine whose **first-party crates are written from scratch** (the COS object model, parser, cross-reference machinery, repair subsystem, filters, encryption, fonts-for-mapping, text extraction, and the incremental/full writer are all original work; permissive Rust crates are used only for leaf problems — codecs, font parsing, crypto primitives), exposed to Python via **PyO3** through both a clean **Rust-native core API** and a **`fitz`-compatible Python shim** so existing `import fitz` code can migrate with near-zero friction. Scope for v1 is **PDF-first plus image documents** (PNG/JPEG/TIFF/GIF/BMP/WEBP); **vector page rasterization is deferred** to a later phase (the precise boundary of which `Pixmap` paths ship in v1 is pinned in §3.3 and §8.10). The project is built under **strict TDD**: every function is decomposed into named, numbered test cases catalogued before implementation, and "done" means implemented **and** the catalogued tests pass, behind a machine-enforced Definition-of-Done gate.

**Document meta.**

| Field | Value |
|---|---|
| Document type | Product Requirements Document (engineering-grade, drives an AI-assisted TDD build) |
| Version | 1.0 (final) |
| Date | 2026-06-15 |
| Status | Approved for build (pending legal sign-off on the four items in §6.5) |
| Target deliverable | v1 = milestones M0–M5; rendering (M6) explicitly post-v1 |
| Primary license | MIT (or `MIT OR Apache-2.0` to match the Rust ecosystem; either is policy-compliant) |
| Reference baseline | PyMuPDF 1.24.14 / MuPDF 1.24.11 (API surface only, via public docs; behavioral expectations seeded only as allowed by §6.1) |
| Baseline-evolution policy | §17.2 (what happens at PyMuPDF 1.25+) |
| Spec basis | ISO 32000-1 (PDF 1.7), ISO 32000-2 (PDF 2.0), ISO/TS 32003 (AES-GCM), ISO/TS 32004 |
| Benchmark corpus | `BENCH-CORPUS-v1` (frozen snapshot; see §14.1) |
| Conformance corpus | `CONF-CORPUS-v1` (frozen snapshot; see §3.1 + §10.3) |
| Audience | Core engineering team, AI build agents, legal/OSS-review, downstream Rust & Python integrators |

---

## 2. Background & Motivation

### 2.1 The AGPL problem

PyMuPDF is a thin Python binding over the **MuPDF** C engine. Both are controlled by **Artifex Software, Inc.** (also behind Ghostscript) under an identical dual-licensing model:

| Layer | Open-source license | Commercial alternative |
|---|---|---|
| MuPDF (C engine) | **GNU AGPL-3.0** | Commercial license from Artifex (exclusive agent) |
| PyMuPDF (Python binding) | **GNU AGPL-3.0** | Commercial license from Artifex |

The obligation follows the **engine**, not the wrapper: writing your own binding does not escape it, because the underlying MuPDF is AGPL. The AGPL is GPL **plus Section 13** — the network-use clause that deliberately closes the "SaaS loophole." Under it, if users interact with the software over a network (i.e., it powers your API/SaaS), you must offer those users the **complete corresponding source** of your version, including modifications and — under the strong-copyleft reading — the larger work it is combined with.

Why this is a non-starter for most companies:

1. **Source-disclosure of proprietary code.** A single AGPL dependency deep in a PDF pipeline can "infect" the surrounding service, forcing disclosure of backend trade secrets/business logic.
2. **Blanket corporate bans.** Google's publicly documented open-source policy disallows AGPL company-wide; many enterprises maintain a flat "no AGPL" policy because *banning is cheaper than building compliance machinery.* (Citation: Google Open Source "AGPL Policy," opensource.google/documentation/reference/using/agpl-policy.)
3. **Procurement friction & cost.** The only non-AGPL option is a negotiated, per-use-case commercial license from Artifex — community-reported in the **~$10k–$50k/yr** range (indicative figures from community forums, **not** an authoritative Artifex price list). It is a recurring budget line and a legal/procurement process, not a checkbox.

> **Note on claims.** Items 1–3 are sourced as follows: (1) is the plain reading of AGPL §13 and standard OSS-counsel guidance; (2) is the cited Google policy; (3) is explicitly flagged as indicative community report, not a vendor quote. No claim in this section relies on inspecting Artifex source.

### 2.2 Why MIT matters

MIT is permissive: no copyleft, no network clause, no commercial gatekeeper, no per-seat negotiation. It is also more permissive than PDFium's BSD-3 (no binary-attribution friction at the source level) and categorically clear of AGPL. An MIT, pure-Rust engine lets the entire spectrum of users — closed-source SaaS, OSS maintainers who must stay permissive end-to-end, security-sensitive shops, and WASM/edge developers — adopt without copyleft legal review.

**Motivation, in one line:** *PyMuPDF owns "fast + complete + great API" but is AGPL; no permissive library unifies render+extract+edit+generate with a `fitz` API in a memory-safe, embeddable form. `oxide-pdf` fills exactly that gap.*

### 2.3 Why pure Rust (not another C/C++ binding)

- **Memory safety (scoped honestly — see §9.6.1).** PDF parsers and image codecs are a classic CVE class (malformed-input attacks; the iMessage zero-click was a JBIG2 bug). Our **first-party crates** are `#![forbid(unsafe_code)]`, which removes whole bug classes from the code we author. This is *not* a claim that the entire dependency tree is unsafe-free — several leaf crates (codecs, `memmap2`, `rustybuzz`) contain or wrap `unsafe`. The precise safety claim, and how dependency `unsafe` is mitigated, is in §9.6.1.
- **Embeddability & build simplicity.** `cargo add`, no C toolchain to cross-compile, trivial static musl builds, clean WASM/edge targets, and a supply chain that is *auditable in source form* (no opaque prebuilt binary blob shipped in the wheel). Supply-chain attestation specifics are in §11.4.
- **Concurrency.** A `Send + Sync` core that releases the GIL enables safe multi-threaded document processing that PyMuPDF (not thread-safe) cannot match. The exact enforced-vs-recommended concurrency contract is in §9.7.

---

## 3. Goals & Non-Goals

### 3.1 Goals (v1)

All corpus-relative goals are measured against **`CONF-CORPUS-v1`**, a frozen, versioned, license-cleared snapshot defined in §10.3 with an exact file count recorded in `fixtures/CONF-CORPUS-v1.manifest.toml` (target size at freeze: **≥ 8,000 files**; the exact integer is fixed at M1 start and never silently changed — a new snapshot gets a new version suffix).

- **G1.** **Open** ≥ 99% of `CONF-CORPUS-v1` in Lenient mode, where **"open" is defined precisely** (§3.4): the document constructs, `page_count` is returned and is **correct** for files with an independently known page count (self-generated + cross-checked subset), and every page in `0..page_count` loads without error. **Never panic/OOM/hang** on *any* input (corpus, malformed set, or fuzz), enforced as an absolute gate (§9.6).
- **G2.** PyMuPDF-class **text extraction & search** across all `get_text(...)` output models, matching PyMuPDF's output *shapes* (keys, tuple arity, coordinate convention) and quality, with accuracy measured **primarily against self-generated ground truth** (§14.2); pdfminer.six is a sanity cross-check only, never the primary oracle. No AGPL output is copied or used to seed expectations beyond §6.1's narrow allowance.
- **G3.** PyMuPDF-class **editing**: full save, **byte-exact incremental save (only for cleanly-parsed, non-repaired files — see §8.7)**, garbage collection, page operations, and `insert_pdf` merge.
- **G4.** **Annotations, forms, and destructive redaction** with portable appearance streams and the multi-surface security-grade redaction guarantee specified in §8.8.
- **G5.** **Image-document** support (PNG/JPEG/TIFF/GIF/BMP/WEBP) as one-page-per-image documents, image codec decode, and a `Pixmap` produced from decoded images and image XObjects (the exact in-scope `Pixmap` matrix is §3.3).
- **G6.** **Encryption read & write** for the Standard Security Handler R2–R6 (RC4-40/128, AES-128, AES-256), with the R5/R6 write policy and `/ID`-absent fallback in §8.4.
- **G7.** A **clean Rust-native API** plus a **`fitz`/`pymupdf`-compatible Python shim** with a machine-checked compatibility matrix; the legal limits on how PyMuPDF output shapes may be discovered are in §6.1.
- **G8.** **MIT licensing with zero copyleft in the shipped dependency tree**, enforced in CI (dev-only MPL tooling carve-out defined in §6.3).
- **G9.** **Strict TDD**: every function decomposed into catalogued test cases written before implementation; Definition-of-Done gate enforced on every PR via the concrete two-PR / `#[ignore]` workflow in §10.1.1.

### 3.2 Non-Goals (v1)

Calling a known-but-unimplemented method raises `PdfUnsupportedError`/`NotImplementedError` with a link to the compat matrix, **never** a bare `AttributeError`.

1. **Vector page rasterization** — `get_pixmap`/`get_svg_image`/`DisplayList`/`run` for PDF *pages that contain vector/text content* (the entire M6 graphics interpreter, font rasterization, shadings, transparency/blend, ICC color management). Image-doc Pixmaps and the image-XObject Pixmap paths defined in §3.3 are **in scope**; vector-page rasterization is not.
2. **HTML/CSS layout engine** — `Story`, `Xml` DOM, `Archive`, `insert_htmlbox`; `convert_to_pdf` for **non-image** inputs only (image inputs → PDF **is** in scope, M5).
3. **OCR** — `get_textpage_ocr`, `pdfocr_save/tobytes`, Tesseract.
4. **Table detection** — `find_tables`/`TableFinder`/`make_table`.
5. **Optional content/layers (OCG/OCMD)**, advanced page labels (basic `/PageLabels` *reading* for TOC/named-dest interplay **is** in scope per §3.5), journalling (undo/redo).
6. **Digital signature creation/validation** — signature fields read-only; no signing/LTV/DSS. (Append-only incremental mechanics that *preserve* existing signatures **are** delivered in M3, subject to the §8.7 clean-parse precondition.)
7. **Linearization (Fast Web View) writing** — read-transparent only; `linear=True` → unsupported.
8. **Non-PDF input formats** — XPS, EPUB, MOBI, FB2, CBZ, SVG, TXT and the chapter/location reflow model (image docs are the only non-PDF inputs in v1).
9. **Font subsetting on by default** — full-font embedding is the v1 default; subsetting (`allsorts`) ships feature-gated and may trail. The size-KPI reconciliation is in §8.5.1.
10. **Full HarfBuzz shaping / the ~170 `UCDN_SCRIPT_*` IDs** — extraction uses visual order + bidi tagging; shaping is a rendering-era concern.
11. **Bit-identical fitz output / 100% API parity** — explicit non-goal; we target behavioral + shape compatibility within the documented numeric tolerances pinned in §14.5.
12. **MuPDF-internal low-level surfaces** — defined concretely in §3.6 (not hand-waved): `Tools.mupdf_warnings` is **mapped** to our warning collector; `Tools.store_*` (the MuPDF cache-tuning knobs) are **no-ops that warn**; raw `mupdf.*` is **out of scope** and raises `PdfUnsupportedError`.

### 3.3 `Pixmap` scope boundary (resolves the scanned-PDF case)

This subsection is **normative** and removes the §1/§8.10 ambiguity. The rule is decided by **what produces the pixels**, not by the document container.

| Source | `get_pixmap()` behavior in v1 | Rationale |
|---|---|---|
| **Image document** (PNG/JPEG/TIFF/…) | **In scope.** Decode the source image → `Pixmap`. | No interpreter needed. |
| **PDF page whose content stream is *only* one or more image XObjects** (the scanned-PDF case: a single full-page `Do` of an image, optionally with a CTM and no text/vector ops) — detected by the **"image-only page" classifier** below | **In scope.** Decode the referenced image XObject(s), apply the placement CTM as an affine resample onto the page raster at the requested DPI, and return the `Pixmap`. | This is the headline RAG/scanned-doc path and must work in v1. It requires only image decode + affine resample, **not** a graphics interpreter. |
| **PDF page containing any text-show, path-paint, shading, or inline-image-with-vector-context operator** (a "vector page") | **Deferred (M6).** Raises `PdfUnsupportedError` with a matrix link. | Requires the full M6 interpreter. |

**Image-only page classifier (normative, testable).** A page qualifies for the in-scope image path **iff**, after concatenating `/Contents` and resolving Form XObjects (depth-capped), the operator stream contains **only**: graphics-state ops (`q Q cm gs cs CS`), color ops, and one-or-more `Do` invocations that each resolve to an **image** XObject — and **zero** of: text operators (`BT…ET`, `Tj/TJ/'/"`), path-construction/paint operators (`m l c v y re S s f F B …`), shading (`sh`), or inline images carrying vector context (`BI…ID…EI`). A page with a single image XObject **plus** an OCR text layer (text operators present) is a **vector page** for Pixmap purposes (deferred), but its **text and image extraction still work** in v1 via `get_text` and `extract_image` — extraction never depends on rendering.

**Degradation contract for the image path.** If the page is image-only but a referenced image XObject uses a codec that fails to decode (JBIG2/JPX edge cases, §8.4/§5 below), `get_pixmap()` raises a typed `PdfUnsupportedError`/`PdfDecodeError` (never a panic), and — critically — **text extraction continues independently** (the codec failure does not abort the document). Tested by `PIXMAP-IMGONLY-*` and `JBIG2-FAIL-*` (§10).

### 3.4 Definition of "open" (resolves the §3.1↔§14 mismatch)

A file is counted **"opened"** for G1/KPI purposes iff **all** of: (a) `Document::open` returns `Ok`; (b) `page_count()` returns a value; (c) for files in the **page-count-verified subset** (self-generated + the independently-validated portion of `CONF-CORPUS-v1`), `page_count()` equals the known value; (d) every `load_page(i)` for `i in 0..page_count` returns `Ok`. Files where (a)/(b)/(d) hold but no independent page count exists count as "opened" but do **not** contribute to page-count accuracy stats. This single definition is used identically in §3.1, §12 (M1 exit) and §14.

### 3.5 Page labels / named destinations interplay (resolves critique #10 gap)

Page-label *authoring* and rich label formats are deferred (§3.2 #5), but **reading** `/PageLabels` is **in scope at P1/M3** strictly to the extent needed so that (a) `get_toc()` can surface the human-facing page label alongside the physical page index, and (b) named-destination / "go to page N" resolution returns the correct *physical* page even when the document uses non-trivial label ranges. The deliverable is: parse the `/PageLabels` number tree into `(physical_index → label_string)`; expose `Page.get_label()` returning the computed label (PyMuPDF-compatible); never *write* `/PageLabels`. Tested by `PAGELABEL-*`.

### 3.6 MuPDF-internal surfaces — concrete dispositions (resolves critique #10)

| PyMuPDF surface | v1 disposition | Behavior |
|---|---|---|
| `Tools.mupdf_warnings()` / `Tools.reset_mupdf_warnings()` | **Mapped** | Returns the formatted contents of our `Warning{offset,kind,detail}` collector (§8.2); reset clears it. |
| `Tools.store_shrink` / `store_maxsize` / `store_size` | **No-op + warn** | Our cache is `Limits`-bounded (§9.6); these knobs return sentinel values and emit a one-time deprecation warning. |
| `Tools.set_aa_level` / `set_small_glyph_heights` / `set_subpixel_*` | **No-op + warn (render-era)** | Rendering is M6; these are accepted and ignored with a warning until then. |
| `Tools.set_annot_stem` | **Implemented** | Affects our generated annotation `/NM` stems. |
| Raw `mupdf.*` module access | **Out of scope** | `PdfUnsupportedError` with matrix link. |

---

## 4. Target Users & Use Cases / User Stories

### 4.1 Personas

**Primary — the "AGPL-blocked builder."** SaaS / closed-source teams whose legal/OSS review bans AGPL (or who refuse the Artifex fee), currently falling back to slower pure-Python (pdfplumber/pypdf) or to pypdfium2 with hand-rolled editing.

**Secondary:**
- **Rust application & systems developers** who refuse to FFI into a C/C++ blob (memory safety of first-party code, single-language build, source-auditable supply chain).
- **WASM / edge / serverless developers** who need PDF processing where shipping a native C engine is impractical.
- **Security-sensitive shops** (gov, fintech, healthcare) wanting a memory-safe parser to reduce malformed-PDF CVE exposure.
- **PyMuPDF migrants** — existing `import fitz` codebases shedding AGPL exposure with minimal rewrite.
- **OSS maintainers** who must stay permissive end-to-end (an AGPL dep would relicense their whole project).

### 4.2 Headline use cases

1. **LLM/RAG document ingestion at scale, license-clean** — high-throughput text + layout extraction, **including scanned PDFs** whose pages are image-only (§3.3 guarantees `get_pixmap` + image extraction on that path; OCR itself is out of scope but the pixels are deliverable).
2. **License-clean server-side processing** — merge/split, redaction, watermarking, form fill.
3. **Memory-safe PDF processing in security-sensitive pipelines.**
4. **Drop-in AGPL-escape for existing PyMuPDF users.**

### 4.3 User stories (selected, with acceptance signals)

| ID | As a… | I want to… | So that… | Acceptance signal |
|---|---|---|---|---|
| US-1 | RAG engineer | extract `get_text("dict")` from arbitrary PDFs | I can chunk documents | shape parity with PyMuPDF dict; **char accuracy ≥0.98 vs self-generated ground truth (§14.2)**; pdfminer agreement reported as secondary diagnostic |
| US-2 | SaaS backend dev | `import fitz` and have it Just Work | I can drop the AGPL dep | fitz-compat suite green; **`implemented` coverage ≥85% of in-scope symbols** (§14.3) |
| US-3 | Compliance officer | redact PII destructively across all surfaces | the data is gone everywhere | **decompressed-corpus** multi-surface scrub gate passes (§8.8); `get_text()` clean; image-region re-encode verified |
| US-4 | Document service | merge N PDFs deduping shared resources | output stays reasonable | `insert_pdf` round-trip; shared font single-instance after GC-3; `qpdf --check` clean (size KPI reconciled in §8.5.1) |
| US-5 | Signing service | edit a **cleanly-parsed** signed PDF incrementally | signatures survive | `out[..orig.len()] == orig` byte-exact **on clean-parse files**; repaired files rejected/upgraded (§8.7) |
| US-6 | Rust app dev | parse PDFs with no C deps in my build | I can target WASM/musl | pure-Rust tree; `cargo-deny` green; (mmap disabled on WASM, §9.6.1) |
| US-7 | Security team | feed untrusted PDFs safely | no crashes/exploits | 0 fuzz crashes; `Limits` enforced; mmap-truncation handled (§9.6.1) |
| US-8 | Imaging pipeline | open a multi-page TIFF / scanned PDF as a doc | uniform API | page_count == IFD count; `get_pixmap` == decoder output for image-only pages (§3.3) |
| US-9 | Concurrency-heavy service | extract from many docs across threads | linear scaling | GIL released; threaded pytest identical results; concurrency contract §9.7 honored |

---

## 5. Competitive Landscape & Positioning

### 5.1 Comparison table (licenses verified per row; engine vs binding kept distinct)

| Library | Binding license | Engine license | Render to image | Text+layout extraction | Edit/write | Generate | Key gap |
|---|---|---|---|---|---|---|---|
| **PyMuPDF (fitz)** | AGPL-3.0 / commercial | MuPDF (C) — AGPL-3.0 / commercial | ✅ fast | ✅ rich | ✅ | ✅ | **License is the dealbreaker** |
| **pypdf** | MIT | pure Python (no separate engine) | ❌ (never had raster render) | ⚠️ basic, no layout; **does extract embedded images** | ✅ split/merge/forms/encrypt | ⚠️ limited | no render, weak text extraction, slow |
| **pikepdf** | MPL-2.0 | qpdf (C++) — **Apache-2.0** | ❌ | ❌ | ✅ structural/repair | ⚠️ low-level | not a content/extraction tool |
| **pdfminer.six** | MIT | pure Python | ❌ | ✅ low-level chars/pos | ❌ | ❌ | extraction-only, slow |
| **pdfplumber** | MIT | pure Python (on pdfminer) | ❌ (debug render only) | ✅ strong + tables | ❌ | ❌ | extraction-only, slow |
| **pypdfium2** | Apache-2.0 / BSD-3 (binding) | PDFium (C/C++) — BSD-3 / Apache-2.0 | ✅ fast | ✅ good | ⚠️ limited | ⚠️ limited | prebuilt C blob, weak editing, no fitz API |
| **ReportLab** | open-source lib is **BSD-3**; a **separate** paid product (RML/PLUS) exists — **not** a dual-license of the same code | Python (+C accelerators) | ❌ | ❌ | ❌ | ✅ | generation only |
| **pdfrw** | MIT | pure Python | ❌ | ❌ | ✅ read/write/merge | ⚠️ low-level | minimally maintained |
| **borb** | AGPL-3.0 + commercial | pure Python | ❌ | ✅ some | ✅ | ✅ | same AGPL trap, pure Python |
| **WeasyPrint** | BSD-3 | pure Python | n/a (HTML→PDF) | ❌ | ❌ | ✅ HTML→PDF | different job |
| **`oxide-pdf`** | **MIT** | **pure Rust (first-party from scratch)** | image-docs + image-only pages now; vector deferred (M6) | ✅ PyMuPDF-class | ✅ incl. incremental/merge/redaction | ✅ | vector rendering deferred to M6 |

> **Table accuracy notes (credibility is our core asset):** (a) pypdf never shipped a raster renderer; the "❌ render" is correct, but it *does* extract embedded images, now reflected. (b) pikepdf is MPL-2.0 **binding** over an **Apache-2.0** qpdf engine — binding and engine licenses are now separated, matching the discipline we use for ourselves. (c) ReportLab's open-source library is BSD-3; the commercial offering is a *separate paid product*, not a dual-license of the same source — the misleading "BSD + commercial dual-license" framing is removed. (d) pypdfium2 wheels and PDFium are both permissive but the engine is a **prebuilt C/C++ binary blob** in the wheel — that is the differentiator we lean on, not raw speed.

### 5.2 Honest assessment of the permissive incumbent

We are candid: the "permissive + fast + render + extract" need is **partially already met by pypdfium2** (PDFium is BSD, mature, the engine in Chrome). For "render a *vector* page to a bitmap, permissively, fast," pypdfium2 is the honest answer **today** and remains so until our M6. Differentiation therefore is **not** "permissive + fast vector render" alone.

### 5.3 The unmet niche & positioning

No single library is **all** of: from-scratch **pure-Rust first-party** (memory-safe authored code, embeddable, WASM-friendly, no shipped C blob) + **MIT** + **render+extract+edit+generate** in one + **`fitz`-compatible API** + **Python bindings**. That intersection is empty, and it is the product.

> **For** developers and companies who need PyMuPDF's speed and breadth **but cannot accept AGPL or pay Artifex,** **`oxide-pdf`** is a **pure-Rust, MIT-licensed PDF toolkit** with first-class **Python bindings and a PyMuPDF-compatible API.** **Unlike PyMuPDF** it imposes no copyleft and no fee; **unlike pypdfium2** it is from-scratch first-party Rust with full editing/authoring and no shipped C blob; **unlike pypdf/pdfplumber** it is fast. *One library, four capabilities, zero copyleft risk.*

Taglines: *"PyMuPDF's power, MIT's freedom, Rust's safety."* / *"PDF processing without the AGPL."*

### 5.4 Naming caution

Do **not** use "PyMuPDF / MuPDF / fitz / Artifex / Ghostscript" in the product, package, or domain name. **Nominative use is fine in docs/marketing** ("a PyMuPDF alternative," "compatible with the `fitz` API"). A literal `import fitz` shim is a strong migration feature but should be an **optional, clearly-labeled compatibility package**, not the core package name; get counsel sign-off before naming any public artifact `fitz`. Run PyPI/crates.io/USPTO/EU/domain clearance before committing the name.

---

## 6. Licensing & Clean-Room Strategy

### 6.1 What we MAY and MUST NOT do — with the AGPL-output question resolved

Grounded in **Google v. Oracle (2021)** (reimplementing an API's *declarations*/method-name structure can be fair use; *Oracle* did **not** bless copying output formats, expressive serialization, or behavior cloning of creative output) and standard clean-room practice (e.g., AWS clean-rooming MongoDB's API to avoid SSPL). **Critical correction over prior drafts:** running AGPL software and reproducing its *expressive* output is **not** categorically safe. We therefore split "output" into two tiers and treat them differently.

**Tier A — facts/structure that MAY seed expectations (low risk):**
- The **API surface**: class names, method names/signatures, parameter names, and the **documented** return *shape* (which keys exist, tuple arity, coordinate convention) **as stated in public PyMuPDF documentation**. These are functional/structural facts, port-enabling, and the core of *Oracle*-permitted reuse.
- **Values that are dictated by the public ISO spec** (e.g., that a `words` tuple is `(x0,y0,x1,y1,…)` because the geometry is spec-defined): facts, freely usable.
- Our **own** computed outputs validated against the **ISO spec + self-generated ground truth**.

**Tier B — expressive/observed output that MUST NOT seed expectations (high risk):**
- The **exact byte-for-byte serialization** of `html`/`xhtml`/`xml` output, the precise prose of error messages, the exact undocumented field set of `rawdict`, or any field-level detail **not present in public docs**. These may be **expressive**; cloning them from observed AGPL output is the contamination path clean-room exists to prevent.
- Where PyMuPDF docs are silent on an exact shape detail, we **derive the shape from the ISO spec + our own design** and document it in `COMPAT.toml` as an **intentional `oxide-pdf`-defined shape** (a documented deviation), rather than reverse-engineering PyMuPDF's undocumented bytes.

**We MAY:**
- Reimplement the `fitz`/PyMuPDF **API surface** (Tier A) so `import fitz` code ports.
- Rely on **public specifications** — ISO 32000-1/-2, OpenType/CFF/Type1, **public PyMuPDF docs/tutorials**, ISO/W3C/Adobe references.
- Observe **black-box functional behavior at the API-contract level** — e.g., "does `authenticate` accept the user password and return True." Functional pass/fail observation is permitted.

**We MUST NOT:**
- Read, paste, translate, or port **any AGPL MuPDF/PyMuPDF source** (C→Rust translation creates a derivative work carrying AGPL — the single highest-risk action).
- Use the **oracle (§6.2 #8)** to lift Tier-B expressive output into our expectations (see the strict oracle-handling protocol in §6.2.1).
- Copy MuPDF's/PyMuPDF's **test suites or sample PDFs**, or any GPL/AGPL-encumbered corpus.
- Copy internal **algorithm code, data tables, or magic constants** lifted from MuPDF source (constants defined by the *public ISO spec* are facts and freely usable).
- Copy copyrightable **documentation prose** verbatim.

### 6.2 Clean-room hygiene rules (AI-assisted build)

1. **Two-room discipline.** Spec-room agents read the public ISO spec + PyMuPDF *docs* (Tier-A API names/behavior only). Implementation-room agents write Rust from a *functional spec*, never from MuPDF source. Contexts stay separate.
2. **Never put AGPL source in an AI context window.** No pasting MuPDF/PyMuPDF `.c`/`.py` into prompts; never "translate this function to Rust."
3. **API-from-docs, implementation-from-spec.** Feed the model the *documentation signature*, not the implementation body.
4. **Provenance logging.** Each module records what informed it (ISO clause numbers, doc URLs); auditable trail proving spec + Tier-A origin.
5. **License-scan the dependency tree** in CI (`cargo-deny` + `cargo-about`) — block any GPL/AGPL/SSPL/LGPL crate in the shipped graph and any MPL crate in the *shipped* graph (dev-only MPL carve-out: §6.3).
6. **Clean test corpus only.** Every fixture documents **affirmative permissive/PD license** (not merely "AGPL-absent") with a named clearer (§10.3 / §6.4).
7. **Generated-output check.** Verbatim/near-verbatim reproduction of any known AGPL source is a build-blocking defect.
8. **PyMuPDF as ephemeral dev-only oracle** — a throwaway, non-committed harness behind `--features oracle-agpl`; subject to the strict protocol in §6.2.1. Its output is never stored as a golden, never committed, never used to seed Tier-B expectations. A CI grep guards that no file under `src/`/`tests/` imports `fitz`/`pymupdf`.

#### 6.2.1 Oracle-handling protocol (resolves critique #2 — the highest legal risk)

The oracle is a **discrepancy flagger of last resort**, not an expectation source. The following are **normative**:

- **Who may view oracle output.** Only **clean-room–excluded personnel/agents** (people/agents who are *not* and will *never be* in the implementation room for the affected subsystem). Implementation-room agents are forbidden from viewing oracle diffs for code they author. This is enforced by an access boundary on the oracle harness output (separate repo/area, access-logged).
- **What the excluded reviewer may communicate back.** Only a **Tier-A, spec-anchored functional bug report**: "Our `words` output omits a glyph that ISO 32000-1 §9.4.3 says is shown" — i.e., the discrepancy is **re-derived from the spec**, not described as "PyMuPDF emits X, so emit X." The reviewer must cite the **spec clause** that makes our output wrong; if no spec clause does, the discrepancy is **logged but not actioned** (it is, by definition, a Tier-B expressive difference and we keep our own documented shape).
- **No osmosis.** The reviewer must **not** paste oracle output values, byte sequences, or message prose into any artifact the implementation room can see. Translation-by-osmosis is treated as contamination (build-blocking).
- **Audit.** Every oracle-driven change carries a provenance note naming (a) the excluded reviewer, (b) the spec clause re-derived, (c) confirmation that no Tier-B bytes crossed the boundary.
- **Default off.** `oracle-agpl` is never enabled in CI, never in release builds, and the harness is in a `dev/oracle/` area excluded from the published crate and from the wheel.

> **Residual-risk acknowledgment.** Even with this protocol, "a human looks at AGPL output and then a fix happens" carries non-zero legal risk. Counsel sign-off on the oracle protocol is a **gating item** (§6.5). If counsel rejects it, the oracle is removed entirely and we rely solely on ISO spec + self-generated ground truth + permissive cross-checkers (qpdf/pikepdf/pdfminer/pdf.js).

### 6.3 License compatibility matrix (our shipped library is MIT)

| License | Verdict (shipped graph) | Dev-only tooling | Why |
|---|---|---|---|
| MIT | ✅ use freely | ✅ | permissive |
| Apache-2.0 | ✅ use freely | ✅ | permissive + patent grant |
| BSD-2 / BSD-3 | ✅ use freely | ✅ | permissive |
| Zlib / libpng / ISC / 0BSD | ✅ use freely | ✅ | permissive |
| Unlicense / CC0 (test data) | ✅ use freely | ✅ | PD-equivalent |
| Unicode-DFS-2016 / IJG | ✅ use freely (reproduce notices) | ✅ | permissive |
| **MPL-2.0** | ❌ **excluded from the shipped graph** | ✅ **allowed dev-only / not linked / not distributed in the wheel** | file-level copyleft; isolated dev tools (e.g., `hypothesis`) that are neither shipped nor linked do not impose obligations on our distribution. The dev-only exception is **policy-level**, not just a per-row note, and `cargo-deny`/`pip`-policy encode the `[graph] ship` vs `[graph] dev` distinction. |
| **LGPL-2.1/3.0** | ⚠️ avoid | ⚠️ avoid | The "static-Rust relink obligation is unsatisfiable" argument is the **common** position but **not absolute** — LGPL-3 §4 can sometimes be satisfied by shipping relinkable object files or a dynamic boundary. We therefore **avoid LGPL by policy** rather than asserting impossibility; if ever needed it must clear counsel. |
| **GPL-2.0 / 3.0** | ❌ forbidden | ❌ | strong copyleft would relicense us |
| **AGPL-3.0** | ❌ forbidden | ❌ (oracle is a non-distributed dev harness, §6.2.1) | the exact thing we escape |
| **SSPL / BSL / source-available** | ❌ forbidden | ❌ | non-OSI restrictive |

**Default crate policy:** prefer `MIT OR Apache-2.0`; accept BSD/Zlib/ISC/Unlicense/Unicode-DFS/IJG; hard-block GPL/AGPL/LGPL/SSPL **and MPL in the shipped graph** in CI; permit MPL **only** in the explicitly-labeled dev/test graph that is not linked into or distributed with the wheel.

### 6.4 Dependency license matrix (key deps) — licenses re-verified

| Crate | License (verified) | Verdict |
|---|---|---|
| flate2 / miniz_oxide | MIT/Apache(/Zlib) | ✅ |
| weezl | MIT/Apache-2.0 | ✅ |
| zune-jpeg / jpeg-decoder | MIT/Apache(/Zlib) | ✅ |
| jpeg-encoder | MIT/Apache **AND IJG** | ✅ (reproduce IJG notice) |
| hayro-jbig2 / hayro-jpeg2000 / hayro-ccitt | MIT/Apache-2.0 | ✅ (pre-1.0 — see R2/R3 fallback policy) |
| fax | MIT | ✅ |
| image / png / tiff | MIT/Apache-2.0 | ✅ |
| ttf-parser / rustybuzz | MIT/Apache, MIT | ✅ (contain `unsafe` — §9.6.1) |
| allsorts | Apache-2.0 | ✅ (single-point subsetter — fallback policy §8.5.1) |
| swash / fontdue / ab_glyph (M6) | Apache / MIT-Apache / Apache-MIT | ✅ |
| moxcms (M6) | BSD-3/Apache-2.0 | ✅ |
| **qcms** | MPL-2.0 | ❌ excluded — use moxcms |
| encoding_rs | (Apache-2.0 OR MIT) AND BSD-3 | ✅ |
| unicode-normalization / -bidi / -segmentation | MIT/Apache-2.0 | ✅ |
| aes / cbc / rc4 / sha2 / md-5 (RustCrypto) | MIT/Apache-2.0 | ✅ |
| bytes / parking_lot / thiserror | MIT/(Apache) | ✅ |
| **memmap2** | MIT/Apache-2.0 | ✅ license-wise; **wraps `unsafe` mmap → truncation-UB mitigation in §9.6.1** |
| pyo3 / maturin | MIT/Apache-2.0 | ✅ |
| rust-numpy | BSD-2-Clause | ✅ |
| proptest / criterion / cargo-fuzz / arbitrary | MIT/Apache | ✅ |
| **insta** | **MIT OR Apache-2.0** (dual; earlier releases were Apache-only — current is dual) | ✅ (cited correctly here; prior "Apache-only" framing was wrong) |
| cargo-deny / cargo-about / cargo-mutants / cargo-llvm-cov | MIT/Apache | ✅ |
| **mupdf-rs** | **AGPL-3.0** | ❌ banned (listed only to exclude) |
| **jbig2dec** | GPLv3/AGPL | ❌ banned |
| pdfium-render (optional oracle/feature) | MIT/Apache (binding); PDFium BSD/Apache | ✅ as oracle/optional only (not shipped by default) |
| **hypothesis** (Python dev dep) | **MPL-2.0** | ✅ **dev-only**, not shipped/linked (per §6.3 policy carve-out) |

### 6.5 Legal gating items (must clear counsel before the dependent work merges)

These are **not** "open questions" — they are **blocking** because dependent milestones cannot ship without them. Tracked as release-blockers:

1. **Oracle protocol sign-off (§6.2.1).** If rejected → remove oracle entirely. (Blocks: any oracle-assisted work.)
2. **Adobe data vendoring (§8.5).** Confirm licenses/notices for Core-14 AFM, AGL/AGLFN, predefined CJK CMaps **before M2 font-mapping merges**. The clean-room thesis depends on these being permissively licensed; if any is not, that data path is replaced (see §8.5.2 fallback). (Blocks: M2 font mapping.)
3. **`fitz`-name artifact clearance (§5.4).** Before any public artifact is named `fitz`. (Blocks: shim package naming, not the build.)
4. **Tier-B output framing (§6.1).** Counsel confirmation that Tier-A/Tier-B split is sufficient and that black-box *functional* observation (not expressive-output cloning) is acceptable. (Blocks: nothing technically, but defines the §6.1 boundary the whole build relies on.)

---

## 7. Scope & Feature Catalog Mapped to PyMuPDF API (P0–P3)

**Priority:** P0 = v1 must-ship core; P1 = strongly expected for credible parity; P2 = valuable, may trail (with a documented coverage subset); P3 = niche/post-v1. **Every PyMuPDF capability an AI builder will encounter is mapped to a (priority, milestone)** — including the surfaces previously unlisted (inline images, patterns/shadings-in-content, `get_drawings`, `Widget`, per-method default flags), which are now explicit rows.

| Capability (PyMuPDF surface) | Priority | Milestone |
|---|---|---|
| Geometry: Matrix/Point/Rect/IRect/Quad + constants | P0 | M0 |
| `open` (file/bytes/filetype), Document lifecycle, `close` | P0 | M1 |
| xref classic + **xref streams** + **object streams** + trailer | P0 | M1 |
| **Malformed-PDF repair / reconstruction** | P0 | M1 |
| Filters: Flate(+predictors)/LZW/ASCIIHex/ASCII85/RunLength (decode) | P0 | M1 |
| Page tree + inheritance, boxes, rotation; `page_count`/`load_page` | P0 | M1 |
| Encryption **read/open** R2–R6, permissions, `authenticate`; `/ID`-absent fallback | P0 | M1 |
| Low-level xref/object read API (`xref_object`/`xref_stream`/`xref_get_key`) | P1 | M1 |
| `get_text` text/blocks/words/dict/json/rawdict | P0 | M2 |
| `get_text` html/xhtml/xml; `get_textbox`/selection | P1 | M2 |
| **Per-method `TEXTFLAGS_*` default flag sets pinned** (text/blocks/words/dict/rawdict each) | P0 | M2 |
| `search_for` (rects/quads, flags, clip) | P0 | M2 |
| Fonts for mapping (encodings/Differences/ToUnicode/AGL/CJK CMaps/widths) | P0 | M2 |
| `get_fonts`/`get_images` inventory | P1 | M2 |
| **Inline images (`BI/ID/EI`) decode** (needed by `get_images`/`extract_image`/redaction PIXELS — not just "skip") | P1 | M2 (parse/inventory) / M4 (PIXELS re-encode) / M5 (decode-to-Pixmap) |
| **Tiling patterns / shadings as fill in content streams** (parse + classify; needed for redaction line-art and text-on-pattern) | P1 | M2 (classify) / M4 (redaction interaction) |
| **`Page.get_drawings()` / `get_cdrawings()`** (vector path extraction; powers table/line analysis and redaction line-art) | P1 | M4 |
| `TextPage` reusable object | P1 | M2 |
| Full `save` (xref + xref/obj-stream authoring); `tobytes`/`write` | P0 | M3 |
| **Incremental save** / `save_incremental` (clean-parse only, §8.7) | P0 | M3 |
| Garbage collection 1–4 (dedup exclusion list §8.7.1); deflate options | P1 | M3 |
| Encryption **write** (RC4-128/AES-128/AES-256 R6; never write R5, §8.4) | P1 | M3 |
| Page ops: new/insert/delete/copy/move/select; box/rotation setters | P0 | M3 |
| **`insert_pdf`** merge (deep-copy + dedup) | P0 | M3 |
| Metadata write (Info+XMP); `get_xml_metadata`/`set_xml_metadata`; TOC get/set; named dests | P1 | M3 |
| **`/PageLabels` read + `Page.get_label()`** (TOC/named-dest interplay, §3.5) | P1 | M3 |
| `insert_text`/`insert_textbox` (Base-14 + TTF embed) | P0 | M4 |
| `insert_image`; `draw_*` + `Shape`; link insert/update/delete | P1 | M4 |
| Annotations: full `add_*_annot` + `Annot` + `/AP` generation | P1 | M4 |
| **`apply_redactions`** (destructive, multi-surface §8.8) | P0 | M4 |
| Forms: read + fill + flatten (AcroForm) **+ `Widget` object API** (field_type/field_value/field_name/…) | P1 | M4 |
| Embedded files (`embfile_*`); `bake`/`scrub` | P2 | M4 |
| Font subsetting (`subset_fonts`, insert-text subsetting) | P2 (documented subset) | M4/M5 (feature-gated) |
| Image documents (PNG/JPEG/TIFF/GIF/BMP/WEBP) | P0 | M5 |
| `convert_to_pdf` **(image inputs only)** | P0 | M5 |
| Image codecs decode: DCT/CCITT | P1 | M5 |
| Image codecs decode: **JBIG2 / JPX (documented coverage subset, P2-may-trail, degradation contract §8.4.1)** | P2 | M5 |
| `Pixmap` (image docs + image-only pages §3.3), buffer-protocol/numpy, save/tobytes | P0 | M5 |
| `extract_image`/`extract_font` | P2 | M5 |
| `fitz`/`pymupdf` compat shim hardening + COMPAT matrix | P0 | M5 |
| **Page rendering** `get_pixmap`/`get_svg_image`/`DisplayList` (vector pages) | deferred | **M6 (post-v1)** |
| Story/Xml/Archive, `insert_htmlbox`, `convert_to_pdf` (non-image) | P3 | post-v1 |
| OCR (`*_ocr`, `pdfocr_*`), `find_tables` | P3 | post-v1 |
| OCG/layers, advanced page labels (write), journalling | P3 | post-v1 |
| Digital signature **create**; linearization **write** | P3 | post-v1 |
| Non-PDF inputs (XPS/EPUB/MOBI/FB2/CBZ/SVG) | P3 | out of scope |

**Deliberately-unlisted surface (explicit catch-all).** Any PyMuPDF symbol not appearing above is **`out-of-scope` by default** and must raise `PdfUnsupportedError` (never `AttributeError`). `COMPAT.toml` enumerates every such symbol with a status; a CI check fails if a PyMuPDF public symbol exists in the pinned baseline that is **absent** from `COMPAT.toml` (forces an explicit disposition for new surface — see baseline-evolution policy §17.2).

---

## 8. Functional Requirements by Subsystem

> Blunt design center, repeated throughout: **real-world PDFs are routinely malformed.** Spec compliance on *write* is the easy 30%; the hard 70% is parsing the garbage 25 years of generators have emitted. Architect around tolerance, not the happy path.

### 8.1 PDF parser / object model (`pdf-core`)

**Object types (8 + indirection).**

| Type | Pitfalls | Diff |
|---|---|---|
| Boolean | trivial | Low |
| Numeric (int `i64`, real `f64`) | tolerate `1e3`, `+`, `1.2.3`, `.5`, `4.`, `--2`; clamp on parse failure | Low/Med |
| String literal `( )` | balanced parens, escapes `\n \r \t \b \f \( \) \\`, octal `\ddd`, line-continuation, raw newlines; bytes not text | Med |
| String hex `<…>` | whitespace inside; odd nibble → pad 0 | Low |
| Name `/Name` | `#XX` escapes; byte sequence post-decode; `/` = empty name | Low/Med |
| Array `[ ]` | heterogeneous, may contain refs | Low |
| Dictionary `<< >>` | duplicate keys (last wins, warn); odd token count must not crash | Med |
| Stream `<<dict>> stream … endstream` | `/Length` often wrong → repair; exactly one EOL after `stream` | High |
| Null | value; ref to nonexistent object yields null, not error | Low |
| Indirect ref `N G R` | resolution, cycle detection, lazy load | Med |

**Tokenizer:** whitespace `\0 \t \n \f \r SPACE`; delimiters `( ) < > [ ] { } / %`; comments `%`…EOL except in strings/streams. Single robust lexer reused by content-stream parsing; resync on garbage, never panic.

**Object model (Rust).** Flat value graph; `Object` enum stores `Reference(ObjRef)` rather than inline children so the graph is flat and `Clone`-cheap; stream bytes held out-of-line (`bytes::Bytes`) for O(1) clone and zero-copy from mmap. (Full type sketch in §9.2.)

**Document structure:** Catalog (`/Pages /Names /Dests /Outlines /AcroForm /Metadata /Version /Lang /PageLabels …`); page tree `/Pages` (intermediate `/Kids /Count /Parent`) + `/Page` leaf; **inheritable attributes** (`/Resources /MediaBox /CropBox /Rotate`) resolved by walking to root (top correctness pitfall); `/Contents` may be a **stream or array of streams** (concatenate with whitespace before tokenizing); `/Rotate` multiple of 90 (negative/>360 allowed; normalized per §8.6.1).

**Requirements:** lazy load with interior-mutable `Arc<Object>` cache; mmap-backed source retained for incremental save (mmap-truncation handling §9.6.1); cycle detection on every graph walk; checked arithmetic; `#![forbid(unsafe_code)]`.

### 8.2 Cross-reference machinery

- **Header** `%PDF-1.x`/`%PDF-2.0` + binary-marker line. Header frequently **not at byte 0** (junk/HTTP/BOM prepended) → scan first ~1 KB, **record offset bias `header_offset`**; catalog `/Version` overrides header. **All stored byte offsets are absolute file offsets** (i.e., declared xref offsets are interpreted relative to `header_offset` per spec convention, but resolved to absolute positions immediately on read), so downstream byte-offset assumptions — including incremental-save (§8.7) and signature byte ranges — operate on a single, consistent absolute coordinate. A nonzero `header_offset` marks the file **repair-tainted for incremental-save purposes** (§8.7).
- **Classic xref** — 20-byte entries `nnnnnnnnnn ggggg n/f\r\n`; tolerate 19-byte/bare-`\n` variants; object 0 free-list head gen 65535; `/Size` lies.
- **Trailer & startxref** — `/Size /Root /Info /ID /Encrypt /Prev`; `startxref` often wrong → repair; multiple `%%EOF` → use last `startxref`; `/ID` required for encrypted-file KDF (absent-`/ID` fallback §8.4).
- **Xref streams (PDF 1.5+)** — `/Type /XRef`, Flate + PNG/TIFF predictors, `/W [w1 w2 w3]`, three entry types (free / uncompressed / compressed). **Mandatory.**
- **Object streams** — `/Type /ObjStm`, `/N`, `/First`; objects inside are **never themselves encrypted** (container is); cannot contain streams/Encrypt/gen≠0; nested resolution must not infinitely recurse. **Mandatory.**
- **Incremental updates / `/Prev` chains** — build effective xref newest→oldest; cycle detection mandatory; a later section may re-free an object.
- **Hybrid-reference files** — `/XRefStm`; read table first, overlay xref-stream objects, then `/Prev`.
- **Linearization** — **read-transparent only**; writing deferred.

**Repair subsystem (the differentiator, P0):**
1. Full object scan / reconstruction (regex/state-machine for `N G obj`/`endobj`, last-defined wins).
2. Recover trailer / locate Catalog by `/Type /Catalog`; recover `/Info`/`/Encrypt` by type.
3. Reconstruct page tree by scanning all `/Type /Page` → synthesize flat `/Kids`.
4. Stream `/Length` repair — locate real `endstream`, ignore declared length.
5. Tolerant tokenizer fallbacks (missing `endobj`, garbage between objects).
6. Decompress-and-scan ObjStms during recovery.
7. **Validation gate** — after a "clean" parse, verify `/Root`→`/Pages`→pages resolve; else auto-fall-back to full reconstruction.
8. Encryption + repair interaction — determine `/Encrypt`/`/ID` before/during repair (streams incl. ObjStm/XRef may be encrypted).

**Repair taints the parse.** Any document that (a) required reconstruction (steps 1–7 triggered), or (b) had `header_offset ≠ 0`, or (c) had any `/Length`/`startxref`/xref entry corrected, is flagged **`parse_was_repaired = true`** on the `Document`. This flag is the precondition gate for incremental save (§8.7) and is queryable from Python (`doc.is_repaired`).

**Modes:** `Strict` (first violation → error; for validators/signing) vs `Lenient` (default; best-effort, collect queryable `Warning { offset, kind, detail }`). CI runs the corpus in **both**.

### 8.3 Filters / codecs (decode; encode where needed)

`/Filter` may be name or array (pipeline); `/DecodeParms` parallel. Abbreviated names for inline images.

| Filter | Decode | Encode | Diff | Notes |
|---|---|---|---|---|
| FlateDecode | essential | essential | Med | 90%+ of streams; handle zlib + raw-deflate + truncated |
| Predictors PNG 0–4 / TIFF 2 | essential | essential | Med | needed just to read xref streams; `/Predictor /Colors /BitsPerComponent /Columns` |
| LZWDecode | needed | optional | Med | 9–12-bit codes, **EarlyChange** (default 1) — the #1 LZW bug |
| ASCIIHexDecode | needed | easy | Low | `>` terminator |
| ASCII85Decode | needed | easy | Low | `z` shorthand, `~>` terminator, mod-5 final group |
| RunLengthDecode | needed | easy | Low | 128 = EOD |
| DCTDecode (JPEG) | M5 | M5 | High | YCbCr/CMYK incl. inverted Adobe APP14; `zune-jpeg` |
| CCITTFaxDecode | M5 | M5 | High | `/K /Columns(1728) /Rows /BlackIs1 /EncodedByteAlign`; `hayro-ccitt`/`fax` |
| JBIG2Decode | M5 (documented subset) | transcode | Very High | `hayro-jbig2`; treat untrusted; ban GPL `jbig2dec`; **degradation contract §8.4.1** |
| JPXDecode (JPEG2000) | M5 (documented subset) | transcode | Very High | `hayro-jpeg2000`; **degradation contract §8.4.1**; OpenJPEG fallback tension flagged §8.4.1 |
| Crypt | essential | essential | — | `/Identity` = no encryption for that stream |

M1 delivers Flate(+predictors)/LZW/ASCII*/RunLength decode + Flate encode (for authoring xref/object streams). On save, exotic codecs (JBIG2/JPX) are transcoded to Flate/JPEG and the format change documented; redaction interaction with transcoding is specified in §8.8.

### 8.4 Encryption (`pdf-crypto`)

**Standard Security Handler revisions:** R2 (RC4-40), R3 (RC4 40–128, MD5×50), R4 (crypt filters `/CF /StmF /StrF`, RC4 `/V2` or **AES-128 `/AESV2`**, `/EncryptMetadata`), **R5 (AES-256 *transitional*, single SHA-256 validation — read-only, see policy below)**, **R6 (AES-256 `/AESV3 /V 5`, Algorithm 2.B hardened iterated hash over SHA-256/384/512)**; AES-GCM (ISO/TS 32003): **read-tracked, not written in v1** — "tracked" means the `/Encrypt` dict is recognized and a `PdfUnsupportedError` is raised on decrypt rather than mis-parsing (no v1 deliverable beyond detection).

**R5 vs R6 — corrected and policy-pinned (resolves critique #12):**
- **R5** uses a **single SHA-256** of (password + salt) for validation; it is the **transitional/deprecated** AES-256 form and is known to be **weaker/forgeable** relative to R6. Policy: **read R5** (decrypt legacy files) **but NEVER write R5.**
- **R6** uses **Algorithm 2.B** (iterated SHA-256/384/512 hardened loop). Policy: **write AES-256 only as R6.**

**Per-object key derivation — corrected (resolves critique #12):**
- For **RC4 and AES-128 (R2–R4 `/V` ≤ 4)**: per-object key = `MD5(filekey ‖ objnum[3 bytes LE] ‖ gen[2 bytes LE] [‖ 0x73 0x41 0x6C 0x54 ("sAlT") **only for AESV2**])`, then **truncate to `min(filekey_len + 5, 16)` bytes**. The `"sAlT"` 4-byte salt is appended **only** for AESV2 (AES-128), **not** for RC4. The `min(len+5,16)` truncation is mandatory and a common bug source.
- For **R6/AESV3 (`/V 5`)**: the **file key is used directly** — **no per-object salting, no truncation** (a very common bug).

**`/ID`-absent fallback (resolves critique #12 omission).** When `/Encrypt` requires the document `/ID` for KDF (R2–R4 use `/ID[0]` in the file-key computation) but `/ID` is **absent** (common in malformed encrypted files): treat the missing `/ID[0]` as the **empty byte string** (matching the de-facto behavior of tolerant readers), emit a `Warning{kind: MissingId}`, and proceed; if decryption then fails authentication, surface `NeedsPassword`/`Crypto` rather than panicking. R6 does not depend on `/ID`, so R6 files are unaffected.

**Must implement:** 32-byte padding (R2–R4), user/owner validation, file-key derivation, per-object key derivation (above); crypt filters incl. `/Identity`; **what is/isn't encrypted** (strings + streams yes; strings inside ObjStm not re-encrypted; `/Encrypt` dict, `/ID`, XRef streams never encrypted — the hardest part); permission flags `/P` (bits 3–12, advisory — expose, don't enforce for extraction); ciphers RC4 + AES-128/256-CBC (PKCS#7, 16-byte IV prepended); **SASLprep (RFC 4013)** for R6 Unicode passwords; empty-user-password case.

**Read in M1; write in M3** (RC4-128/AES-128/**AES-256 R6 only**). Order on save: **compress then encrypt**. Cross-check against Acrobat- and `qpdf`-produced fixtures; fuzz `fuzz_decrypt`.

#### 8.4.1 JBIG2 / JPX degradation contract & scope subset (resolves critique #5)

JBIG2 and JPX are **P2, may-trail, with a documented coverage subset** — not a blanket "fully supported" claim.

- **Documented coverage subset (v1 target).** JBIG2: **generic region + symbol dictionary + text region** (the combinations that cover the overwhelming majority of scanned PDFs); halftone/refinement regions are **best-effort, may be unsupported**. JPX: **baseline JP2/J2K with the common color spaces (gray, sRGB, YCC) at 8/16-bit**; exotic component transforms / extended capabilities are **best-effort**. The exact supported feature list is recorded in `docs/codec-coverage.md` and asserted by `JBIG2-COV-*`/`JPX-COV-*`.
- **Degradation contract (normative).** On any decode failure (unsupported region type, codec error, resource cap hit): return a **typed error** (`PdfDecodeError`/`PdfUnsupportedError`) **for that image only**; **never panic**; and **text extraction and the rest of the document continue unaffected** (a scanned JBIG2 page that can't be decoded yields *no Pixmap* but the document still opens, page_count is correct, and any text layer extracts). For the RAG persona this turns a "silent text-extraction failure" into an explicit, catchable signal: `extract_image`/`get_pixmap` raise, while `get_text` returns whatever text exists.
- **Fallback plan (concrete).** If `hayro-jbig2`/`hayro-jpeg2000` (both pre-1.0, R2/R3) cannot decode a real-world file: (1) cross-validation is **against multiple independent oracles** (pdfium-render and pdf.js outputs, both permissive) — **not** solely oxidize-pdf, whose JBIG2 correctness is itself unestablished; (2) failures are catalogued as known-coverage-gaps in `docs/codec-coverage.md`; (3) the **OpenJPEG (BSD-2) optional fallback for JPX is gated behind a non-default `jpx-openjpeg-c` feature** — and we **explicitly flag the tension**: enabling it pulls a C dependency that **breaks** the pure-Rust / `#![forbid(unsafe_code)]` / WASM-clean selling points. Therefore the default wheel **never** enables it; it exists only for downstream users who knowingly trade purity for coverage, and is documented as such.
- **No silent success.** A partially-decoded image is **never** returned as if complete; partial decode → typed error.

### 8.5 Fonts (`pdf-fonts`, mapping only — no rasterization in v1)

Requirements differ by goal; v1 needs **glyph-code → Unicode** + widths/metrics, **not** outlines.

**Font types:** Type1/MMType1/TrueType (simple, 1 byte/code), Type3 (CharProcs — widths via `/FontMatrix`×`/Widths`, Unicode via Encoding/ToUnicode, **not** executed for text), Type0/CID (composite; `/Encoding` = CMap; descendant CIDFontType0/2 + `/CIDToGIDMap`).

**Machinery:**
- **Encodings** — Standard/WinAnsi/MacRoman/PDFDoc/Symbol/ZapfDingbats (from ISO 32000 Annex D, public-domain facts, generated programmatically) + `/Differences`.
- **ToUnicode CMap** — `beginbfchar`/`bfrange` (increment + array forms), codespace-driven variable-length decode, surrogate pairs, multi-char (ligature) mappings, `usecmap` chaining. Authoritative when present; frequently missing/wrong → fallback ladder.
- **Glyph-name → Unicode** — Adobe Glyph List (vendored w/ notice — license gating item §6.5 #2) + algorithmic rules (`uniXXXX`, `uXXXXX`, `_`-split ligatures, `.`-suffix strip, `gNN`/`.notdef` → unresolved).
- **Predefined CJK CMaps** — Adobe-Japan1/GB1/CNS1/Korea1 ROS + `…-UCS2` tables (vendored w/ attribution — license gating item §6.5 #2).
- **Resolution ladder:** ToUnicode → encoding+AGL → name-pattern → predefined CMap→UCS2 → font cmap reverse → U+FFFD (or CID with `TEXT_CID_FOR_UNKNOWN`).
- **Widths** — simple `/Widths`+`/FirstChar`+`/MissingWidth` (+ Core-14 AFM for unembedded standard 14 — license gating item §6.5 #2); Type0 `/W`+`/DW`. Defensive: clamp absurd/NaN/negative → 0; never index OOB.
- **FontDescriptor `/Flags`** → span flags: **bit0 superscript (layout-set), bit1 italic, bit2 serif, bit3 mono, bit4 bold** (matching PyMuPDF's integer flag semantics, which are documented Tier-A facts).

#### 8.5.1 Reconciling the size KPI with full-embedding-by-default (resolves critique #15)

The apparent conflict between "output stays small (US-4)" and "full embedding by default (§3.2 #9)" is resolved by **clarifying what each claim covers**:
- **US-4 / merge size** is about **deduplication, not subsetting**: `insert_pdf` + GC-3/4 ensure a font that appears in N merged source documents is stored **once**, not N times. That is the size win we promise for merge, and it does not require subsetting.
- **`insert_text` with a large CJK TTF** is explicitly **not** covered by a "small output" promise in v1. The v1 default is **full embedding**, which *does* bloat output for large fonts; this is a **documented, accepted v1 tradeoff**. The KPI table (§14) is corrected so the size KPI applies to **merge dedup**, not to authored-text-with-CJK output.
- **Subsetting** (the real fix for authored-CJK size) is **P2, feature-gated** behind `subset` via `allsorts`, may trail v1, and when enabled produces `ABCDEF+`-tagged subsets. When it ships, the size promise extends to authored text.

#### 8.5.2 `allsorts` single-point-dependency fallback (resolves critique #15)

`allsorts` is the only realistic pure-Rust subsetter, making it a single point of failure for the P2 subsetting feature. Policy: **subsetting is best-effort with a defined fallback** — if `allsorts` cannot subset a given CFF/CID font (returns error or produces an invalid table), oxide-pdf **falls back to full embedding of that font** (the correct larger output), emits a `Warning{kind: SubsetFallback}`, and continues. Subsetting failure is therefore **never** a hard error and **never** produces a broken font. Tested by `SUBSET-FALLBACK-*`.

### 8.6 Text extraction & search (`pdf-text`)

Strict pipeline (each arrow independently testable): **content stream → `RawGlyph` list → layout grouping → `TextPage` → serializers / search.**

#### 8.6.1 Coordinate & page transforms — fully specified (resolves critique #18)

PyMuPDF coordinates: origin **top-left, y down**. PDF user space: bottom-left, y up. The complete device transform is composed and **the `/Rotate` handling is given explicitly** (the single most common text-extraction bug). Let the page MediaBox be `mb = [x0, y0, x1, y1]`, width `w = x1 - x0`, height `h = y1 - y0`, and let `r = /Rotate mod 360 ∈ {0,90,180,270}`.

**Step 1 — text rendering matrix (device-independent, row-vector PDF convention):**
```
params = Matrix(Tfs·Th, 0, 0, Tfs, 0, Trise)
Trm    = params · Tm · CTM        // applied to row vectors [x y 1]
```
This yields a glyph origin in **PDF user space** (y-up, MediaBox-relative).

**Step 2 — page transform to PyMuPDF device space.** Apply, **after** CTM, the page transform `P_r` chosen by rotation. Each matrix translates the MediaBox origin to (0,0), flips/rotates, and lands in a top-left/y-down space whose extent matches the *rotated* page:

```
r = 0:    P_0   = [ 1   0   0  -1   -x0      y1 ]      // flip y, origin→top-left;   page is w × h
r = 90:   P_90  = [ 0   1   1   0   -y0     -x0 ]      // rotate 90° CW;             page is h × w
r = 180:  P_180 = [-1   0   0   1    x1     -y0 ]      // rotate 180°;               page is w × h
r = 270:  P_270 = [ 0  -1  -1   0    y1      x1 ]      // rotate 270° CW;            page is h × w
```
(Matrices are `[a b c d e f]` row-vector form: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.) The final device coordinate of a glyph is `[x y 1] · Trm · P_r`. The `MediaBox`-origin translation (`-x0`/`-y0`/`x1`/`y1` terms) is **baked into each matrix** so a non-zero MediaBox origin is handled correctly. `page.rect` reports `w × h` for `r ∈ {0,180}` and `h × w` for `r ∈ {90,270}`, matching PyMuPDF. These four matrices are asserted byte-for-byte by `COORD-ROT-{0,90,180,270}-*`, and round-trip device→user inversion is property-tested.

#### 8.6.2 Interpreter, layout, model, serializers, search

- **Content-stream interpreter** — graphics state subset (`q/Q/cm/gs`), text object (`BT/ET`), all text-state ops (`Tc Tw Tz TL Tf Tr Ts`), positioning (`Td TD Tm T*`), showing (`Tj TJ ' "`), color ops → packed sRGB, `Do` form-XObject recursion (depth cap 16 + cycle visited-set), **inline-image (`BI…ID…EI`)**: for text extraction the inline image is **skipped** (find real `EI` robustly, accounting for binary data that may contain `EI`-like bytes by using the declared filter/length where present and a tolerant scan otherwise) — **but the inline image is captured into the image inventory** so `get_images`/`extract_image` and redaction PIXELS mode can act on it (§7 inline-image rows). Word spacing `Tw` only on single-byte code `0x20`; `TJ` numeric offsets drive word-gap detection; vertical writing (wmode 1) via `/W2`/`/DW2`.
- **Per-method default flag sets (pinned — resolves critique #10).** Shape parity requires exact `TEXTFLAGS_*` defaults per method. v1 pins them to PyMuPDF's documented per-method defaults and records them in `COMPAT.toml`:
  - `text`/`blocks`/`words`: `PRESERVE_LIGATURES | PRESERVE_WHITESPACE | MEDIABOX_CLIP` (images off).
  - `dict`/`rawdict`/`json`/`rawjson`: `PRESERVE_LIGATURES | PRESERVE_WHITESPACE | PRESERVE_IMAGES | MEDIABOX_CLIP` (images on).
  - `html`/`xhtml`: include images; `xml`: char-level, ligatures preserved.
  Any deviation from these defaults is a documented `COMPAT.toml` entry, not a silent difference. Asserted by `TEXTFLAGS-DEFAULT-*`.
- **Layout** — glyphs→spans (split on font/size/color/render-mode/flags/baseline/dir change)→lines (shared baseline)→blocks (vertical proximity + column overlap); **reading order**: content order for dict/blocks (sort by `(y1,x0)` when `sort=True`), XY-cut column detection re-synthesized for text/words; super/subscript (flags bit0); word segmentation (whitespace incl. NBSP `0xA0` + spatial gap ≈ 0.2–0.3× space width, exact threshold pinned in §14.5); rotated/vertical/RTL (visual order + bidi tag); dehyphenation (`TEXT_DEHYPHENATE`).
- **`TextPage` model** mirrors the PyMuPDF dict **key names and nesting** (Tier-A documented shape): block{number,type,bbox,lines}; line{spans,wmode,dir,bbox}; span{size,flags,char_flags,font,color(sRGB int),alpha,ascender,descender,origin,bbox,text|chars}; char{origin,bbox,c,synthetic}. Any field PyMuPDF docs do **not** specify exactly is defined by us as an intentional shape and recorded in `COMPAT.toml` (Tier-B handling, §6.1).
- **Serializers (shape parity per §6.1 Tier-A):** `text`, `blocks` `(x0,y0,x1,y1,"lines",block_no,block_type)`, `words` `(x0,y0,x1,y1,word,block_no,line_no,word_no)`, `dict`/`json`, `rawdict`/`rawjson` (per-char), `html` (positioned), `xhtml` (semantic), `xml` (char-level); plus `get_textbox`, `get_text_selection`. The **byte-exact** serialization of `html`/`xhtml`/`xml` is a Tier-B concern: we produce a **valid, documented** serialization and assert it against our own goldens + a structural validity check, **not** against PyMuPDF's exact bytes.
- **`search_for`** — normalized flat stream (case-fold, collapse whitespace, optional dehyphenate); all hits (no `hit_max`); whole-word option; cross-line/hyphenated; per-line quad (UL,UR,LR,LL) or merged rect (overlapping same-line rects joined); `clip` filter.
- **Edge cases (must-handle):** invisible text (Tr 3) **extracted**; white-on-white extracted (color reported, not gating); overlap dedup (shadow/faux-bold); encrypted strings already decrypted by core; broken widths → never panic; render-mode 7 (clip) extracted; combining marks (width 0) placed.
- **`TEXT_*` flags** — PRESERVE_LIGATURES/WHITESPACE/IMAGES/SPANS, INHIBIT_SPACES, DEHYPHENATE, MEDIABOX_CLIP, CID_FOR_UNKNOWN.

### 8.7 Document/page editing & saving (`pdf-edit`)

**Core design:** copy-on-write overlay over original bytes — `Document` holds `original_bytes: Arc<[u8]>` + `xref` + `changed` overlay + `next_free_objnum` + `parse_was_repaired` flag (§8.2). Reads fall through to original; writes land in overlay. Enables incremental save and tractable GC.

- **Serializer** — canonical syntax; name `#xx` / string literal-vs-hex escaping; exact `/Length` (direct or indirect); dates `D:YYYYMMDDHHmmSS±HH'mm'`; round-trip-tested (`parse(serialize(obj)) == normalize(obj)`).
- **Object-edit API** — `get/create/update/delete_object`, `update_stream`, `xref_get_key`/`xref_set_key` (set Null deletes key), `get_new_xref`, `intern` dedup, `resolve` (hop-capped).
- **Full save** — classic xref **and** xref-stream/object-stream authoring; dense renumber (garbage≥2); `/ID` (stable first, fresh second); free-list correctness (obj 0 gen 65535 head); `/Size` = max+1; offsets computed post-header.
- **Garbage collection 1–4** — (1) mark-and-sweep from Root/Info/Encrypt/ID; (2) compact/renumber + rewrite refs; (3) dedup identical objects (structural-hash fixpoint, exclusion list §8.7.1); (4) dedup identical streams. ObjStm survivors re-packed.
- **Stream deflation** — `deflate`/`deflate_images`/`deflate_fonts`/`use_objstms`; skip DCT/already-filtered; compress-then-encrypt.
- **Incremental save — clean-parse precondition (resolves critique #11).** Append-only; `out[..orig.len()] == orig` byte-exact; new xref `/Prev` = prior `startxref`; match original xref style. **Incremental save is valid ONLY when `parse_was_repaired == false`** (no reconstruction, `header_offset == 0`, no `/Length`/`startxref`/xref corrections). If `parse_was_repaired == true`, `save_incremental()` **rejects** with `PdfUnsupportedError("incremental save requires a clean parse; use full save")`, and `can_save_incrementally()` returns `false`; callers may opt into automatic upgrade to full save via `SaveOptions{ on_repaired: Upgrade | Reject }` (default `Reject` to avoid silently breaking a signature-preservation expectation). For **signed PDFs**: because a repaired/`header_offset≠0` file no longer has trustworthy original byte offsets, incremental-over-repaired would corrupt the `/Prev` chain and invalidate signatures — hence the hard rejection. On a **clean** parse, `out[..orig.len()] == orig` holds exactly and existing signature byte ranges remain valid. Also rejects incremental + garbage/linearization as before.
- **Save with encryption** — RC4-128/AES-128/AES-256 R6 (never R5, §8.4).
- **Page ops** — `new_page`, `insert_page`, `delete_page(s)`, `copy/fullcopy_page`, `move_page`, `select`, box setters (cropbox ⊆ mediabox), `set_rotation`/`remove_rotation`, n-up (source pages → Form XObjects + placement matrices). Flatten page tree to single-level `/Kids` on first edit (default; round-trip implications in §15 Q11 → resolved: flatten is the v1 default and is round-trip-safe because inherited attributes are materialized onto leaves).
- **`insert_pdf`** — transitive-closure deep copy + fresh-number renumber map (rewrite internal refs) + dedup-against-dst (`pdf_graft`-style) + materialize inherited attributes onto copied leaves + `/Parent` repoint + drop broken cross-range links + re-encrypt on save.
- **Metadata** — Info dict + XMP (`set_metadata` mirrors common keys into XMP; never silently desync); `get_xml_metadata`/`set_xml_metadata` expose the raw XMP packet; set our `/Producer`.
- **TOC / outline** — `get_toc` walks `/First`/`/Next`/`/Down` (+ page labels per §3.5); `set_toc` rebuilds tree (`/First /Last /Next /Prev /Parent /Count` signed); reject level jumps; named destinations via `/Names /Dests` name tree (kept sorted), resolved to **physical** page indices even under non-trivial `/PageLabels`.
- **Save surface:** `save`/`ez_save` (garbage=3,deflate=1)/`write`/`tobytes`/`save_incremental`/`can_save_incrementally`.

#### 8.7.1 GC level-3 dedup exclusion rule (resolves critique #23)

Structural-hash dedup (GC-3) **must not** merge objects whose identity is semantically load-bearing or whose later divergence under COW would corrupt the file. The **exclusion list is explicit and normative**:

- **Never deduped (by `/Type` or role):** `/Page`, `/Pages`, `/Annot`, `/Widget`, any object referenced from `/AcroForm` field tree, `/Catalog`, `/Encrypt`, the two `/ID` strings, any object that is the target of a **named destination** or an **outline `/Dest`**, any object carrying a `/StructParent`/`/StructParents` (tagged-PDF identity), and any **stream** with side-effecting semantics (`/Type /ObjStm`, `/Type /XRef`, signature `/V` value streams).
- **Dedup-eligible:** leaf value objects with no identity role — e.g., identical font descriptors, identical `/Resources` *sub-dictionaries that are not shared-then-mutated*, identical image XObjects (by content hash), identical Type1/TTF font streams.
- **COW-safety rule for shared objects.** When two references are merged to a single object, that object is marked **`shared = true`**. Any subsequent edit that would mutate a `shared` object triggers **copy-on-write unsharing** (clone, edit the clone, repoint only the editing reference) so divergence after merge is impossible. This makes "two pages sharing a Resources dict that later diverges" safe by construction. Tested by `GC3-EXCLUDE-*` and `GC3-COW-*`.

### 8.8 Annotations / forms / redaction (`pdf-edit`)

**Content emission (shared):** `insert_text`/`insert_textbox` (Base-14 via AFM metrics for wrapping; full TTF/OTF embedding with `/ToUnicode`, CID Type0; subsetting feature-gated via `allsorts` with full-embed fallback §8.5.2); `insert_image` (JPEG passthrough `/DCTDecode` / Flate / PNG-alpha→`/SMask` / palette→`/Indexed`; correct flip matrix); `draw_*` + `Shape` (batched `q…Q`, circles = 4 cubic Béziers κ=0.5523, even-odd vs nonzero); `insert/update/delete_link`.

**Annotations:** full `add_*_annot` family + `Annot` (`update`, colors/opacity/border/flags/info, line-ends, `set_rect`, popup). **`/AP /N` appearance stream generated for every subtype** (the load-bearing portability requirement); QuadPoints in **Acrobat order** for Highlight/Underline/Squiggly/StrikeOut; `/AP` BBox+Matrix map into `/Rect`; `/CA` reflected via `/ExtGState`; delete cleans `/AP`/`/Popup`/embedded files.

**Vector path extraction:** `Page.get_drawings()` / `get_cdrawings()` (resolves critique #10) — extract path-construction/paint operators into structured path items (type, points, fill/stroke color, width, even-odd flag). Required by redaction line-art modes (below) and exposed publicly for pdfplumber-style line/table analysis. P1/M4.

**Redaction (`apply_redactions`) — multi-surface destructive guarantee (resolves critique #14):** **destructive** — not "draw a black box." The redaction pipeline scrubs **every** content surface that can carry a redactable secret, and the **acceptance gate runs over the fully-decompressed file**, not the compressed save (a byte-grep over a compressed save is a false pass and is explicitly forbidden as the gate).

Surfaces redaction **must** scrub when a redaction rect overlaps them:
1. **Page text** — interpreter-driven glyph removal on any overlap (split `Tj` / adjust `TJ` to preserve survivors' positions; survivors unshifted).
2. **Image content under the rect** — modes `PIXELS` (decode → blank the overlapped pixel region → **re-encode**) and `NONE`/`REMOVE` (drop the image). For **JBIG2/JPX** images, PIXELS requires **decode → blank → re-encode to Flate/JPEG** (transcode), reusing the §8.4.1 decode path; **if that image cannot be decoded, redaction of that image fails closed** — the redaction call raises `PdfRedactionError` (never silently leaves the secret pixels) so the caller must choose `REMOVE` for that image. This closes the "scanned text under a redaction rect" gap.
3. **Vector/line-art under the rect** — clipped/removed using `get_drawings()` geometry (line-art modes).
4. **Annotation and form-field text** overlapping the rect.
5. **Other enumerated surfaces** that survive a naive page-content scrub and must be addressed: **document metadata / XMP**, **annotations not authored by us that fall in the rect**, **embedded files / `/JS` referenced from redacted content**, and **object streams on the redacted page** — these are decompressed and checked by the gate (below) and scrubbed where they fall in scope.
6. **Force full rewrite** — redaction **rejects incremental save** (full rewrite only) to prevent pre-redaction bytes leaking in an appended revision; remove `Redact` annotations after applying.

**Redaction acceptance gate (corrected — resolves the false-pass bug):** after `apply_redactions()` + full save, the test harness (a) **fully decompresses every stream and object stream** in the output (Flate/LZW/etc. expanded; ObjStms unpacked), then (b) runs the secret byte-grep over that **decompressed** corpus AND over decoded text (`get_text()`), AND (c) for image-bearing redactions, verifies the redacted pixel region is blanked in the re-encoded image. A secret found in *any* decompressed surface is a **gate failure**. Note: this gate does **not** claim to scrub secrets embedded inside arbitrary third-party font glyph programs or `ToUnicode` of fonts we did not author when those fonts are *also used elsewhere on non-redacted pages*; that limitation is **documented** (PyMuPDF shares it) — but font glyph names / `ToUnicode` for glyphs that appear **only** under the rect are scrubbed.

**Forms (AcroForm) + `Widget` API (resolves critique #10):** read field tree + FQN (parent chain); set text/checkbox/radio/choice; **appearance regeneration** (`NeedAppearances=false`); checkbox on-state discovered from `/AP /N` keys (not assumed `/Yes`); radio group `/V`; flatten (render widget `/AP` into page content as Form XObject, remove `/AcroForm` + widgets); signatures read-only. The **`Widget` object** is a first-class API (distinct from raw field-dict access) exposing `field_type`, `field_value`, `field_name`, `field_label`, `rect`, `xref`, `text_maxlen`, `choice_values`, `field_flags`, `update()` — matching PyMuPDF's documented `Widget` surface. Tested by `WIDGET-*`.

**Embedded files / attachments** (`embfile_*`, `/Type /EmbeddedFile`, `/Params /Size /CheckSum`); `bake`; `scrub`.

### 8.9 Metadata / TOC / links

Covered in §8.7 (Info+XMP consistency incl. `get/set_xml_metadata`, `get_toc`/`set_toc` tree build with signed `/Count`, page-label-aware TOC §3.5, named destinations → physical page, link read/insert/update/delete; broken-link cleanup in GC). String encodings: PDFDocEncoding, UTF-16BE (BOM `FE FF`), **UTF-8 (PDF 2.0)** — auto-detect on read, choose correctly on write; tolerant date parsing.

### 8.10 Image-document support (`pdf-image`)

`ImageDocument` implements the same `Document`/`Page` traits as PDF. Formats: PNG (all depths/palette/alpha/interlace), JPEG (baseline+progressive; header read without full decode for passthrough), **multi-page TIFF (one page per IFD)**, GIF, BMP, WEBP.

- **Page model:** `/MediaBox` = pixel size × DPI (default 72; honor format resolution tags); content `q w 0 0 h 0 0 cm /Img Do Q`.
- **`convert_to_pdf` (image inputs only):** JPEG → `/DCTDecode` passthrough (lossless, tiny); PNG/TIFF → decode → Flate; alpha → `/SMask`; palette → `/Indexed`; CMYK → `/DeviceCMYK` + Adobe `/Decode` inversion; 16-bit → BPC 16; honor EXIF/TIFF orientation (bake into matrix). Non-image inputs → `PdfUnsupportedError` (§3.2 #2).
- **`Pixmap` (in-scope render path, per §3.3):** for image documents and **image-only PDF pages**, decode → `Pixmap { width, height, n, alpha, stride, samples, colorspace }`; for image-only PDF pages the image XObject is decoded and resampled by its placement CTM at the requested DPI. This is "render" for the image path (no vector rasterization). Vector pages → `PdfUnsupportedError`. Codecs DCT/CCITT/JBIG2/JPX feature-gated, resource-capped, fuzzed, untrusted; cross-validated against **multiple oracles** (pdfium-render + pdf.js), with the degradation contract of §8.4.1.
- **Extraction:** `extract_image`/`extract_font`/`get_char_widths`.

### 8.11 Rendering subsystem (deferred, M6, post-v1)

Reserved crate slot `pdf-render`; nothing above it depends on it. Post-v1 goal: PyMuPDF-class `get_pixmap`/`get_svg_image`/`DisplayList` for **vector pages** (full graphics interpreter, font rasterization via `swash`/`fontdue`/`ttf-parser` + Type3 interpreter, ICC color via `moxcms` + PDF Functions 0/2/3/4, shadings 1–7, transparency groups/blend modes/soft masks, patterns/tiling). **Strategy: depend on the `hayro` interpreter/renderer (Apache/MIT) as the "very large leaf"** rather than writing a rasterizer from scratch; keep `pdfium-render` as an optional non-default backend and differential oracle. Until then, vector `get_pixmap` raises `PdfUnsupportedError`; the **image-only page path of §3.3 already works in v1** and does **not** wait for M6.

---

## 9. Technical Architecture & API Design

### 9.1 Cargo workspace & crate DAG

| Crate | Responsibility | First-party safety | Public? |
|---|---|---|---|
| `pdf-core` | object model, lexer/parser, xref (table+stream), trailer, repair, filters, writer, `DocumentStore`. No domain logic. | `#![forbid(unsafe_code)]` | yes |
| `pdf-crypto` | Standard security handler (RC4/AES-128/256), `/Encrypt` parsing, KDF, permissions. | `#![forbid(unsafe_code)]` | yes |
| `pdf-fonts` | font parsing for mapping, encodings/ToUnicode/CMap, widths/metrics, glyph→Unicode. **No rasterization.** | `#![forbid(unsafe_code)]` | yes |
| `pdf-text` | content-stream interpreter (text), `TextPage`, all `get_text` formats, search. | `#![forbid(unsafe_code)]` | yes |
| `pdf-edit` | page ops, `insert_pdf`, annotations/links/forms, content emission, metadata/TOC, redaction. | `#![forbid(unsafe_code)]` | yes |
| `pdf-image` | image-doc support, image-XObject decode/encode, `Pixmap`. | `#![forbid(unsafe_code)]` (first-party; codec deps contain `unsafe`) | yes |
| `pdf-render` | *(future)* vector rasterizer → Pixmap. Slot reserved. | `#![forbid(unsafe_code)]` (first-party) | yes (future) |
| `pdf-api` | unified ergonomic facade / re-exports; the only crate `py-bindings` touches. | `#![forbid(unsafe_code)]` | yes (primary) |
| `py-bindings` | PyO3 `#[pyclass]` wrappers over `pdf-api`; builds `_core` cdylib; zero PDF logic. | **`unsafe` allowed only here**, audited (buffer protocol §9.4) | ext module |
| `pdf-fuzz` | `cargo-fuzz` targets. | n/a | internal |
| `pdf-testdata` | corpus loader + golden helpers. | n/a | internal (dev) |

```
                  py-bindings  (PyO3 cdylib → _core.abi3.so)
                       │  (depends on exactly one core crate)
                       ▼
                    pdf-api      facade / re-exports
        ┌──────────┬───┴────┬──────────┐
        ▼          ▼        ▼          ▼
    pdf-text   pdf-edit  pdf-image  pdf-render(fut)
        │          │        │          │
        └────┬─────┘        │          │
             ▼              │     (pdf-fonts, pdf-text)
         pdf-fonts ◄────────┘
             ▼
         pdf-core  ◄─────────  pdf-crypto   (core uses crypto behind `encryption` feature)
```

**CI-enforced rule:** no crate depends on a sibling in its own layer except through `pdf-core` types; `pdf-text` and `pdf-edit` may share `pdf-fonts` but not each other. Rationale: compile-time parallelism, isolated TDD (independently red/green-able units), reuse of `pdf-crypto`/`pdf-fonts`, and a single stable FFI chokepoint.

### 9.2 Core data model (Rust)

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ObjRef { pub num: u32, pub gen: u16 }

pub type Dict = std::collections::BTreeMap<Name, Object>;   // ordered → deterministic output

#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Null, Boolean(bool), Integer(i64), Real(f64),
    String(PdfString),              // raw bytes + kind (Literal|Hex)
    Name(Name),                     // interned
    Array(Vec<Object>),
    Dictionary(Dict),
    Stream(StreamObj),              // dict + out-of-line StreamData (Raw{off,len}|Encoded|Decoded)
    Reference(ObjRef),              // flat graph — children are refs, not boxed values
}
```

`DocumentStore` owns `source: Source(Mmap|Owned|Empty)`, a lazily-filled `RwLock<HashMap<u32, Arc<Object>>>` arena, `XrefTable`, `trailer`, optional `Decryptor`, `NameInterner`, `version`, `header_offset`, `parse_was_repaired`, a `ChangeSet`, and `Limits`. **`Page = { doc: Arc<DocumentStore>, index, page: ObjRef }`** — fully owned, `'static`, FFI-safe (no borrows cross the boundary).

**Memory-model decisions:**

| Concern | Decision | Tradeoff |
|---|---|---|
| Object sharing | `Arc<Object>` + integer handles (not `&'a`/`Rc`) | one atomic refcount/resolve; enables zero-lifetime FFI + `Send+Sync` |
| Backing bytes | mmap default, owned `Bytes` for in-memory; **mmap disabled on WASM & opt-out** (§9.6.1) | low RSS; lifetime by `Arc` refcount, not Rust lifetimes |
| Edits | interior mutability + `RwLock` + `ChangeSet` | lock cost on writes only; keeps `Document: Sync`; basis of incremental save |
| Stream payloads | `bytes::Bytes`, lazy-decode + LRU cache bounded by `Limits` | first-touch decode cost |

### 9.3 Error handling

`thiserror` core `Error` enum (`Io`, `InvalidHeader`, `Syntax{offset,msg}`, `Xref`, `MissingObject`, `ReferenceCycle`, `Unsupported(&'static str)`, `Filter`, `Decode`, `Crypto`, `NeedsPassword`, `MissingId`, `Redaction`, `LimitExceeded(LimitKind)`, `Recovered(Vec<Warning>)`); sub-crates have focused enums flattened by `pdf-api`. **Strict vs Lenient** modes (§8.2). **Rust→Python mapping** via `From<pdf_api::Error> for PyErr`: a `_core.PdfError` base with `PdfSyntaxError`/`PdfPasswordError`/`PdfUnsupportedError`/`PdfDecodeError`/`PdfRedactionError`/`PdfLimitError` subclasses, plus standard `PyFileNotFoundError`/`PyOSError`; the `fitz` shim aliases PyMuPDF names (`FileDataError`, `EmptyFileError`, …). **Error-message i18n:** messages are **English-only, stable, machine-greppable** strings with a stable `kind` discriminant; localization is explicitly out of scope, but the stable `kind` lets downstream code branch without parsing prose (avoids the "error prose is expressive AGPL output" trap — our messages are our own).

### 9.4 PyO3 binding strategy

- **Handle/index pattern** — every `#[pyclass]` is `'static`, holds an `Arc` (or index), never a Rust borrow. `PyPage` carries its own `Arc` clone, not a `Py<PyDocument>` parent reference.
- **GIL release** — all CPU-heavy work (`open`, `save`, text extraction, search, decode, font parse) runs inside `py.detach(|| …)`; trivial getters do not. Core types are `Send` → multiple Python threads process different docs on different cores.
- **Zero-copy buffer-protocol lifetime contract (resolves critique #13).** `Pixmap` implements the buffer protocol (`__getbuffer__`/`__releasebuffer__`) so `memoryview(pix)` / `np.frombuffer` / `Image.frombuffer` read straight from Rust memory. The **lifetime contract is enforced, not merely documented**: (a) the backing pixel buffer lives in an `Arc<[u8]>` inside the `Pixmap`; (b) `__getbuffer__` increments an **export count** and **clones the `Arc` into the `Py_buffer.internal`** so the bytes outlive the `Pixmap` Python object even if the latter is GC'd while a view is alive; (c) `__releasebuffer__` drops that `Arc` clone and decrements the count; (d) **any `Pixmap` mutator** (e.g., `clear`, `invert_irect`, in-place ops) checks the export count and, if > 0, performs **copy-on-write** into a fresh `Arc` rather than mutating bytes a live view points at — so a `memoryview`/`numpy` array can never observe a mutate-under-view or use-after-free. Tested by `PIXMAP-BUF-LIFETIME-*` (hold a `memoryview`, drop the `Pixmap`, mutate, read the view). `pix.samples` (`bytes`, an owning copy) is also offered for callers who want zero lifetime concerns.
- **`get_text("dict"/"rawdict")`** — heavy content-stream parse under `py.detach` → plain Rust `TextPage`; only the cheap final Rust→Python object construction holds the GIL, reusing interned keys; bbox returned as tuples (matching PyMuPDF).
- **abi3 vs free-threaded — distinct artifacts (resolves critique #22).** The **GIL build** ships as a single `abi3-py310` wheel per (OS, arch) covering CPython ≥3.10 (this is the *only* claim that is true for one artifact). The **free-threaded (PEP 703) build is a *separate* wheel** — abi3 historically does **not** cover the free-threaded ABI in the same artifact — built when the PyO3/abi3t support matrix is green. **Support pinning:** free-threaded support is gated on **PyO3 ≥ the first release whose changelog declares stable free-threaded + abi3t support** (tracked in Open Question §15 Q4 alongside MSRV; not on the v1 critical path). The two builds are never conflated in one wheel.

### 9.5 The `fitz`-compat shim

```
oxide_pdf/      native package (Rust-backed, idiomatic Python) — _core.abi3.so + thin wrappers
fitz/           compat package: re-exports + geometry value types + exception aliases + constants
pymupdf/        alias: from fitz import *
```

Mostly pure Python over `_core` (cheap to express PyMuPDF's huge surface + deprecated aliases; iterate without recompiling). Geometry (`Point/Rect/Matrix/Quad/IRect`) are pure-Python value types matching PyMuPDF arithmetic exactly (arithmetic semantics are Tier-A documented facts). Documented deviations (vector rendering deferred → `PdfUnsupportedError`; dict numeric *exactness* → tolerance per §14.5; html/xhtml/xml byte-exactness is `oxide-pdf`-defined per §6.1; Story/OCR out of scope) encoded in machine-readable `COMPAT.toml`; unimplemented-but-known methods raise `PdfUnsupportedError` with a matrix link, never `AttributeError`.

### 9.6 Security / fuzzing / robustness

- `#![forbid(unsafe_code)]` in **all first-party crates** except `py-bindings` (audited buffer-protocol `unsafe`); `cargo-geiger` tracks unsafe surface across the **whole tree including deps** (not just first-party).
- **`cargo-fuzz` targets from M1:** `fuzz_open`, `fuzz_lexer`, `fuzz_xref`, `fuzz_repair`, per-filter (`fuzz_flate`/`fuzz_lzw`/`fuzz_ascii85`/`fuzz_runlength`/`fuzz_predictor`), `fuzz_cmap`, `fuzz_content_stream`, `fuzz_get_text`, `fuzz_decrypt`, `fuzz_font_parse`, and (M5) per-codec `fuzz_dct`/`fuzz_ccitt`/`fuzz_jbig2`/`fuzz_jpx`. Invariant: never panic/OOM/hang.
- **`Limits` struct:** `max_file_size`, `max_objects`, `max_recursion_depth`, `max_decompressed_stream`, `max_total_decompressed`, `max_objstm_objects`, `max_decode_ratio`, `timeout`. Defends zip/decompression bombs (capped streaming readers + incremental ratio check), reference/recursion cycles (visited-set + depth counter), xref/object-count bombs (never pre-allocate from declared `/Size`), integer overflow (checked/saturating; offsets validated before slicing). **Shipped defaults are pinned in §15 Q10 → resolved in §9.6.2** so the "never OOM" gate is testable.
- **No-panic parsing** — clippy `unwrap_used`/`indexing_slicing` = deny in `pdf-core`/`pdf-fonts`/`pdf-crypto`; slicing via `get(..)`.

#### 9.6.1 Memory-safety claim scoped precisely + mmap-truncation UB handled (resolves critique #6)

**Precise claim (this replaces any "memory-safe engine" blanket statement):** *All first-party `oxide-pdf` Rust crates are `#![forbid(unsafe_code)]`. Memory-unsafety in `oxide-pdf` can therefore originate only from (a) the single audited `unsafe` block in `py-bindings` (buffer protocol, §9.4) and (b) third-party dependencies that contain or wrap `unsafe` (notably `memmap2`, `bytes`, `rustybuzz`, `ttf-parser`, and the image codecs).* We do **not** claim a transitively-unsafe-free tree. Dependency `unsafe` is mitigated, not ignored:

- **`cargo-geiger`** runs in CI over the **entire** dependency graph and the unsafe-surface count is tracked; a regression (new unsafe-bearing dep, or a jump in count) requires explicit reviewer sign-off.
- **Codec/font unsafe** is exercised by the per-codec/font fuzz targets (above) under `Limits`, treating those crates as untrusted attack surface (which they are).
- **mmap-truncation UB (the named #1-gate vector).** `memmap2` mmap is fundamentally `unsafe`: if the underlying file is **truncated by another process while an mmap is live**, reads can fault/UB — a real crash vector for a malformed-input library whose #1 gate is "never panic/OOM/hang." Mitigations, all normative: (1) **mmap is opt-in-able-out**: `OpenOptions{ mmap: Auto | Never }`, and `mmap` is **disabled by default for files opened from a path that is detected to be on a volatile/remote filesystem** is *not* something we can detect portably, so instead (2) **on any SIGBUS/EXC_BAD_ACCESS-prone read we never index raw mmap memory directly** — all access to the mmap goes through bounds-checked `get(..)` against the length captured **at open time**, and the captured length is the authority (a later truncation cannot make us read past the original length because we never re-query the OS length); (3) for the residual OS-level truncation-fault risk, the **documented hard-safe mode** `OpenOptions{ mmap: Never }` reads the file into an owned `Bytes` (no mmap at all) — this is the **recommended mode for untrusted/volatile inputs** and is the default in the security profile preset `OpenOptions::untrusted()`; (4) **WASM builds disable mmap entirely** (no mmap syscall) and always use owned `Bytes`. The "never panic/OOM/hang" fuzz gate runs in `mmap: Never` mode so it measures our code, not OS mmap faults; a separate nightly job fuzzes `mmap: Auto` on a tmpfs with concurrent-truncation injection to characterize the residual risk.

#### 9.6.2 Shipped `Limits` defaults (resolves critique #24 Q10)

Defaults are **pinned here** (overridable per `OpenOptions`) so M1's "never OOM" gate has concrete numbers to test:

| Limit | Default | Rationale |
|---|---|---|
| `max_file_size` | 4 GiB | i32-offset safety + practical ceiling |
| `max_objects` | 8,388,608 (2²³) | bounds xref/object-count bombs |
| `max_recursion_depth` | 256 | dict/array/XObject nesting |
| `max_decompressed_stream` | 1 GiB | single-stream bomb cap |
| `max_total_decompressed` | 4 GiB | whole-document decompression budget |
| `max_objstm_objects` | 1,048,576 | per-ObjStm member cap |
| `max_decode_ratio` | 200:1 | incremental ratio trip for zip bombs |
| `timeout` | none by default; `OpenOptions::untrusted()` sets 30 s | wall-clock guard for pathological inputs |

`OpenOptions::untrusted()` preset = `{ mmap: Never, timeout: 30s, strict_ratio: true }`. These defaults are asserted by `LIMITS-DEFAULT-*` and exercised by bomb fixtures.

### 9.7 Concurrency — enforced vs recommended (resolves critique #13)

Everything `Send + Sync` (`Object`, `DocumentStore`, `Page`, `Pixmap`, the `#[pyclass]` wrappers). **`Document` is concurrently readable, serialized on write** (`parking_lot::RwLock` arena: read mode on cache hits, brief write mode on cache fill; mutations take write lock).

**What is *enforced* (memory safety):** No data race is possible — every shared access goes through the `RwLock`/`Arc`, so concurrent read+write cannot corrupt memory. This is the "strictly safer than PyMuPDF (not thread-safe)" claim, and it is **literally true at the memory-safety level**.

**What is *not* enforced (logical correctness) — stated honestly:** The "shared-immutable for parallel reads, edits funneled through one thread" rule is a **documented usage convention, not a compiler-enforced invariant**. Specifically:
- A read concurrent with a write is **memory-safe** but its *result* is "whichever side won the lock" — i.e., you may read pre-edit or post-edit state. This is a normal `RwLock` ordering property, not a bug.
- **Derived/cached data staleness is a real footgun** and is handled, not hand-waved: a `TextPage` (or any cached derived structure) captures a **content snapshot version** at creation. Any document edit **bumps the `Document`'s content-version counter**; a derived object built from an older version is **invalidated** — using it raises `PdfStaleError` rather than silently returning stale text. This converts the "stale `TextPage` after edit" footgun into a typed, catchable error. Tested by `CONCURRENCY-STALE-*`.
- We therefore **scope the safety claim precisely**: *oxide-pdf is memory-safe under any thread interleaving (enforced); it is logically race-free only when callers follow the documented single-writer convention (recommended), and stale-derived-data is turned into a typed error rather than a silent correctness bug.*

No `async` in core (PDF work is CPU-bound).

### 9.8 Rust + Python API sketch (side by side)

```text
OPEN          R: let doc = Document::open("a.pdf")?;            P: doc = fitz.open("a.pdf")
OPEN UNTRUST  R: Document::open_with("a.pdf", OpenOptions::untrusted())?  P: fitz.open("a.pdf", untrusted=True)
OPEN BYTES    R: Document::open_memory(&b, OpenOptions{filetype:Some("pdf"),..})?  P: fitz.open(stream=b, filetype="pdf")
PAGE COUNT    R: let n = doc.page_count();                      P: n = doc.page_count            # len(doc)
LOAD PAGE     R: let p = doc.load_page(0)?;                     P: p = doc[0]
IS REPAIRED   R: let dirty = doc.is_repaired();                 P: dirty = doc.is_repaired
GEOMETRY      R: let r = p.rect(); let rot = p.rotation();      P: r = p.rect; rot = p.rotation
TEXT          R: p.get_text(TextFormat::Text, opts)?;           P: p.get_text()
DICT          R: p.get_text_dict(Detail::Spans)?;               P: p.get_text("dict")
SEARCH        R: p.search("invoice", SearchOptions{quads:true,..})?  P: p.search_for("invoice", quads=True)
DRAWINGS      R: p.get_drawings()?;                             P: p.get_drawings()
TOC           R: doc.get_toc(true)?; doc.set_toc(&toc)?;        P: doc.get_toc(); doc.set_toc(toc)
LABEL         R: p.get_label()?;                                P: p.get_label()
METADATA      R: doc.metadata(); doc.set_metadata(&m)?;         P: doc.metadata; doc.set_metadata(m)
XMP           R: doc.xml_metadata(); doc.set_xml_metadata(&x)?; P: doc.get_xml_metadata(); doc.set_xml_metadata(x)
MERGE         R: doc.insert_pdf(&src, InsertOptions{..})?;      P: doc.insert_pdf(src, from_page=0, to_page=4)
PAGE OPS      R: doc.new_page(None,595.,842.)?; doc.delete_page(3)?; doc.select(&[0,2,4])?
              P: doc.new_page(width=595,height=842); doc.delete_page(3); doc.select([0,2,4])
ANNOT         R: let a = p.add_highlight_annot(&quads)?;        P: a = p.add_highlight_annot(quads)
WIDGET        R: for w in p.widgets()? { w.field_value(); }     P: for w in p.widgets(): w.field_value
REDACT        R: p.apply_redactions(RedactOptions{images:Pixels,..})?;  P: p.apply_redactions(images=fitz.PDF_REDACT_IMAGE_PIXELS)
AUTH          R: if doc.needs_password() { doc.authenticate(b"pw")?; }  P: if doc.needs_pass: doc.authenticate("pw")
SAVE          R: doc.save("o.pdf", SaveOptions{garbage:4,deflate:true,..})?;
              R: if doc.can_save_incrementally() { doc.save_incremental()?; }   // false if repaired
              P: doc.save("o.pdf", garbage=4, deflate=True); doc.saveIncr()      # raises if repaired
PIXMAP        R: let pix = p.get_pixmap(&RenderOptions{dpi:200,..})?; let buf = pix.samples();
              //  image doc or image-only page → Pixmap; vector page → PdfUnsupportedError
              P: pix = p.get_pixmap(dpi=200); mv = memoryview(pix)   # vector page raises PdfUnsupportedError
```

---

## 10. TDD Methodology & Test Architecture

> **Normative.** Where this says MUST, CI fails on violation. **Clean-room invariant:** no test/fixture derived from MuPDF/PyMuPDF/AGPL; goldens are *our* validated outputs.

### 10.1 Workflow (spec-driven red→green→refactor→harden)

Per unit of work: **SPEC** (one-line traceability note `// ISO 32000-1 §7.4.4.2 …` — a test with no source of truth is rejected) → **RED** (tests fail for the right reason; paste red output in PR) → **GREEN** (minimum impl, no speculative generality) → **REFACTOR** (no behavior change) → **HARDEN** (property tests + fuzz seed for untrusted-byte functions; mutation check on changed files).

**Up-front decomposition.** A project-level **Test Case Catalog** (`docs/test-case-catalog.md`) enumerates *every* planned public function and internal algorithm into named, numbered cases (`FLATE-DEC-007`, `WORDS-031`) with `status: catalogued|written|red|green` — this is the required full decomposition (specification, not yet code). Granularity rule: one test case = one observable behavior / one input equivalence class (not one-per-line) — enumerated by spec branches and edge classes, which is finite and stable across implementation churn.

#### 10.1.1 The test-first workflow, made enforceable (resolves critique #7 — D1/D5 contradiction)

The prior "all milestone tests become one giant red PR before any implementation" is **abandoned** because it is (a) not provable in a squashed PR and (b) mutually exclusive with diff-coverage gating. Replaced by a concrete, CI-enforceable **two-state-per-test protocol**:

1. **Catalog first (cheap, no code conflict).** Every milestone test exists in `docs/test-case-catalog.md` with a status **before** that milestone's implementation work starts. `test-order-guard` checks that each *new test function* added in a PR has a matching catalog entry whose status was `catalogued` or `written` **in a prior merged commit** (git-history check, not within-one-PR).
2. **Tests land RED-but-`#[ignore]`-tagged, then go green in the next PR.** The enforceable rule that does **not** fight diff-coverage:
   - A **test PR** adds the test functions tagged `#[ignore = "RED: <CATALOG-ID> awaiting impl"]` (and Python `@pytest.mark.xfail(strict=True, reason="RED: …")`). Ignored/xfail tests **do not run as failures** and **do not introduce uncovered new public functions** (no implementation lands), so **D1 and D5 are both satisfiable** — there is no implementation code in the test PR, hence nothing for diff-coverage to flag.
   - The matching **implementation PR** (a) removes the `#[ignore]`/`xfail` tags for the catalog IDs it implements, (b) lands the implementation, and (c) is subject to D1 (those tests now green) **and** D5 (diff-coverage ≥90% on the new implementation lines). Because the tests and the impl arrive together-or-test-first, coverage of the new code is automatic.
   - `test-order-guard` enforces: an implementation PR may only **un-ignore** a catalog ID whose RED test was added in an **earlier merged commit** (proving test-precedes-impl across the git history), and may not introduce a new public function whose only tests are added in the *same* PR unless those tests were catalogued earlier. This is what makes "tests precede implementation" **mechanically checkable** without the impossible single-red-PR model.
3. **Milestone exit** requires **0 remaining `#[ignore]=RED` / `xfail` tags** for that milestone's catalog IDs (all RED tests have been driven green), checked by `catalog-status-guard`.

This reconciles D1 (every catalogued test for the feature green at merge of the *implementation* PR) with D5 (diff-coverage on the implementation PR), because the only PR that introduces implementation code is the one that also un-ignores its tests — there is never a PR that both introduces 0%-covered new public code **and** is blocked for it.

### 10.2 Test taxonomy

| Layer | Tool | Location | Asserts | Clean-room note |
|---|---|---|---|---|
| Unit | `#[test]` | `src/**/#[cfg(test)]` | per-function behavior & edge classes | trace to ISO clause |
| Integration | `#[test]` | `crate/tests/` | open/save/merge/redact/incremental | corpus affirmatively-licensed |
| Property | `proptest` | near code / `tests/` | round-trips, inverses, invariants | generators are ours |
| Snapshot | `insta` | `tests/snapshots/` | large structured outputs | golden = our validated output |
| Fuzz | `cargo-fuzz` | `fuzz/` | never panic/OOM | corpus affirmatively-licensed |
| Python | `pytest`+`hypothesis` | `python/tests/` | API + fitz-compat | contract from public docs (Tier-A) |
| Conformance | custom harness | `conformance/` | external validity | compare behavior, never copy |

**Property examples:** filter inverse `decode(encode(x)) == x` (Flate/LZW/ASCII85/ASCIIHex/RunLength) + predictor inverse; serializer round-trip `parse(serialize(o)) == normalize(o)`; geometry (normalize idempotent, union/intersect commutative, area = |det|, invert round-trip ε, **rotation matrices `COORD-ROT-*` exact**); decode-never-panics; tokenizer-total (spans cover input, no overlap); cross-mode `words`-concat ≈ `text` whitespace-normalized; buffer-protocol lifetime (`PIXMAP-BUF-LIFETIME-*`); GC-3 COW-unshare (`GC3-COW-*`).

**Snapshot clean-room rule:** a `.snap` is generated by *our* code, then **human-validated against the PDF spec + visible content** (e.g. vs `pdftotext -bbox` geometry within the §14.5 tolerance) before `cargo insta accept`; reviewer sign-off recorded; never seeded from PyMuPDF.

### 10.3 Fixtures / corpus — affirmative-license required + named clearers (resolves critique #19)

**The corpus is itself a license-exposure surface** for a project whose thesis is license cleanliness; the policy is therefore **affirmative-permissive-license-required**, not merely "AGPL-absent" (a manifest lint that only greps for `AGPL|GPL` cannot detect an unlicensed-but-not-AGPL file — that gap is closed here).

- **Every fixture MUST carry an affirmative permissive/PD license in `fixtures/MANIFEST.toml`** with fields `source`, `license` (must be one of the §6.3 ✅ set or an explicit PD/CC0 declaration), `sha256`, `cleared_by` (named human), `cleared_date`. A fixture with `license = "unknown"` or empty **fails the manifest lint** — absence of a positive license is a hard fail, not a pass.
- **Named clearers / process owners (not just a tool):**
  - **veraPDF + PDF Association corpora:** portions have mixed/unclear provenance. **Each file used must be individually cleared by the OSS-review owner** (role: *Corpus Steward*) and tagged with the upstream license; files whose license cannot be affirmatively established are **excluded**, not included-by-default.
  - **GovDocs1 / scraped real-document corpora:** these contain real third-party documents with **unknown copyright** (used in research under fair-use norms, **not** an OSS license). Policy: **GovDocs1 is NOT used as a shipped/committed fixture corpus.** It may be used **only** in a *non-committed, locally-fetched* fuzz/robustness pass (crash-finding, where the file content is never reproduced in our outputs or repo), gated behind a `dev/corpus-external` fetch script and **excluded from the wheel, the repo, and any golden**. The Corpus Steward signs off on this usage boundary. This removes GovDocs1 from the license-exposure surface entirely.
- **`CONF-CORPUS-v1` composition (the G1 denominator):** self-generated (primary), plus the **affirmatively-cleared** subsets of veraPDF/PDF-Association/Project-Gutenberg/US-gov/NASA PD PDFs. Frozen by content hash; the exact file count is recorded at M1 start; a changed corpus → new version suffix.
- **Manifest lint (strengthened):** fails on (a) any file with non-affirmative license, (b) `AGPL|GPL` string match, (c) known-AGPL-hash blocklist match, (d) any file present on disk but absent from `MANIFEST.toml`, (e) any `cleared_by` that is empty for a non-self-generated file.
- **Generators:** well-formed (known text at known positions → predictable `words` boxes; each filter/xref-style/encryption variant) + malformed (truncated tail, broken xref, missing startxref, wrong `/Length`, dangling/cyclic refs, garbage prefix) + adversarial (nested-filter bombs, deep nesting, huge `/Length`) doubling as fuzz seeds. Deterministic (fixed seeds).

### 10.4 Coverage & mutation targets

- Whole-repo line ≥85% (soft, tracked); **diff-coverage ≥90% hard gate** on every PR; new public functions at 0% block merge (interacts cleanly with §10.1.1 — only the implementation PR carries new impl lines).
- **`pdf-core` parser/filter modules ≥95% line.**
- **Mutation:** `cargo-mutants --in-diff` per PR (surviving mutants block unless annotated `// MUTANTS: skip — reason`); sharded full run nightly; **caught-mutant ratio ≥80%** in core filters/parsers/geom/serialize.

### 10.5 CI & Definition-of-Done gate

CI jobs (PR + main): `fmt --check`; `clippy --all-targets --all-features -D warnings`; multi-OS `test` (Linux/macOS/Windows); **`test-order-guard`** (git-history test-precedes-impl check, §10.1.1); **`catalog-status-guard`** (milestone has 0 remaining RED tags at exit); `coverage` (`cargo-llvm-cov` → Codecov, 90% diff threshold); `mutants-diff`; **`fuzz-smoke`** (Linux+nightly, 60 s/target, 0 crashes, run in `mmap:Never` mode per §9.6.1); `pytest` (matrix py 3.10–3.13, doctests, hypothesis); `wheels` (build + smoke-import + pytest on 3 OSes); `conformance` (`qpdf --check` / pikepdf / pdfminer / pdf.js / veraPDF); `cargo-deny` license + advisory + **shipped-vs-dev graph split** (§6.3) + AGPL-provenance/manifest lint; `cargo-geiger` whole-tree unsafe-surface tracking (§9.6.1); `compat-symbol-guard` (every baseline PyMuPDF public symbol has a `COMPAT.toml` disposition, §7). Scheduled: long fuzz (`-max_total_time=3600`, persisted minimized corpus), full sharded mutation, full-corpus conformance, **mmap-truncation nightly fuzz** (§9.6.1), supply-chain attestation refresh (§11.4).

**Definition of Done (per PR):**

| # | Gate | Enforced by |
|---|---|---|
| D1 | every catalogued test for the feature green (at the implementation PR) | test job + catalog status |
| D2 | builds `--all-features` on 3 OSes | CI matrix |
| D3 | `cargo fmt --check` clean | CI |
| D4 | `cargo clippy … -D warnings` clean | CI |
| D5 | diff-coverage ≥90%; no uncovered new public fn (impl PR only) | llvm-cov diff |
| D6 | untrusted-byte fns have a fuzz target; 60 s smoke clean | fuzz-smoke |
| D7 | property tests for algebraic invariants | review + presence |
| D8 | mutation score on changed source met or justified skip | mutants-diff |
| D9 | Python API additions have pytest + runnable doctest | pytest |
| D10 | spec traceability note on each new test module | review |
| D11 | no AGPL/GPL-derived fixture/test; new fixtures **affirmatively** license-tagged with named clearer | manifest lint |
| D12 | test-precedes-impl satisfied (RED tag added in an earlier merged commit) | test-order-guard |
| D13 | new PyMuPDF-baseline symbols, if any, have a `COMPAT.toml` disposition | compat-symbol-guard |

**Milestone exit (aggregate gate):** (a) 100% of milestone catalog entries `green` and **0 remaining RED tags** (catalog-status-guard), (b) module coverage met, (c) mutation target met, (d) fuzz targets exist + clean nightly, (e) integration scenarios pass on 3 OSes, (f) all milestone-blocking legal gating items (§6.5) cleared. CI refuses to tag a milestone release while any checklist box is unchecked. *"Implemented but tests not passing" is not Done; "tests pass but no fuzz/property hardening for a parser" is not Done.*

### 10.6 Conformance / cross-validation (clean-room oracles)

Structural validity → **`qpdf --check`** / **pikepdf** (Apache/MPL **dev-only** tooling, used as external checkers); extraction sanity → **pdfminer.six** / **pdf.js** (MIT/Apache — Jaccard overlap + ground-truth string presence, **never byte-identical**, never importing their tests); image-codec correctness → **pdfium-render + pdf.js** as **two independent** oracles (not a single unestablished oracle, §8.4.1); PDF/A classification → **veraPDF** verdict agreement within an allowlist (corpus referenced by URL+hash, fetched in CI). **PyMuPDF** is used **only** under the strict §6.2.1 oracle protocol (clean-room–excluded reviewer, spec-re-derived bug reports, no Tier-B bytes crossing the boundary, default-off, removable on counsel rejection per §6.5 #1) — it is never a golden source and never seeds Tier-B expectations.

### 10.7 Concrete test-first template (representative granularity)

**Feature: `FlateDecode` (`filters::flate::decode`/`encode`)** — Spec ISO 32000-1 §7.4.4 + RFC 1950/1951. *(Catalogued, then added RED+`#[ignore]` in the test PR, then un-ignored in the impl PR per §10.1.1.)*

- **Decode** `FLATE-DEC-001` empty→empty · `-002` `decode(encode(b"hello"))==b"hello"` · `-003` known zlib bytes → precomputed hex · `-004` 64 KiB random round-trip · `-005` `b"A"*100000` round-trips+compresses · `-006` truncated → typed `Err` no panic · `-007` corrupted middle → `Err` · `-008` trailing garbage → valid prefix per policy · `-009` raw deflate (no header) per policy · `-010` declared output > rss limit → bounded error not OOM · `-011` wrong Adler-32 per policy.
- **Predictors** `FLATE-PRED-001` none=identity · `-002..005` PNG Sub/Up/Average/Paeth (incl. tie-break) round-trip · `-006` PNG optimum multi-row · `-007` TIFF 2 · `-008` Colors/BPC/Columns stride matrix · `-009` Columns mismatch → `Err`.
- **Property** `FLATE-PROP-001` `decode(encode(x))==x ∀x` · `-002` `unpredict(predict(rows,cfg))==rows` · `-003` arbitrary bytes never panic.
- **Fuzz** `FLATE-FUZZ-001` `fuzz_flate`, rss/len bounded, 0 crashes smoke.
- **Integration** `FLATE-INT-001` self-generated Flate-content PDF extracts correctly · `-002` re-save→reopen text intact.

Order: 001→002→003 (working decode) → 006/007 (errors) → predictors → property+fuzz. Each number RED before its code exists; `decode` grows only enough to green the next case.

**Feature: `Page.get_text("words")`** — contract = `(x0,y0,x1,y1,word,block_no,line_no,word_no)` (Tier-A shape). Cases (abridged): `WORDS-001` empty→[] · `-002` single word, bbox within §14.5 tolerance from self-gen font metrics · `-003` space-split in one `Tj` · `-004` `TJ`-kerning split with no literal space (hard) · `-005` two lines → `line_no`++ · `-006` two BT/ET → distinct `block_no` · `-007` content emitted out of order → sorted output · `-008` rotated text axis-aligned envelope (uses `COORD-ROT-90`) · `-009` Differences encoding · `-010` Type0+ToUnicode multi-byte · `-016` Tr 3 included per contract; `WORDS-PROP-001` well-formed bbox · `-002` words-concat ≈ `text` · `-003` `(block,line,word)` unique non-decreasing; `WORDS-PY-001..004` shape/types/kwargs/fitz-compat/hypothesis; `WORDS-GT-001` char accuracy vs **self-generated ground truth**; `WORDS-CONF-001` Jaccard ≥ threshold vs pdfminer (secondary diagnostic only).

---

## 11. Dependency & Packaging Plan

### 11.1 Crate table (consolidated)

| Crate | Version | License | Purpose | Phase |
|---|---|---|---|---|
| pdf-writer / lopdf / krilla | 0.13 / 0.41 / 0.6 | MIT/Apache, MIT | writer *design references* (pdf-writer optionally depended — see §15 Q5) | M3 |
| flate2 + miniz_oxide | 1.1 / 0.8 | MIT/Apache(/Zlib) | FlateDecode | M1 |
| weezl | 0.1 | MIT/Apache | LZWDecode | M1 |
| (in-house) | — | MIT | ASCIIHex/85/RunLength/predictors | M1 |
| aes/cbc/rc4/sha2/md-5 | 0.8/0.1/0.1/0.10/0.10 | MIT/Apache | encryption R2–R6 | M1/M3 |
| zune-jpeg / jpeg-decoder | 0.5 / 0.3 | MIT/Apache(/Zlib) | DCTDecode + cross-check | M5 |
| jpeg-encoder | 0.7 | MIT/Apache AND IJG | JPEG encode on save | M5 |
| hayro-jpeg2000 | 0.4 | MIT/Apache | JPXDecode (subset §8.4.1) | M5 |
| hayro-jbig2 | 0.3 | MIT/Apache | JBIG2Decode (subset §8.4.1) | M5 |
| hayro-ccitt / fax | 0.3 / 0.2 | MIT/Apache / MIT | CCITTFaxDecode (+encode via fax) | M5 |
| image / png / tiff | 0.25 | MIT/Apache | image docs / pixel buffers | M5 |
| ttf-parser | 0.25 | MIT/Apache | font parsing/metrics (contains `unsafe`) | M2/M6 |
| allsorts | 0.16 | Apache-2.0 | font subsetting (full-embed fallback §8.5.2) | M4/M5 (feature) |
| rustybuzz / swash / fontdue / moxcms | 0.20/0.2/0.9/0.8 | MIT, Apache, MIT/Apache, BSD/Apache | shaping/rasterize/ICC | M6 |
| encoding_rs | 0.8 | (Apache OR MIT) AND BSD-3 | legacy/CJK encodings | M2 |
| unicode-normalization/-bidi/-segmentation | 0.1/0.3/1 | MIT/Apache | normalization/bidi/boundaries | M2 |
| bytes / parking_lot / thiserror | 1 / 0.12 / 2 | MIT/(Apache) | byte slices / locks / errors | all |
| memmap2 | 0.9 | MIT/Apache | mmap source (truncation mitigation §9.6.1) | M1 |
| pyo3 | ≥0.29 (free-threaded support gated per §9.4) | MIT/Apache | Python bindings | all |
| rust-numpy | 0.27–0.28 | BSD-2 | pixmap↔ndarray | M5 |
| maturin (+ maturin-action) | 1.13 | MIT/Apache | packaging | all |
| proptest/insta/criterion/cargo-fuzz+libfuzzer-sys/arbitrary/pretty_assertions | current | MIT/Apache (**insta: MIT OR Apache-2.0**) | testing | all |
| cargo-deny / cargo-about / cargo-mutants / cargo-llvm-cov / cargo-geiger | current | MIT/Apache | license gate / NOTICE / mutation / coverage / unsafe-tracking | all |
| pdfium-render (optional + oracle) | 0.8 | MIT/Apache (binding); PDFium BSD/Apache | fallback render + differential oracle (not shipped by default) | M6/CI |
| hypothesis (Python dev) | 6.x | **MPL-2.0** | property tests (**dev-only**, §6.3 carve-out) | all |

### 11.2 Wheels / maturin / abi3

- **GIL build:** one **`abi3-py310`** wheel per (OS, arch) → covers CPython ≥3.10 GIL builds.
- **Free-threaded build:** a **separate** wheel (PEP 703), **not** folded into the abi3 wheel (§9.4), shipped once the PyO3 free-threaded/abi3t support matrix is green; not on the v1 critical path.
- **Platforms:** Linux manylinux2014 + musllinux_1_2 (x86_64, aarch64 via `--zig`), macOS universal2, Windows x86_64. Pure-Rust backends only (no system zlib/C linkage; the optional `jpx-openjpeg-c` feature is **off** in published wheels, §8.4.1) → self-contained wheels with no shipped C blob.
- **Each wheel** smoke-imported (`python -c "import oxide_pdf"`) and pytest-smoked in CI before publish; bundled `THIRD-PARTY-LICENSES` via `cargo-about` (incl. IJG/Unicode-DFS notices); `cargo-deny` green (zero GPL/AGPL/LGPL/SSPL **and zero MPL in the shipped graph**).

### 11.3 CI matrix

| Job | OS | Python | Notes |
|---|---|---|---|
| lint (fmt/clippy/deny/geiger) | ubuntu | — | blocking; deny enforces shipped-vs-dev graph |
| test | ubuntu/macos/windows | — | `--all-features` |
| coverage / mutants-diff | ubuntu | — | 90% diff gate; in-diff mutation |
| fuzz-smoke | ubuntu (nightly) | — | libFuzzer; `mmap:Never` mode |
| pytest | ubuntu/macos/windows | 3.10–3.13 | doctests + hypothesis |
| wheels | ubuntu/macos/windows | abi3-py310 | build+import+smoke; publish on tag |
| conformance | ubuntu | — | qpdf/pikepdf/pdfminer/pdf.js/veraPDF |
| supply-chain | ubuntu | — | cargo-vet/cargo-deny advisories + checksum pin verify (§11.4) |

### 11.4 Supply-chain / build reproducibility (resolves critique #25e — "auditable supply chain" was a headline claim with no mechanism)

`cargo-deny` (license/advisory) is **necessary but not sufficient** for the "auditable supply chain" claim. v1 adds concrete attestation:

- **Dependency review:** `cargo-vet` is adopted; every dependency (and version bump) requires a recorded `cargo-vet` audit entry (or an imported trusted audit) — CI fails on unvetted deps. This is the actual mechanism behind "auditable."
- **Checksum pinning:** `Cargo.lock` is committed and CI verifies the locked hashes; wheel builds are pinned to the locked graph. Python build deps are hash-pinned in the lockfile used by `maturin`.
- **Reproducible builds:** wheel builds set `SOURCE_DATE_EPOCH` and a fixed Rust toolchain (pinned `rust-toolchain.toml`) so a rebuild from a tag is bit-reproducible where the toolchain allows; deviations are documented.
- **Provenance attestation:** release wheels carry **SLSA-style build provenance** via the CI provenance generator (GitHub Actions OIDC attestations), so consumers can verify the wheel was built from the tagged source by our pipeline. PyPI Trusted Publishing (OIDC) is used (no long-lived tokens).
- **No prebuilt binary blob:** the wheel contains only our compiled-from-source Rust + Python; no third-party prebuilt C engine is bundled (the differentiator vs pypdfium2). `cargo-geiger` output is published per release for unsafe-surface transparency.

---

## 12. Roadmap & Milestones

Effort in **agent-week-equivalents (AWE)** + T-shirt size (planning units, not calendar promises). Every exit criterion is a **TEST gate**. Standing rules: parser/codec code `#![forbid(unsafe_code)]`, runs under `Limits` (defaults §9.6.2), ships `cargo-fuzz` targets from introduction; `cargo-deny` + AGPL-provenance lint + `compat-symbol-guard` on every PR.

**AWE corrected to match stated risk (resolves critique #17).** The prior table contradicted §8.2/R1/R16 by sizing M1 == M3 even though "repair should budget more than the writer," and underfunded M2. Corrected so the numbers **reflect** the risk concentration: M1 (which *contains* the writer-exceeding repair subsystem **plus** parse/objects/filters/crypto-read) is now the largest single milestone; M2 (the largest single *subsystem* — full text interpreter + font mapping + 10 serializers + search + layout) is funded above its prior figure.

| Milestone | Slice | Size | AWE | Gating theme |
|---|---|---|---|---|
| M0 | setup/CI/TDD/geometry | S | 2–3 | CI gates provably red-on-violation |
| M1 | parse+objects+filters+crypto-read+**repair** | XXL | **18–24** | ≥99% corpus opens (§3.4 def); never-panic; ≥95% core cov |
| M2 | text extraction + search | XL | **14–18** | shape parity + ≥0.98 char accuracy vs **ground truth** |
| M3 | save/incremental/GC + page ops + merge | XL | 12–16 | byte-exact incremental (clean-parse); `qpdf --check` clean |
| M4 | annotations / forms / redaction + get_drawings + Widget | XL | 12–16 | multi-surface redaction gate; AP portability |
| M5 | image-docs + codecs + Pixmap (incl. image-only pages) + shim | L | 9–12 | codec cross-checks (2 oracles); compat coverage % |
| **M6** | **vector rendering** | XXL | 30–45+ | **deferred past v1** |

**Repair-vs-writer sizing made explicit.** Within M1, the repair subsystem alone is budgeted at **~8–11 AWE** — i.e., **greater than the entire M3 writer's ~12–16** *is not* the claim; the precise claim (now consistent with §8.2) is: **repair alone exceeds the *write-path serializer* portion of M3** (the serializer/full-save/incremental core, ~6–8 AWE of M3), which is what "budget more than the writer" means. M1's total dwarfs M3 because M1 also carries parse+xref+filters+crypto-read. This removes the prior M1==M3 inconsistency.

**v1 total (M0–M5) ≈ 67–89 AWE.** M1–M4 hold ~80% of v1 effort and the risk concentration; M5 de-risked by `hayro-*` permissive codecs (with the §8.4.1 fallback policy).

### Milestone detail & TDD exit gates

**M0 — Setup / CI / TDD / geometry (S, 2–3).** Eight-crate workspace + DAG lint; geometry (`Matrix/Point/Rect/IRect/Quad` + `Identity`, `EMPTY/INFINITE_RECT`, paper sizes); full CI (fmt, clippy-deny, multi-OS test, llvm-cov diff, mutants-in-diff, fuzz-smoke, cargo-deny shipped-vs-dev, cargo-geiger, provenance lint, **test-order-guard (git-history)**, catalog-status-guard, compat-symbol-guard); catalog skeleton + fixture `MANIFEST.toml` (affirmative-license schema §10.3); maturin/abi3 stub wheel; `cargo-vet` bootstrap + `rust-toolchain.toml` pin (§11.4). **Exit:** `GEOM-*` unit+property green (concat/invert round-trip ε, **rotation 0/90/180/270 exact `COORD-ROT-*`**, normalize idempotent, union/intersect commutative, area=|det|); each CI gate proven red-on-violation by a canary PR; stub wheel imports on 3 OSes; provenance lint fails a planted AGPL-hashed fixture AND a planted unlicensed (non-AGPL) fixture; test-order-guard fails a canary that adds impl before a RED test.

**M1 — Parse + objects + filters + crypto-read + repair (XXL, 18–24).** Tokenizer; object model + lazy `Arc` cache + mmap source (truncation mitigation §9.6.1); xref (classic + stream + objstm + `/Prev` + hybrid + linearization-read); **repair subsystem** + `parse_was_repaired`/`header_offset` tracking; filters (Flate+predictors/LZW/ASCII*/RunLength decode); page tree + inheritance; encryption read R2–R6 + crypt filters + exemptions + SASLprep + **`/ID`-absent fallback** + **R5-read/R6-write policy**; low-level xref API; `Document.open`/`open_with(untrusted())`/`page_count`/`load_page`/`metadata`/`needs_pass`/`authenticate`/`permissions`/`is_repaired`; `Page.bound/rect/rotation/*box`; `Limits` with pinned defaults (§9.6.2). **Exit:** `LEXER-*`/`XREF-*`/`OBJSTM-*`/filter/`REPAIR-*`/`CRYPT-*`/`LIMITS-DEFAULT-*` catalogs 100% green (incl. per-filter round-trips + predictor inverse + per-object-key `min(len+5,16)`/`sAlT`-AESV2-only + R6-no-salt); **≥99% of `CONF-CORPUS-v1` opens per §3.4 definition (page_count correct on the verified subset) in Lenient mode; malformed set repairs-with-warning or typed-error — never panics/OOM/hangs** (fuzz, in `mmap:Never`); every re-serialized fixture passes `qpdf --check`/pikepdf; crypto round-trips RC4/AES-128/AES-256(R6), R5 decrypts but never writes, wrong password fails cleanly, `/Encrypt`+`/ID` verified never-encrypted, `/ID`-absent path proven, one fixture vs `qpdf --decrypt`; fuzz `fuzz_open`/`fuzz_xref`/`fuzz_repair`/per-filter clean nightly; `pdf-core` parser/filter ≥95% line; mmap-truncation nightly job characterized.

**M2 — Text extraction + search (XL, 14–18).** Content-stream interpreter (incl. inline-image capture-not-just-skip, pattern/shading classify); font mapping layer (encodings/Differences/Type0-CID/ToUnicode/AGL/CJK CMaps/widths/Core-14 AFM/descriptor flags — **after §6.5 #2 data-license clearance**); layout (spans/lines/blocks/words, XY-cut order, super/subscript, rotated/vertical/RTL, dehyphenation); **per-method `TEXTFLAGS_*` defaults pinned**; serializers (text/blocks/words/dict/json/rawdict/rawjson/html/xhtml/xml + textbox/selection); `search_for`; `get_fonts`/`get_images`; `TEXT_*` flags. **Exit:** `WORDS-*`/`DICT-*`/`CMAP-*`/`ENCODING-*`/`GLYPHLIST-*`/`WIDTHS-*`/`LAYOUT-*`/`SEARCH-*`/`TEXTFLAGS-DEFAULT-*`/`COORD-ROT-*` green + cross-mode property; **serializer key/nesting/arity == PyMuPDF *documented* shape (Tier-A); html/xhtml/xml validated as `oxide-pdf`-defined valid serialization (Tier-B, own goldens)**; insta goldens human-validated vs `pdftotext -bbox`; **char accuracy ≥0.98 (norm. Levenshtein) vs self-generated ground truth (primary); ≥0.95 CJK on ground truth where ToUnicode/CMaps exist; word-bbox IoU ≥0.90 vs ground-truth boxes; pdfminer agreement reported as secondary diagnostic, not a gate**; search recall/precision vs ground truth; fuzz `fuzz_cmap`/`fuzz_content_stream`/`fuzz_get_text` clean.

**M3 — Save/incremental/GC + page ops + merge (XL, 12–16).** Serializer; object-edit API; full save (classic + xref/objstm authoring, `/ID`, free-list); GC 1–4 (**exclusion list + COW-unshare §8.7.1**); deflate options; **incremental save (clean-parse precondition §8.7)**; encryption write (R6 only); page ops + `insert_pdf`; metadata write + `get/set_xml_metadata` + TOC + named dests + `/PageLabels` read + `get_label`. **Exit:** `SAVE-*`/`INCR-*`/`GC-*`/`GC3-EXCLUDE-*`/`GC3-COW-*`/`MERGE-*`/`PAGEOPS-*`/`TOC-*`/`META-*`/`PAGELABEL-*` green + serializer property; **incremental byte-exactness `out[..orig.len()]==orig` on clean-parse fixtures; repaired fixtures rejected (or upgraded only when opted in)**, new `/Prev` == prior `startxref`, both revisions reopen; signature-preservation fixture: a clean signed PDF edited incrementally keeps `out[..orig.len()]==orig` and the signature byte range intact; round-trip invariants across full/full+GC/incremental/encrypted; GC levels each proven incl. dedup-exclusion and COW-unshare-after-merge; merge order/count/refs correct, shared font deduped single, every saved fixture `qpdf --check` clean; TOC round-trip equals input, level-jump rejected; named dest resolves to correct physical page under non-trivial `/PageLabels`.

**M4 — Annotations / forms / redaction (XL, 12–16).** Content emission (insert_text/textbox Base-14 + TTF embed, subset-with-full-embed-fallback; insert_image; draw_*/Shape; links); annotations full family + `/AP` generation; **`get_drawings()`/`get_cdrawings()`**; **redaction (multi-surface §8.8)**; forms read/fill/flatten + **`Widget` API**; embedded files + bake/scrub. **Exit:** `ANNOT-*`/`FORM-*`/`WIDGET-*`/`REDACT-*`/`DRAWINGS-*`/`INSERT-*` green; each subtype reopens with subtype/geometry/`/AP /N`; `update()` reflects color in AP; **redaction security gate — after apply+full-save, the harness fully decompresses every stream + object stream, then byte-greps for the secret across the decompressed corpus AND `get_text()` AND verifies image-region blanking; a hit anywhere fails** (the old compressed-file grep is explicitly forbidden); survivors unshifted; PIXELS blanks+re-encodes (incl. JBIG2/JPX transcode, fail-closed `PdfRedactionError` if undecodable); incremental-after-redaction rejected/auto-upgraded; forms set→`/V`+AP value, checkbox on-state from `/AP /N`, radio group `/V`, flatten removes `/AcroForm`+widgets; embedded-file extract byte-equals original. (No cross-viewer *render* smoke — rendering is M6; replaced by structural `qpdf --check` + reopen + `/AP` presence checks, per §3.1.)

**M5 — Image-docs + codecs + Pixmap + shim (L, 9–12).** `pdf-image` (PNG/JPEG/TIFF-multi-IFD/GIF/BMP/WEBP); image codecs (DCT/CCITT/JBIG2/JPX — JBIG2/JPX as documented subset §8.4.1, fuzzed/capped/untrusted, save-transcode, **degradation contract**); `Pixmap` (samples/save/tobytes/buffer-protocol+lifetime §9.4/numpy) for **image docs and image-only pages §3.3**; `extract_image`/`extract_font`/`get_char_widths`; **fitz/pymupdf shim hardening** + `COMPAT.toml` + `PdfUnsupportedError`. **Exit:** `IMGDOC-*`/`DCT-*`/`CCITT-*`/`JBIG2-*`/`JBIG2-COV-*`/`JBIG2-FAIL-*`/`JPX-*`/`JPX-COV-*`/`PIXMAP-*`/`PIXMAP-IMGONLY-*`/`PIXMAP-BUF-LIFETIME-*`/`FITZCOMPAT-*` green; PNG→1-page correct MediaBox; JPEG passthrough byte-equal + `/DCTDecode`; alpha→`/SMask`; palette→`/Indexed`; multi-TIFF page_count == IFDs; `convert_to_pdf` (image inputs) passes `qpdf --check`, non-image input → `PdfUnsupportedError`; **`get_pixmap` on image doc AND on an image-only PDF page == decoder output (pixel-equality); vector page → `PdfUnsupportedError`; undecodable image-only page → typed error while `get_text` still works**; JBIG2/JPX cross-validated vs **two oracles** (pdfium-render + pdf.js) on the supported subset, coverage gaps catalogued, codec fuzz clean under caps; buffer-protocol lifetime test (drop Pixmap with live memoryview, mutate, read) passes; **compat coverage ≥ target % `implemented`**, behavioral-parity pytest green (xfail documented deviations), `compat-symbol-guard` blocks `implemented→missing` regression.

**M6 — Vector rendering (XXL, 30–45+, deferred).** Depend on `hayro`; `pdfium-render` optional backend + oracle. **Exit (when scheduled):** render-to-pixmap perceptual-hash parity vs `pdfium-render` within tolerance on the render corpus (PyMuPDF only via §6.2.1 protocol); calls that currently raise `PdfUnsupportedError` for vector pages become green; transparency/shading fidelity tracked as documented long-tail.

### Feature Priority Matrix → milestone

See §7 (full table; every PyMuPDF capability mapped to priority+milestone, including the previously-unlisted inline images / patterns-shadings / `get_drawings` / `Widget` / per-method default flags). Long poles & staff-first: content-stream interpreter (M2/M4 shared), repair subsystem (M1, the dominant cost), `insert_pdf` deep-copy/dedup (M3), incremental-save byte-exactness on clean parse (M3), AES-256 R6 (M1/M3), redaction multi-surface correctness (M4). De-risking levers: `hayro-*` codecs collapse JBIG2/JPX (with §8.4.1 fallback); `pdf-writer`/`krilla`/`lopdf` are permissive writer design references; `allsorts` is the realistic subsetter (with full-embed fallback §8.5.2).

---

## 13. Risks & Mitigations

Likelihood (L) / Impact (I): Low / Med / High.

| # | Risk | L | I | Mitigation (concrete) |
|---|---|---|---|---|
| R1 | **Malformed-PDF repair gap** (#1 "works on A, fails on B") | High | High | First-class M1 repair subsystem (now the largest-budgeted slice, 18–24 AWE incl. ~8–11 for repair); gate ≥99% open per §3.4 in Lenient + never-panic fuzz; differential-validate vs qpdf/pikepdf; `parse_was_repaired` flag prevents downstream corruption |
| R2 | **JBIG2** young pre-1.0 permissive decoder + CVE history | Med | High | `hayro-jbig2` (pre-1.0, R-acknowledged); **documented coverage subset + degradation contract (§8.4.1): decode-fail → typed error, text extraction continues; partial decode never returned as complete**; cross-check vs **two** oracles (pdfium-render + pdf.js), not oxidize-pdf alone; fuzz under rss/time caps; ban GPL jbig2dec; coverage gaps catalogued in `docs/codec-coverage.md` |
| R3 | **JPEG2000** young permissive pure-Rust codec | Med | High | `hayro-jpeg2000` (pre-1.0); same degradation contract + documented subset (§8.4.1); optional `jpx-openjpeg-c` (BSD-2) fallback **off by default**, with the pure-Rust/no-unsafe/WASM tension explicitly flagged; differential vs pdfium-render + pdf.js |
| R4 | No permissive JBIG2/JPX **encoder** | Low | Low | Transcode to Flate/JPEG on save; document format change; redaction transcode path §8.8 |
| R5 | **Font subsetting complexity / `allsorts` single point** | Med | Med | Ship Base-14 + full embedding first; subsetting feature-gated; **full-embed fallback on `allsorts` failure (§8.5.2) — never a hard error, never a broken font**; size KPI reconciled to merge-dedup not authored-CJK (§8.5.1) |
| R6 | **Encryption edge cases** (R6 2.B, layering, exemptions, SASLprep, `/ID`-absent, R5-vs-R6) | Med | High | RustCrypto leaves; strict-from-spec KDF with corrected per-object key (`min(len+5,16)`, `sAlT`-AESV2-only, R6-no-salt); R5-read/R6-write policy; `/ID`-absent fallback; exhaustive `CRYPT-*`; cross-check Acrobat/qpdf; `fuzz_decrypt` |
| R7 | **fitz compat gaps + Tier-B contamination** | High | High | Behavioral/Tier-A shape compat, not bit-identical; **Tier-A/Tier-B split (§6.1)** keeps expressive output out of expectations; machine-checked `COMPAT.toml` + `compat-symbol-guard`; tolerance goldens (§14.5); `PdfUnsupportedError` not `AttributeError` |
| R8 | **Redaction security failure** (selectable under box / image / incremental leak / compressed-grep false-pass) | Med | High | Multi-surface scrub (§8.8); force full-rewrite; **gate runs over FULLY-DECOMPRESSED corpus** (closes the compressed-grep false-pass bug); image PIXELS decode+blank+re-encode with fail-closed `PdfRedactionError`; documented limitation for fonts shared with non-redacted pages |
| R9 | **Performance vs C — unfalsifiable KPIs** | Med | Med | KPIs rebuilt on a **named frozen `BENCH-CORPUS-v1` + criterion harness + fixed hardware baseline + methodology (§14.1)**; speculative pypdfium2 ratios **dropped from v1 gates** (kept only as tracked-non-gating observations); the only hard perf KPI is "far faster than pypdf"; cold-open expected to *beat* pypdfium2 (we mmap+lazy, no decode) is stated as a *hypothesis to measure*, not a gate |
| R10 | **Clean-room / AGPL contamination (incl. oracle osmosis)** | Low→Med | High | Two-room discipline; never AGPL source in AI context; **strict oracle protocol §6.2.1 (clean-room-excluded reviewers, spec-re-derived bug reports, no Tier-B bytes, default-off, removable on counsel rejection)**; provenance logging; cargo-deny; affirmative-license fixture manifest + named clearers; **residual risk acknowledged, counsel sign-off gating (§6.5)** |
| R11 | **`hayro` "experimental"** (no encryption, partial blend, single-vendor) | Med | Med | Encryption is *our* core (not hayro's); pin versions + `cargo-vet`; keep pdfium-render fallback; M5 needs only hayro's codec leaves with §8.4.1 fallback; **single-vendor governance risk noted in §17.3**; contribute upstream |
| R12 | **Untrusted-input attack surface incl. dependency `unsafe`/mmap-UB** | High | High | `forbid(unsafe)` **first-party** + precise scoped claim (§9.6.1); cargo-fuzz from M1 (+OSS-Fuzz when public); `Limits` pinned defaults (§9.6.2); **mmap-truncation mitigation + `mmap:Never` untrusted preset + WASM no-mmap (§9.6.1)**; `cargo-geiger` whole-tree tracking; checked arithmetic + no-panic clippy deny |
| R13 | **abi3t (free-threaded) tooling immaturity / abi3 conflation** | Low | Low | Ship abi3-py310 GIL baseline; **separate** free-threaded wheel, PyO3-version-gated (§9.4); not critical path |
| R14 | **pypdfium2 already covers permissive+fast vector render** | Med | Med | Differentiate on MIT + pure-Rust first-party (no shipped C blob) + render+extract+edit+generate + fitz-API + bindings; target AGPL-blocked migrants; image-only-page Pixmap covers the scanned-doc path **now** without waiting for M6 |
| R15 | **Scope creep** (Story/OCR/tables/OCG/sig/non-PDF) | High | Med | Hard non-goals (§3.2); P3 deferred; `PdfUnsupportedError` + matrix link; `compat-symbol-guard` forces explicit disposition of every baseline symbol; milestone exit gated on 100%-green catalog |
| R16 | **Effort underestimate on M1/M2/M4** | Med | High | **AWE corrected (§12) so M1 is the largest and M2 is funded above prior figure** — numbers now match the stated risk; ranges carry headroom; M1→M2 deliver standalone value even if later slips |
| R17 | **Corpus license exposure** (the fixtures themselves) | Med | High | **Affirmative-permissive-license-required (§10.3)**; named Corpus Steward clears veraPDF/PDF-Association per-file; **GovDocs1 removed from shipped/committed corpus**, allowed only as non-committed local fuzz input; manifest lint fails on non-affirmative license, not just AGPL strings |
| R18 | **PyMuPDF baseline moves (1.25+)** | Med | Med | **Baseline-evolution policy §17.2**: pinned baseline, `compat-symbol-guard` detects new symbols, scheduled re-baseline with explicit dispositions; SemVer policy §17.1 governs our own surface |

---

## 14. Success Metrics / KPIs

All KPIs are now **testable against a named, frozen artifact** (no "vibes" numbers). Tolerances are pinned in §14.5.

### 14.1 Performance methodology (resolves critique #9 — KPIs were untestable)

- **Benchmark corpus:** **`BENCH-CORPUS-v1`** — a frozen, license-cleared set of N PDFs spanning size buckets (small <100 KB, medium 100 KB–5 MB, large 5–100 MB, huge >100 MB), content classes (text-heavy, image-heavy/scanned, form-heavy, encrypted), and pathology (clean vs repair-needed). Composition + hashes recorded in `bench/BENCH-CORPUS-v1.manifest.toml`.
- **Harness:** `criterion` for micro/op benches + a wall-clock end-to-end harness for open/extract/save/merge; results checked into `bench/results/` per release.
- **Hardware baseline:** a **named reference machine** (recorded in `bench/HARDWARE.md`: CPU model, core count, RAM, OS, filesystem) is the canonical baseline; CI perf runs on the GitHub `ubuntu-latest` class are tracked separately and only for **regression detection**, not for the published absolute numbers.
- **Methodology:** warm vs cold open distinguished (cold = page-cache dropped where the OS permits); N≥10 iterations, report median + IQR; competitors (pypdf, pypdfium2, pikepdf) run on the identical corpus + machine.

### 14.2 Correctness & conformance (ground-truth primary — resolves critique #8)

- Open/parse ≥99% of `CONF-CORPUS-v1` per the §3.4 "open" definition (Lenient); **0 panics/OOM/hangs** on corpus + malformed + fuzz (non-negotiable, absolute gate).
- 100% of saved/merged/redacted outputs pass `qpdf --check` (allowlist only).
- Encryption: 100% R2–R6 **read** round-trip; R6 **write** round-trip; R5 read-only (never written); exemption-set verified; `/ID`-absent path verified.
- **Extraction accuracy is measured PRIMARILY against self-generated ground truth** (PDFs we synthesize with known text at known positions, so the target is *true*, not pdfminer's errors): char accuracy **≥0.98** (norm. Levenshtein) vs **ground truth** Latin; **≥0.95** CJK where ToUnicode/predefined CMaps exist; word-bbox mean IoU **≥0.90** vs **ground-truth word boxes** (we know where each word is because we placed it). pdfminer.six and pdf.js are **secondary cross-checks reported as diagnostics**, never the gate — being *more* correct than pdfminer must not penalize us.

### 14.3 fitz-API coverage

- **≥85% `implemented`** of in-scope symbols (machine-counted via `COMPAT.toml`), rest `partial`/`deviates`/`out-of-scope`, never silently `missing`; coverage badge monotonically non-decreasing (`compat-symbol-guard` blocks regressions); behavioral-parity suite green with every deviation a documented `xfail`.

### 14.4 Performance KPIs (gated vs tracked-only)

- **Gated (must pass on `BENCH-CORPUS-v1`/reference machine):** parse/open + enumeration **far faster than pypdf** (≥3× median across buckets); text extraction **≥ pypdf**; save/merge **competitive with pikepdf** (within 1.5× median); **multi-doc extraction scales near-linearly across N Python threads** (GIL released) — a capability pypdf/PyMuPDF can't match safely (measured speedup ≥0.7·N up to physical cores).
- **Tracked-only (observations, NOT v1 gates — per R9):** ratios vs **pypdfium2** for cold open and text extraction are recorded each release as informational; the *hypothesis* that cold-open beats pypdfium2 (we mmap+lazy, don't decode) is measured and reported but is **not** a release-blocking gate. Vector-render speed vs C is not a v1 KPI at all (M6).

### 14.5 Pinned numeric tolerances (resolves critique #24 Q9 — gates referenced "tolerance" but it was undefined)

These are **normative** and make the M2/M3 bbox/parity gates testable:

| Tolerance | Value | Applies to |
|---|---|---|
| bbox absolute coordinate tolerance | **≤ 0.5 pt** per coordinate | `dict`/`words`/`blocks` bbox vs ground-truth boxes |
| word-bbox IoU floor | **≥ 0.90** mean, **≥ 0.80** p5 | `words` vs ground-truth |
| char accuracy (Latin) | **≥ 0.98** norm. Levenshtein | `text` vs ground truth |
| char accuracy (CJK, ToUnicode/CMap present) | **≥ 0.95** | `text` vs ground truth |
| reading-order Kendall-τ | **≥ 0.90** | block/line order vs ground-truth order |
| color value | exact (sRGB int) | span/char color |
| word-gap space threshold | **0.25 × space-advance** (default; `INHIBIT_SPACES`/`PRESERVE_WHITESPACE` modify) | word segmentation |
| Jaccard vs pdfminer (diagnostic only) | reported, **no floor gate** | secondary cross-check |

### 14.6 Fuzz stability

0 reproducible crashes across all targets on clean nightly long-run (in `mmap:Never`); mmap-truncation nightly job characterizes residual OS-fault risk; caught-mutant ratio **≥80%** in core filters/parsers/geom/serialize; corpora minimized (`cmin`) + persisted; OSS-Fuzz onboarding once public.

### 14.7 Wheels / platform

abi3-py310 GIL wheels for manylinux2014 + musllinux_1_2 (x86_64, aarch64), macOS universal2, Windows x86_64 — each smoke-imported + pytest-smoked before publish; **separate** free-threaded wheel once green; pure-Rust backends only (no shipped C blob; `jpx-openjpeg-c` off in published wheels); bundled `THIRD-PARTY-LICENSES`; SLSA provenance attached (§11.4); `cargo-deny` green (zero GPL/AGPL/LGPL/SSPL, zero MPL in shipped graph).

### 14.8 Process (TDD)

Diff-coverage **≥90%** every merged PR; `pdf-core` parser/filter **≥95% line**; **100% of a milestone's catalogued tests green with 0 remaining RED tags** before milestone close (machine-checked); DoD gates D1–D13 enforced on 100% of PRs; `cargo-vet` clean (no unvetted deps).

---

## 15. Open Questions (genuinely open; blocking items have been promoted out)

The previously-"open" items that **block** milestones (MSRV/PyO3-matrix data **needed for build**, Adobe data licensing, tolerance definitions, `Limits` defaults) have been **resolved or promoted**: tolerances are pinned in §14.5; `Limits` defaults in §9.6.2; Adobe data licensing is now a **legal gating release-blocker** (§6.5 #2, blocks M2) rather than an open question; the oracle legality is a gating item (§6.5 #1). What remains genuinely open does **not** block any milestone gate:

1. **Final name & clearance.** `oxide-pdf` is a placeholder — needs PyPI/crates.io/USPTO/EU/domain clearance and counsel sign-off (esp. before naming any artifact `fitz`). (Gating only for *public naming*, §6.5 #3.)
2. **`fitz` shim packaging.** Ship `import fitz` literally, or only an opt-in, clearly-labeled compat package? (Counsel + migration-friction tradeoff.)
3. **fitz-API coverage target.** ≥85% `implemented` is the KPI — confirm which specific P2/P3 symbols are `out-of-scope` vs `partial` (does not block; `compat-symbol-guard` forces a disposition either way).
4. **MSRV + PyO3 free-threaded matrix.** Pin MSRV (1.74 vs 1.75) against the chosen PyO3 release; pin the **first PyO3 version with stable free-threaded/abi3t** for the separate free-threaded wheel (§9.4). Resolve before the free-threaded wheel job is enabled (not on v1 critical path).
5. **`pdf-writer` dependency vs reimplement.** Depend on it for spec-correct emission, or reimplement the writer to keep the chokepoint fully in-house? (Affects the "from scratch" framing; decide at M3 start.)
6. **Predefined CJK CMap footprint.** Bundle all Adobe-Japan1/GB1/CNS1/Korea1 + UCS2 tables, feature-gate per-ROS, or fetch-on-demand? (Performance/size tradeoff; licensing itself is the gating item §6.5 #2.)
7. **`hayro` coupling for M6.** Commit to `hayro` as the rendering leaf, or keep `pdfium-render` co-equal until `hayro` matures? (M6-only.)
8. **OSS-Fuzz onboarding timing** — gate on first public release, or run a private continuous-fuzz pipeline before that?

---

## 16. Appendix

### 16.1 Glossary

- **AGPL Section 13** — network-use clause requiring source disclosure to remote users; the reason PyMuPDF is avoided.
- **AWE** — agent-week-equivalent; one focused AI-agent week on one coherent slice (planning unit).
- **`BENCH-CORPUS-v1` / `CONF-CORPUS-v1`** — frozen, license-cleared, content-hashed corpus snapshots used as the testable denominators for performance and conformance KPIs respectively.
- **COS** — Carousel Object System; PDF's low-level object/dictionary/stream layer.
- **CID / Type0** — composite font model for CJK and subsetted fonts; codes map via a CMap to CIDs to glyphs.
- **CMap** — character-code mapping (encoding CMap code→CID, or ToUnicode CMap code→Unicode).
- **Clean parse / repaired parse** — a parse with `parse_was_repaired == false` (clean: incremental-save-eligible) vs one that needed reconstruction / had `header_offset ≠ 0` (repaired: full-save-only).
- **Cross-reference (xref)** — index of object byte offsets; classic table, xref stream, or object stream entries.
- **DoD** — Definition of Done; the per-PR machine-enforced gate (§10.5, D1–D13).
- **Image-only page** — a PDF page whose content stream contains only image XObject `Do` invocations + graphics-state ops (no text/path/shading); in scope for `get_pixmap` in v1 (§3.3).
- **Incremental update** — appended body+xref+trailer block, `/Prev`-chained; preserves prior bytes (signatures) — valid only on a clean parse.
- **Linearization** — "Fast Web View" layout for byte-range streaming; read-transparent, write deferred.
- **Object stream (ObjStm)** — compressed container of multiple indirect objects; members never individually encrypted.
- **Repair / reconstruction** — full object scan + structure recovery for malformed files; the production differentiator; sets `parse_was_repaired`.
- **ROS** — Registry-Ordering-Supplement; identifies a CID font's character collection (e.g. Adobe-Japan1).
- **R5 vs R6** — AES-256 PDF encryption: R5 is the deprecated transitional single-SHA-256 form (read-only); R6 is the Algorithm-2.B iterated-hash form (read + write).
- **Standard Security Handler R2–R6** — PDF encryption revisions: RC4-40 (R2) → AES-256/Algorithm 2.B (R6).
- **TextPage** — cached structured-text model (blocks→lines→spans→chars) backing all `get_text` variants; carries a content-version snapshot and raises `PdfStaleError` if used after an edit (§9.7).
- **Tier A / Tier B (clean-room)** — Tier A = facts/structure (API names, documented shapes, spec-dictated values) that MAY seed expectations; Tier B = expressive/observed output (exact serialization bytes, undocumented fields, message prose) that MUST NOT (§6.1).
- **Trm** — text rendering matrix = `params · Tm · CTM`, locating each glyph in device space (page transform §8.6.1 applied after).

### 16.2 References

- ISO 32000-1 (PDF 1.7), ISO 32000-2 (PDF 2.0), ISO/TS 32003 (AES-GCM), ISO/TS 32004.
- PyMuPDF docs (public, Tier-A only): Document / Page / TextPage / Widget / Appendix 1 (text extraction) / Functions / Tools — pymupdf.readthedocs.io.
- Licensing: artifex.com/licensing (AGPL + commercial); GNU AGPL-3.0 text (§13); **Google Open Source AGPL Policy (opensource.google/documentation/reference/using/agpl-policy)**; Google v. Oracle (2021) (Supreme Court opinion; Texas Law Review & CACM analyses, noting the holding covers API *declarations*, not output formats/behavior cloning).
- Encryption/structure: qpdf docs (encryption, object/xref streams, tolerant xref); iText "Unknown encryption type R=6"; PDF Association (PDF 2.0 crypto, UTF-8 in PDF 2.0); Algorithm 2.B (ISO 32000-2 §7.6.4.3.4).
- Crates: lopdf, pdf-rs, pdf-writer, krilla, hayro (+ hayro-jbig2/-jpeg2000/-ccitt), oxidize-pdf, flate2/miniz_oxide, weezl, zune-jpeg, jpeg-decoder/-encoder, fax, ttf-parser, rustybuzz, allsorts, swash, moxcms, encoding_rs, unicode-*, RustCrypto (aes/cbc/rc4/sha2/md-5), pyo3, rust-numpy, maturin, memmap2, proptest, insta (MIT/Apache), criterion, cargo-fuzz, cargo-deny, cargo-mutants, cargo-llvm-cov, cargo-geiger, cargo-vet.
- Competitors: pypdf, pikepdf (binding MPL-2.0 / qpdf engine Apache-2.0), pdfminer.six, pdfplumber, pypdfium2 (PDFium BSD/Apache), ReportLab (BSD-3 OSS library + separate paid product), pdfrw, borb, WeasyPrint.
- Supply chain: SLSA framework, cargo-vet book, PyPI Trusted Publishing (OIDC), reproducible-builds.org (`SOURCE_DATE_EPOCH`).
- Tooling specifics: cargo-mutants (`--in-diff`/`--shard`), cargo-fuzz / Rust Fuzz Book, OSS-Fuzz Rust integration, cargo-llvm-cov + codecov-action, PyO3 parallelism/free-threading guide & building-and-distribution, PEP 703 / PEP 803 (free-threading, abi3t).

---

## 17. Versioning, Compatibility & Governance (new — resolves critique #25)

### 17.1 SemVer & API-stability policy

- **Rust crates** follow **Cargo SemVer**. Pre-1.0, breaking changes bump the minor (0.x.0); the public surface of `pdf-api` is the stability contract — internal crates (`pdf-core` etc.) may break more freely until 1.0. A `#[doc(hidden)]`/`__internal` boundary marks non-contract items. The **1.0 Rust release** is gated on M1–M5 complete + the public `pdf-api` surface frozen.
- **Python package** follows **PEP 440 / SemVer-aligned** versioning independent of the Rust crate versions (they may differ). The Python public API (the `oxide_pdf` package, **not** `_core`) is the stability contract; `_core` is private and may change between any releases.
- **`fitz` shim** versions track the **PyMuPDF baseline** they target (e.g., a shim version metadata field records `targets PyMuPDF 1.24.x`), so consumers know which surface they're getting.

### 17.2 PyMuPDF-baseline-evolution policy (what happens at 1.25+)

- The reference baseline is **p
