# oxide-pdf — Real-Corpus Validation Report

_Generated: 2026-06-15T09:47:53.411317+00:00 • qpdf: qpdf version 12.3.2 • oracle (PyMuPDF/pdfminer) available: True_

This is the project's first accuracy/robustness measurement on **real-world** PDFs (prior numbers used self-generated fixtures only). Oracles run locally as diff references only; **no PyMuPDF/oracle output is committed** — only similarity scores and content-free structural diff reasons.

## 1. Corpus

- **tier1** (committable, public-domain): 30 files, 26.9 MB total
- **tier2** (fetch-only, NOT committed (CC BY-SA)): 4 files, 0.0 MB total

Tier-1 provenance: all files are US-federal-government works (public domain, 17 U.S.C. §105) from IRS, GovInfo, CDC MMWR, NASA NTRS, USGS, and NIST — each recorded in `fixtures/MANIFEST.toml` (source/license/sha256/cleared_by/cleared_date). Tier-2 (PDF Association `pdf20examples`, CC BY-SA 4.0) is used for robustness only.

## 2. Open / Repair / Fail rate

- Opened: **34/34 (100.0%)**
- Reported as repaired: 0
- Failed to open: 0

## 3. Never-panic / Robustness

- **No aborts, no panics, no hangs** across all 34 inputs. Every open+extract ran in an isolated subprocess under a wall-clock timeout; all exited cleanly (exit 0).

## 4. Structural validity (qpdf --check on re-saved output)

- Sampled 14 opened PDFs → `doc.save()` → `qpdf --check`: **14/14 pass (100.0%)** (pass = qpdf reports no structural errors; warnings allowed).

| file | qpdf result |
|---|---|
| `cdc-mmwr-7251a1.pdf` | PASS — clean |
| `cdc-mmwr-7301a1.pdf` | PASS — clean |
| `cdc-mmwr-7302a1.pdf` | PASS — clean |
| `govinfo-cdoc110-50.pdf` | PASS — clean |
| `govinfo-hjres1.pdf` | PASS — clean |
| `govinfo-hr1.pdf` | PASS — clean |
| `govinfo-hr2.pdf` | PASS — clean |
| `govinfo-hr3056.pdf` | PASS — clean |
| `govinfo-hr815enr.pdf` | PASS — clean |
| `govinfo-s1.pdf` | PASS — clean |
| `irs-f1040.pdf` | PASS — clean |
| `irs-f1040sb.pdf` | PASS — clean |
| `irs-f1040sc.pdf` | PASS — clean |
| `irs-f1065.pdf` | PASS — clean |

## 5. Differential text accuracy vs PyMuPDF (headline) & pdfminer

Per-document similarity of `oxide_pdf` `get_text("text")` vs each oracle, on whitespace-normalized full-document text. Levenshtein = normalized edit similarity (sequence-level); Jaccard = word-set overlap (vocabulary-level).

| oracle | docs | Levenshtein mean | Lev. median | Jaccard mean | Jacc. median | ≥0.95 | ≥0.80 | <0.50 |
|---|---|---|---|---|---|---|---|---|
| **pymupdf** | 34 | **0.6933** | 0.6641 | 0.9149 | 0.969 | 4 | 11 | 5 |
| **pdfminer** | 34 | **0.6452** | 0.6155 | 0.9132 | 0.943 | 5 | 11 | 5 |

**Headline (vs PyMuPDF / fitz):** mean Levenshtein **0.693**, median **0.664**, mean Jaccard **0.915** over 34 documents.

### Worst-case divergences vs pymupdf

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `cdc-mmwr-7302a1.pdf` | 0.346 | 0.987 | 21802 | 21680 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p501.pdf` | 0.375 | 0.875 | 204225 | 200596 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `cdc-mmwr-7301a1.pdf` | 0.411 | 0.990 | 25614 | 25605 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `cdc-mmwr-7251a1.pdf` | 0.414 | 0.981 | 37585 | 37586 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p502.pdf` | 0.497 | 0.879 | 120194 | 116058 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-s1.pdf` | 0.529 | 0.602 | 791947 | 785714 | 3199/9211 oxide tokens absent from oracle (spurious/mis-decoded) |
| `irs-p15.pdf` | 0.530 | 0.932 | 313274 | 303270 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-f4868.pdf` | 0.556 | 1.000 | 20931 | 20965 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

### Worst-case divergences vs pdfminer

