# HANDOFF — OCG gaps (PRD-NEXT §0 item 6): `oc=` marked content + `/Usage /ViewState` + `/AS`

> Historical handoff preserved while merging all branches (2026-09-26). Its WIP status and proposed actions are superseded by the current implementation, docs/PRD-NEXT.md and clean-room development rules; this is not an active task instruction.
Status: **WIP — research and design complete, no production code landed.** The
session ran out of quota right after the design was fixed and the implementation
subagent was launched (it was told to discard its edits). Everything below was
verified against real PyMuPDF 1.28.2 on 2026-09-05; the probe scripts and the
hand-built fixture PDFs are in `/Volumes/ExternalSSD/tmp/ocg2/` (`p_a_writers.py`,
`p_a_writers2.py`, `p_b_usage.py`, `fix_*.pdf`, `run.sh`). MuPDF's `pdf-layer.c`
(`pdf_is_ocg_hidden_imp`) is at `/Volumes/ExternalSSD/tmp/ocg/pdf-layer.c`.

## 1. PyMuPDF semantics — (a) writers with `oc=`

| PyMuPDF call | What it writes |
|---|---|
| `page.insert_text(..., oc=x)` / `insert_textbox` / `Shape.insert_text` / `Shape.insert_textbox` / `TextWriter.write_text(page, oc=x)` | `q\n/OC /MC0 BDC\nBT\n…\nET\nEMC\nQ\n` — BDC right after the opening `q`, EMC right before the closing `Q` |
| `Shape.finish(..., oc=x)` (`Shape.commit` has **no** `oc`) | each finished block: `q\n/OC /MC1 BDC\n<w / d / RG / rg / path / paint>\nEMC\nQ\n` |
| `page.insert_image(..., oc=x)` / `page.show_pdf_page(..., oc=x)` / `insert_htmlbox(oc=)` | **no BDC**; `/OC x 0 R` is put on the image / form XObject dict (pdfspine's interpreter already honours XObject `/OC`) |
| `oc=0` (default) | nothing extra |
| bad `oc` (object not `/Type /OCG` or `/OCMD`) | `ValueError("bad optional content: 'oc'")`; nonexistent xref → `RuntimeError("bad xref")` |
| OCMD xref as `oc` | accepted like an OCG |

`/Resources /Properties` key rule (PyMuPDF `Page._get_optional_content`): the key
is `MC<i>`, the smallest `i ≥ 0` whose name is not already a `/Properties` key;
if an existing entry already references the same xref its key is **reused**
(two inserts with the same OCG both say `/OC /MC0 BDC`; a pre-existing unrelated
`/MC0` makes the next one `/MC1`). Note: the task brief said `/ocN` — that is
wrong, PyMuPDF uses `/MCn`. The `/Properties` dict lives on the page's
`/Resources` (which PyMuPDF keeps indirect; pdfspine materialises a direct dict).

## 2. PyMuPDF semantics — (b) `/Usage /View /ViewState` and `/AS` (View usage)

Probe = OCG referenced by `/OC /MC0 BDC` text; `get_text()` and `get_pixmap()`
always agreed.

| Case (config state, OCG `/Usage`, `/D /AS`) | PyMuPDF | pdfspine target |
|---|---|---|
| ON, no usage | visible | visible |
| ON, `/View << /ViewState /OFF >>`, no `/AS` | **hidden** | hidden (MuPDF parity, unconditional) |
| ON, ViewState OFF, `/AS [<< /Event /View /Category [/View] /OCGs [ocg] >>]` | hidden | hidden |
| ON, ViewState OFF, `/AS` with `/Event /Print` only, or `/OCGs []` | hidden | hidden (MuPDF ignores `/AS`) |
| ON, ViewState ON | visible | visible |
| OFF, ViewState ON, no `/AS` | hidden | hidden |
| OFF, ViewState ON, `/AS` View entry lists the OCG | hidden | **visible** — deliberate ISO 32000-1 §8.11.4.4 divergence (usage application dict sets the state; MuPDF's FIXME says it should but ignores `/AS`). Document it like the `/VE` / AllOn / AnyOff divergences. |
| ON, `/Print << /PrintState /OFF >>` or `/Export …` | visible | visible (only View usage is evaluated) |
| ON, ViewState OFF reached through an OCMD `/OCGs [ocg]` | hidden | hidden (evaluate per OCG, OCMD inherits) |
| ViewState bogus name / `/View <<>>` / no `/View` | config state | config state |
| OCG not listed in `/OCProperties /OCGs` | visible (desc empty) | visible (already the case) |
| OCG `/Intent /Design` with config `/Intent /View` (or vice-versa) | hidden | **not in scope** — remaining gap (MuPDF hides on intent mismatch only when the config has a non-empty `/Intent`) |

Reporting parity: MuPDF's `layer_ui_configs()` / `get_ocgs()` / `ocg_state()`
reflect only the config ON/OFF state, **not** `/Usage`. Keep `OcConfig::is_on`
unchanged and apply usage rules only in `OcVisibility::read`.

Decision table for `OcVisibility::read` (per OCG `num` in `/OCGs`):
```
vs = /Usage /View /ViewState of the OCG (Some(true) | Some(false) | None)
hidden = if vs == Some(false)                      { true }        // MuPDF: always hides, even over a panel override
         else if let Some(forced) = overrides[num] { !forced }     // layer-panel override
         else if vs == Some(true) && as_view ∋ num { false }       // /AS usage application (spec)
         else                                       { !cfg.is_on(num) }
```
`as_view` = OCG numbers of every `/AS` entry of the **active** config (`/D` or the
selected `/Configs[n]`, no inheritance from `/D`) with `/Event /View` and
`/Category` containing `/View` (resolve references at every level).

## 3. Code map (verified line numbers at `21d636a`)

- `crates/pdf-core/src/ocg.rs`: `OcVisibility` 359–477 (`read` 368), `OcConfig` 687–770
  (`active` 704, `from_dict` 726, `is_on` 758), helpers `config_dict` 494, `oc_properties` 563,
  `ocg_object_numbers` 574, `ref_nums` ~806. No `/Usage`, `/ViewState`, `/AS`, `/Event`,
  `/Category` handling anywhere in `crates/`.
- `crates/pdf-text/src/interp.rs`: `bdc_hidden` 1144–1173 (looks `/OC /name` up in
  `/Resources /Properties`), `do_xobject` `/OC` check 1216–1223, BDC/EMC counter 479–509.
- `crates/pdf-edit/src/content.rs`: `PageContent::append_content` 90, `add_resource` 141,
  `fresh_name` 216 (`{prefix}{n}`, n from 0 — matches `MC0`). No `/Properties` handling.
- `crates/pdf-edit/src/text.rs`: `TextOptions` 52 (+ `Default` 67), `insert_text` 153,
  `build_text_chunk` 206 (`q\nBT\n…ET\nQ\n`), `insert_textbox` 239.
- `crates/pdf-edit/src/image.rs`: `insert_image_jpeg` 33, `insert_image_rgb` 81,
  `place_image` 128. `crates/pdf-edit/src/merge.rs`: `show_pdf_page` 141 (form dict 174–187).
- `crates/pdf-edit/src/drawing.rs`: `Shape::finish` 263 (`q\n{w} w\n…Q\n`, returns `()`),
  `commit` 324, one-shot `draw_*` wrappers 345+. `crates/pdf-edit/src/ocg.rs`: `set_oc` 326
  (style for `/OC`), `set_ocmd` type check via `doc.get_object(xref, 0)`.
- `crates/pdf-api/src/lib.rs`: `FinishParams` 2730, `ShapeHandle::commit` 2819 (default
  trailing-block literal ~2825), `page_insert_text` 2882, `page_insert_textbox` 2914,
  `page_insert_image_jpeg` 2946, `page_insert_image_rgb` 2961, `page_show_pdf_page` 3751.
- `crates/py-bindings/src/lib.rs`: `value_err` 40 (`PyValueError`), `map_err` 100
  (`invalid-argument` currently → `PdfSyntaxError`; add a local mapper for the oc entry
  points instead of changing it), `PyShape::finish` 1550, `commit` 1573, `show_pdf_page` 2375,
  `insert_text` 2668, `insert_textbox` 2699, `insert_image` 2734.
- `python/pdfspine/document.py`: `Shape.finish` 1196, `Shape.insert_text` 1225,
  `Shape.insert_textbox` 1254, `Page.show_pdf_page` 2239, `Page.insert_text` 2818,
  `Page.insert_textbox` 2843, `Page.insert_image` 2869, one-shot `Page.draw_line/rect/circle/
  oval/bezier/polyline` 2904–2960 (Rust one-shots), Shape-based `draw_curve/quad/sector/
  squiggle/zigzag` 2960–3044, `Page.write_text` 3604 (has `oc`, drops it on the fast path),
  `TextWriter.write_text` 4036. Stubs: `python/pdfspine/_core.pyi` (224, 476, 486, 497),
  `python/pdfspine/document.pyi` (168, 365, 376, 387, 605, 677–720).
- Docs to update: `python/pdfspine/_llms/docs/api.md` line 228 (the "局限" sentence —
  remove the writer gap, add the ViewState/AS sentence + the `/AS` divergence),
  `gotchas.md` (no OCG section yet), `CHANGELOG.md` Unreleased OCG bullet (add Added/Fixed;
  the "Known gap" text lives in `docs/PRD-NEXT.md` lines 67–68 and 212–213).
- Tests: `crates/pdf-core/tests/ocg_unit.rs` (`build_doc`/`vis_doc`/`config_doc`, tests
  `ocg_vis_*` 1004–1106), `crates/pdf-text/tests/interp_ocg.rs`,
  `crates/pdf-edit/tests/insert_text_e2e.rs` + `common/mod.rs` (`page_content_bytes`,
  `page_fonts`, `first_xobject_dict`, `save_reopen`), `python/tests/test_ocg_layers.py`
  (highest id PYOCG-038; helpers `_assemble`, `_text_words`, `_nonwhite`,
  `_real_pymupdf_available`, `_run_child` subprocess oracle; next ids PYOCG-039…).

## 4. Implementation plan (as specified to the subagent; nothing landed)

(a) pdf-edit: `PageContent::add_optional_content(&self, oc: u32) -> Result<String>`
(validate `/Type` OCG|OCMD else `Error::InvalidArgument("bad optional content: 'oc'")`,
reuse existing `/Properties` entry referencing `oc`, else `add_resource("Properties",
"MC", Reference)`); `TextOptions.oc: u32` (Default 0); `insert_text`/`insert_textbox` emit
`q\n/OC /MCn BDC\nBT…ET\nEMC\nQ\n`; `Shape::finish(…, oc: u32) -> Result<()>` wraps each
block; `insert_image_jpeg/rgb(…, oc)` and `show_pdf_page(…, oc)` put `/OC` on the XObject
dict. pdf-api: `FinishParams.oc`, `oc` params on the five `page_*` fns. py-bindings: `oc=0`
kw on `insert_text/insert_textbox/insert_image/show_pdf_page/PyShape.finish`, map
`invalid-argument` → `ValueError` locally. Python: explicit `oc: int = 0` on all of the
above plus `Shape.insert_text/insert_textbox`, `TextWriter.write_text`, `Page.write_text`
fast path, `Page.draw_*` (one-shots route through `new_shape()` only when `oc != 0`);
update both `.pyi`. Rust tests `crates/pdf-edit/tests/ocg_writers_e2e.rs` (`OCG-WRITE-*`).

(b) pdf-core `ocg.rs`: `OcConfig.as_view: HashSet<u32>` read from the active config's
`/AS`; `fn view_state(doc, num) -> Option<bool>`; `OcVisibility::read` uses the decision
table in §2; module doc comment updated. Tests in `ocg_unit.rs`: ViewState OFF hides (direct
and via OCMD, and over a panel override), ViewState ON + no `/AS` stays OFF, `/AS` View
promotes, `/AS` Print-only does not, PrintState ignored, alternate config's own `/AS`
applies when selected and `/D`'s does not leak, `ocg_state()` unaffected.

Python tests (PYOCG-039…): pdfspine `insert_text(oc=)` content shape + `/MCn` reuse;
`insert_image(oc=)` → `get_oc()`; `Shape.finish(oc=)`; bad oc → `ValueError`; pdfspine
write → real PyMuPDF (subprocess, skip if unavailable) hides/shows identically for an OFF
OCG; PyMuPDF write → pdfspine reads identically (extend the PYOCG-038 pattern); ViewState
OFF / PrintState / `/AS` fixtures via `_assemble` cross-checked with PyMuPDF (`/AS` refs
cannot be written with `xref_set_key` — build bytes by hand); keep PYOCG-038 green.

## 5. Next-step commands

```
export CARGO_TARGET_DIR=/Volumes/Cargo/target/pdfspine-ocg2 CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/ExternalSSD/tmp
python3 -m venv .venv-task && . .venv-task/bin/activate
pip install "maturin>=1.12,<2" pytest hypothesis "ruff==0.14.14" mypy pymupdf && maturin develop
python /Volumes/ExternalSSD/tmp/ocg2/p_a_writers.py      # re-verify §1 (needs real pymupdf)
python /Volumes/ExternalSSD/tmp/ocg2/p_b_usage.py        # re-verify §2, rewrites fix_*.pdf
cargo fmt --check && cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
python -m pytest -W error --doctest-modules python/pdfspine python/tests
ruff format --check python/pdfspine python/tests scripts && ruff check && mypy
python scripts/test-order-guard.py; python scripts/catalog-status-guard.py
python scripts/compat-symbol-guard.py; python scripts/manifest-lint.py
```
Commit as two commits: `feat(ocg): emit marked content for oc= writers` and
`feat(ocg): honour /Usage /ViewState and /AS`; `COMPAT.toml` unchanged.

## 6. Known risks

- `Shape::finish` returning `Result` and the new trailing `oc` params on
  `insert_image_*` / `show_pdf_page` ripple into pdf-api, py-bindings, pdf-image and
  several test crates — grep every caller before building.
- The `/AS` promotion diverges from PyMuPDF (visible vs hidden); a live-oracle test must
  not assert equality on that one case — hardcode the ISO expectation and document it.
- MuPDF caches its OCG descriptor: PyMuPDF `get_text()` on a document where `add_ocg` ran
  in the same session still shows OFF layers; always save + reopen in oracle scripts.
- `/Intent` mismatch hiding is a further uncovered gap (see §2 last row).