| file | Lev | Jacc | our chars | their chars | why they differ |
|---|---|---|---|---|---|
| `govinfo-s1.pdf` | 0.122 | 0.603 | 791947 | 802103 | 3196/9211 oxide tokens absent from oracle (spurious/mis-decoded); similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr2.pdf` | 0.128 | 0.703 | 268593 | 271696 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr1.pdf` | 0.130 | 0.700 | 227066 | 229815 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `govinfo-hr3056.pdf` | 0.209 | 0.869 | 17695 | 17816 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p501.pdf` | 0.416 | 0.881 | 204225 | 206746 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `cdc-mmwr-7301a1.pdf` | 0.513 | 0.880 | 25614 | 23480 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `cdc-mmwr-7302a1.pdf` | 0.522 | 0.851 | 21802 | 19677 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |
| `irs-p15.pdf` | 0.541 | 0.936 | 313274 | 315342 | similar vocabulary but different ordering/spacing (reading-order or word-break difference) |

## 6. Prioritized divergence causes (future diff-oracle fix tasks)

1. **Reading-order / word-spacing differences (column/line segmentation vs fitz)** — 18 doc(s). e.g. `cdc-mmwr-7251a1.pdf`, `cdc-mmwr-7301a1.pdf`, `cdc-mmwr-7302a1.pdf`
2. **Moderate divergence (mixed spacing/encoding)** — 11 doc(s). e.g. `govinfo-cdoc110-50.pdf`, `govinfo-hjres1.pdf`, `govinfo-hr1.pdf`
3. **Extra/spurious content — oxide emits text fitz does not (mis-decoded glyphs or duplicated content)** — 1 doc(s). e.g. `govinfo-s1.pdf`

> Sections 1–6 above are generated by the harness from live oracle diffs on each
> run. The subsection below records a manual root-cause investigation of the
> worst cases (no oracle text is reproduced — only the structural cause).

### 6a. Verified root causes (manual investigation of worst cases)

These were confirmed by inspecting the actual oxide vs PyMuPDF output for the
lowest-Levenshtein documents. They are the prioritized, concrete fix tasks:

1. **Multi-column reading order (highest-impact).** On multi-column documents
   (CDC MMWR articles, IRS publications) oxide and fitz extract essentially the
   *same words* — Jaccard 0.95–0.99 — but in a *different sequence*, which is why
   Levenshtein drops to 0.34–0.53. Concretely, on `cdc-mmwr-7302a1.pdf` oxide
   emits the main article column first and the "INSIDE" sidebar table-of-contents
   later, whereas fitz interleaves the sidebar at its on-page vertical position.
   Fix: column/zone detection in the text reading-order pass to match fitz's
   block ordering. This single cause explains the bulk of the headline gap
   between the high Jaccard (0.915) and the moderate Levenshtein (0.693).

2. **Text not clipped to the CropBox / printable area.** `govinfo-s1.pdf` shows
   ~3,200 oxide-only tokens that fitz never emits — they are GovInfo print-control
   strings in the page margin (e.g. `00000Frm 00001Fmt 00002Fmt …`) that sit
   outside the visible/crop region. fitz suppresses glyphs outside the cropbox;
   oxide currently extracts them. Fix: clip extracted text runs to the page
   CropBox (or at least drop runs fully outside it). This is the dominant
   "spurious content" cause and also depresses Jaccard on `govinfo-*` bills.

3. **Occasional duplicated lines.** On `cdc-mmwr-7302a1.pdf` oxide emitted a long
   line ("Cannabis use during adolescence is associated with poor …") twice in a
   row where fitz emitted it once. A small number of consecutive long-line
   duplications were observed; worth a targeted look at the content-stream text
   accumulation path (possible double-emit on certain `Tj`/`TJ` or form-XObject
   replays).

4. **Whitespace/line-break normalization (low severity).** Even on the closest
   documents, oxide tends to insert slightly more newlines than fitz (e.g. 525 vs
   479 on `cdc-mmwr-7302a1.pdf`). This barely affects Jaccard and is mostly
   cosmetic, but contributes a few points of Levenshtein loss across the board.

No glyph-loss / ToUnicode failures were observed in this corpus (no document had
oxide-empty-while-fitz-has-text), and there were **no scanned/image-only blanks**
among these born-digital government PDFs. A future Tier-2 robustness batch with
scanned corpora would be needed to exercise that path.

---

_Methodology: each PDF is opened+extracted in an isolated subprocess (timeout per file) so a Rust panic/abort cannot crash the harness. qpdf qpdf version 12.3.2. Oracles: PyMuPDF (AGPL, local-only) primary; pdfminer.six (MIT) secondary. Similarity computed on normalized text via difflib SequenceMatcher (Levenshtein proxy) and token Jaccard._
