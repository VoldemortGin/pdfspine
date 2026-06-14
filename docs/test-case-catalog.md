# Test Case Catalog

The project-level decomposition required by PRD §10.1.1: every planned public
function and internal algorithm is enumerated into named, numbered cases with a
status, **before** that milestone's implementation work starts. One test case =
one observable behavior / one input equivalence class (not one-per-line).

**Status legend:**

- `catalogued` — case exists here only (specification; no code yet).
- `written` — test code drafted but not yet landed RED.
- `red` — test landed and failing for the right reason (tagged
  `#[ignore = "RED: <ID> …"]` / `@pytest.mark.xfail(strict=True, reason="RED: …")`).
- `green` — implementation landed; the test passes.

> Milestone exit requires **0 remaining `red` tags** for that milestone's IDs
> (`catalog-status-guard`, PRD §10.1.1 step 3).

---

## M0 — Geometry (`pdf-core::geom`, re-exported via `pdf-api`)

Spec source of truth: PyMuPDF (`fitz`) geometry algebra — a Tier-A documented
contract (PRD §9.5), cross-checked against the PyMuPDF Matrix/Rect/Point/Quad
docs. Tests live in `crates/pdf-core/tests/geom_unit.rs` (unit) and
`crates/pdf-core/tests/geom_property.rs` (property).

### Matrix

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-MAT-001` | identity constant == constructor == default | PyMuPDF Matrix | green |
| `GEOM-MAT-002` | scale / translate constructors | PyMuPDF Matrix | green |
| `GEOM-MAT-003` | determinant `a*d - b*c` | linear algebra | green |
| `GEOM-MAT-004` | point transform matches documented `(12,16)` example | PyMuPDF algebra | green |
| `GEOM-MAT-005` | `p*(m1*m2) == (p*m1)*m2`; `*` == `concat` | PyMuPDF concat | green |
| `GEOM-MAT-006` | identity is neutral for concat | PyMuPDF Matrix | green |
| `GEOM-MAT-007` | invert of known matrix; `m * m^-1 == I` | linear algebra | green |
| `GEOM-MAT-008` | singular matrix has no inverse | linear algebra | green |
| `GEOM-MAT-009` | inverse matches documented division `(2,-2)` example | PyMuPDF algebra | green |

### Cardinal rotations (bit-exact)

| ID | feature | spec ref | status |
|---|---|---|---|
| `COORD-ROT-0` | `rotate(0)` is exactly identity | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-90` | `rotate(90) == [0,1,-1,0,0,0]`, zeros exact | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-180` | `rotate(180) == [-1,0,0,-1,0,0]`, zeros exact | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-270` | `rotate(270) == [0,-1,1,0,0,0]`, zeros exact | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-WRAP` | negative / >360 angles normalize bit-exact | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-90-PT` | `(1,0)` rotated 90° CCW -> `(0,1)` | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-CYCLE` | four 90° turns compose to identity (exact) | PyMuPDF Matrix(deg) | green |
| `COORD-ROT-45` | non-cardinal angle uses trig path | trig | green |

### Point

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-PT-001` | identity transform is a no-op | PyMuPDF Point | green |
| `GEOM-PT-002` | add / sub / neg / scale / norm | PyMuPDF Point algebra | green |
| `GEOM-PT-003` | `point * matrix` operator == transform | PyMuPDF algebra | green |

### Rect

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-RECT-001` | normalize swaps inverted edges; idempotent | PyMuPDF Rect.normalize | green |
| `GEOM-RECT-002` | width / height / area | PyMuPDF Rect | green |
| `GEOM-RECT-003` | union (`|`) smallest enclosing | PyMuPDF include_rect | green |
| `GEOM-RECT-004` | intersect (`&`) largest enclosed | PyMuPDF intersect | green |
| `GEOM-RECT-005` | disjoint intersect -> empty; `intersects` false | PyMuPDF intersect | green |
| `GEOM-RECT-006` | union with empty rect (argument-first order) | PyMuPDF include_rect | green |
| `GEOM-RECT-007` | contains point (inclusive) and sub-rect | PyMuPDF contains | green |
| `GEOM-RECT-008` | empty / infinite / valid predicates | PyMuPDF Rect | green |
| `GEOM-RECT-009` | `round()` floors x0/y0, ceils x1/y1 -> IRect | PyMuPDF Rect.round | green |
| `GEOM-RECT-010` | transform by translate (exact) and rotate(90) envelope | PyMuPDF Rect*matrix | green |

### IRect

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-IRECT-001` | width/height/area/normalize/union/intersect | PyMuPDF IRect | green |
| `GEOM-IRECT-002` | IRect <-> Rect round-trip; from(Rect) rounds outward | PyMuPDF IRect.rect | green |

### Quad

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-QUAD-001` | from-rect corner order ul, ur, ll, lr | PyMuPDF Quad | green |
| `GEOM-QUAD-002` | bounding-box rect of a rotated quad | PyMuPDF Quad.rect | green |
| `GEOM-QUAD-003` | rect -> quad -> transform(I) -> rect is stable | PyMuPDF Quad | green |
| `GEOM-QUAD-004` | width / height from side lengths | PyMuPDF Quad | green |

### Paper sizes

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-PAPER-001` | A4 / Letter / Legal exact dims; case-insensitive; unknown -> None | PyMuPDF paper_size | green |
| `GEOM-PAPER-002` | `-l` / `-landscape` suffix swaps w/h | PyMuPDF paper_size | green |
| `GEOM-PAPER-003` | `paper_rect` places page at origin | PyMuPDF paper_rect | green |

### Property (proptest)

| ID | feature | spec ref | status |
|---|---|---|---|
| `GEOM-PROP-001` | identity transform leaves point unchanged | invariant | green |
| `GEOM-PROP-002` | invert round-trip within ε | invariant | green |
| `GEOM-PROP-003` | `concat(m, m^-1)` acts as identity | invariant | green |
| `GEOM-PROP-004` | `p*(m1*m2) == (p*m1)*m2` | invariant | green |
| `GEOM-PROP-005` | normalize idempotent; result valid | invariant | green |
| `GEOM-PROP-006` | union commutative (non-empty domain) | invariant | green |
| `GEOM-PROP-007` | intersect commutative | invariant | green |
| `GEOM-PROP-008` | union contains both operands (non-empty) | invariant | green |
| `GEOM-PROP-009` | intersection contained in both | invariant | green |
| `GEOM-PROP-010` | area == width * height | invariant | green |
| `GEOM-PROP-011` | normalize preserves area | invariant | green |
| `GEOM-PROP-012` | cardinal rotation compose-to-identity (exact) | invariant | green |
| `GEOM-PROP-013` | `round()` outward contains original rect | invariant | green |
| `GEOM-PROP-014` | transformed-quad bbox contains all corners | invariant | green |

---

## M1a — Lexer/tokenizer + object model + serializer (`pdf-core`)

Spec source of truth: ISO 32000-1 §7.2 (lexical conventions), §7.3 (objects),
§7.3.8 (streams). Implements PRD §8.1 (tokenizer / object types) and §9.2 (core
data model). Tests live in `crates/pdf-core/tests/lexer_unit.rs`,
`crates/pdf-core/tests/object_unit.rs`, `crates/pdf-core/tests/serialize_unit.rs`
(unit) and `crates/pdf-core/tests/objmodel_property.rs` (property). Design center
(PRD §8.1): the lexer is **total** — arbitrary / truncated input yields a typed
error or EOF token, never a panic or out-of-bounds.

### Lexer — token kinds (`LEXER-*`)

| ID | feature | spec ref | status |
|---|---|---|---|
| `LEXER-001` | whitespace (incl. NUL, FF, CR, LF, TAB, SP) is skipped between tokens | ISO 32000-1 §7.2.2 | green |
| `LEXER-002` | comment `%`…EOL skipped; not inside strings | ISO 32000-1 §7.2.3 | green |
| `LEXER-003` | integer literal (`0`, `123`, `+17`, `-98`) | ISO 32000-1 §7.3.3 | green |
| `LEXER-004` | real literal `34.5`, `-3.62`, `+.002`, `4.` (trailing dot), `.5` (leading dot) | ISO 32000-1 §7.3.3 | green |
| `LEXER-005` | real with exponent `1e3`, `1.2E-2` tolerated (PRD §8.1) | PRD §8.1 | green |
| `LEXER-006` | literal string `(...)` with escapes `\n \r \t \b \f \( \) \\` | ISO 32000-1 §7.3.4.2 | green |
| `LEXER-007` | literal string octal escape `\ddd` (1–3 digits, overflow wraps mod 256) | ISO 32000-1 §7.3.4.2 | green |
| `LEXER-008` | literal string line-continuation `\`+EOL elides newline | ISO 32000-1 §7.3.4.2 | green |
| `LEXER-009` | literal string balanced nested parens + raw newlines | ISO 32000-1 §7.3.4.2 | green |
| `LEXER-010` | hex string `<48656C6C6F>`; whitespace skipped inside | ISO 32000-1 §7.3.4.3 | green |
| `LEXER-011` | hex string odd nibble count → pad trailing `0` | ISO 32000-1 §7.3.4.3 | green |
| `LEXER-012` | name `/Name`; `/` = empty name | ISO 32000-1 §7.3.5 | green |
| `LEXER-013` | name `#XX` hex escape decoded (`/A#42` → `AB`) | ISO 32000-1 §7.3.5 | green |
| `LEXER-014` | dict delimiters `<<` / `>>` | ISO 32000-1 §7.3.7 | green |
| `LEXER-015` | array delimiters `[` / `]` | ISO 32000-1 §7.3.6 | green |
| `LEXER-016` | keywords `obj endobj stream endstream R true false null xref trailer startxref` | ISO 32000-1 §7.3 | green |
| `LEXER-017` | keyword vs name disambiguation (`true` keyword, `/true` name) | ISO 32000-1 §7.3 | green |
| `LEXER-018` | EOF token at end of input; repeated `next` stays EOF | — | green |
| `LEXER-019` | truncated literal string → typed `Err`, no panic | PRD §8.1 | green |
| `LEXER-020` | truncated hex string → typed `Err`, no panic | PRD §8.1 | green |
| `LEXER-021` | truncated name escape (`/A#`) → typed `Err`, no panic | PRD §8.1 | green |
| `LEXER-022` | regular-character run after a number boundary (delimiter ends token) | ISO 32000-1 §7.2.2 | green |
| `LEXER-PROP-001` | tokenizing arbitrary bytes never panics; terminates at EOF | PRD §8.1 / §10.2 | green |

### Object parser (`OBJ-*`)

| ID | feature | spec ref | status |
|---|---|---|---|
| `OBJ-001` | parse `null` / `true` / `false` | ISO 32000-1 §7.3.2/§7.3.9 | green |
| `OBJ-002` | parse integer and real | ISO 32000-1 §7.3.3 | green |
| `OBJ-003` | parse literal & hex string into `PdfString` (bytes + kind) | ISO 32000-1 §7.3.4 | green |
| `OBJ-004` | parse name into `Name` (decoded) | ISO 32000-1 §7.3.5 | green |
| `OBJ-005` | parse empty array `[]` and heterogeneous array | ISO 32000-1 §7.3.6 | green |
| `OBJ-006` | parse empty dict `<<>>` and nested dict | ISO 32000-1 §7.3.7 | green |
| `OBJ-007` | parse reference `12 0 R` → `Reference` | ISO 32000-1 §7.3.10 | green |
| `OBJ-008` | `R` is reference keyword, not a name/keyword object | ISO 32000-1 §7.3.10 | green |
| `OBJ-009` | nested array containing dict containing reference | ISO 32000-1 §7.3 | green |
| `OBJ-010` | duplicate dict key → last wins | ISO 32000-1 §7.3.7 / PRD §8.1 | green |
| `OBJ-011` | indirect object `N G obj <obj> endobj` (no stream) | ISO 32000-1 §7.3.10 | green |
| `OBJ-012` | indirect stream with correct `/Length` integer body | ISO 32000-1 §7.3.8 | green |
| `OBJ-013` | indirect stream with no usable `/Length` → scan to `endstream` | ISO 32000-1 §7.3.8 / PRD §8.1 | green |
| `OBJ-014` | stream EOL after `stream` keyword consumed (CRLF and bare LF) | ISO 32000-1 §7.3.8 | green |
| `OBJ-015` | truncated indirect object → typed `Err`, no panic | PRD §8.1 | green |
| `OBJ-016` | unexpected closing delimiter / odd dict token count → typed `Err`, no crash | PRD §8.1 | green |

### Serializer (`SER-*`)

| ID | feature | spec ref | status |
|---|---|---|---|
| `SER-001` | scalars: `null`, booleans, integer, real (canonical formatting) | ISO 32000-1 §7.3 | green |
| `SER-002` | name re-encoded with `#XX` for delimiters / non-regular bytes | ISO 32000-1 §7.3.5 | green |
| `SER-003` | literal string re-escaped to canonical literal form | ISO 32000-1 §7.3.4.2 | green |
| `SER-004` | hex string emitted as `<…>` uppercase | ISO 32000-1 §7.3.4.3 | green |
| `SER-005` | array round-trips; dict keys emitted in BTreeMap order (deterministic) | PRD §9.2 | green |
| `SER-006` | stream emits a correct `/Length` for the payload | ISO 32000-1 §7.3.8 | green |
| `SER-007` | `write_indirect(ObjRef, &Object)` emits `N G obj … endobj` | ISO 32000-1 §7.3.10 | green |
| `SER-PROP-001` | `parse(serialize(o)) == normalize(o)` over generated `Object` | PRD §10.7 | green |

---

## M1b — Stream filters + predictors (`pdf-core::filters`)

Spec source of truth: ISO 32000-1 §7.4 (filters), §7.4.4 (Flate + predictors),
§7.4.4.4 (LZW), RFC 1950/1951 (zlib/deflate), TIFF 6.0 §14 (PNG/TIFF
predictors). Implements PRD §8.3 (filters/codecs) at the §10.7 granularity.
Tests live in `crates/pdf-core/tests/flate_unit.rs`,
`crates/pdf-core/tests/lzw_unit.rs`, `crates/pdf-core/tests/ascii_unit.rs`,
`crates/pdf-core/tests/runlength_unit.rs`,
`crates/pdf-core/tests/predictor_unit.rs`,
`crates/pdf-core/tests/dispatch_unit.rs` (unit),
`crates/pdf-core/tests/filters_property.rs` (property) and
`crates/pdf-core/tests/limits_unit.rs`. Design center (PRD §8.1/§9.6): every
decoder is **total** — arbitrary/truncated/corrupt input yields a typed `Err`,
never a panic; every decoder respects `Limits::max_decompressed_stream`.

### FlateDecode (`filters::flate`) — `FLATE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FLATE-DEC-001` | empty input round-trips: `decode(encode(b"")) == b""` | RFC 1950 | green |
| `FLATE-DEC-002` | `decode(encode(b"hello")) == b"hello"` | RFC 1950 | green |
| `FLATE-DEC-003` | known zlib bytes → precomputed expected | RFC 1950 | green |
| `FLATE-DEC-004` | 64 KiB random round-trip | RFC 1950 | green |
| `FLATE-DEC-005` | `b"A"*100000` round-trips and compresses (out < in) | RFC 1951 | green |
| `FLATE-DEC-006` | truncated zlib stream → typed `Err`, no panic | PRD §8.1/§8.3 | green |
| `FLATE-DEC-007` | corrupted middle bytes → typed `Err`, no panic | PRD §8.3 | green |
| `FLATE-DEC-008` | trailing garbage after valid stream → valid prefix (policy) | PRD §8.3 | green |
| `FLATE-DEC-009` | raw deflate (no zlib header) → decoded (raw fallback policy) | PRD §8.3 | green |
| `FLATE-DEC-010` | declared/effective output > tiny limit → `LimitExceeded`, bounded | PRD §9.6.2 | green |

### Predictors (`filters::predictor`) — `FLATE-PRED-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FLATE-PRED-001` | predictor 1 (none) is identity (decode + encode) | ISO 32000-1 §7.4.4.4 | green |
| `FLATE-PRED-002` | PNG Sub (predictor 11) round-trip | TIFF 6.0 / PNG | green |
| `FLATE-PRED-003` | PNG Up (predictor 12) round-trip | TIFF 6.0 / PNG | green |
| `FLATE-PRED-004` | PNG Average (predictor 13) round-trip | TIFF 6.0 / PNG | green |
| `FLATE-PRED-005` | PNG Paeth (predictor 14) round-trip incl. tie-break | TIFF 6.0 / PNG | green |
| `FLATE-PRED-006` | PNG optimum (predictor 15) multi-row, mixed tag bytes | PNG | green |
| `FLATE-PRED-007` | TIFF predictor 2 round-trip | TIFF 6.0 §14 | green |
| `FLATE-PRED-008` | Colors/BitsPerComponent/Columns stride matrix (incl. sub-byte BPC) | ISO 32000-1 §7.4.4.4 | green |
| `FLATE-PRED-009` | `/Columns` mismatch (row stride ∤ data) → typed `Err` | PRD §8.3 | green |

### LZWDecode (`filters::lzw`) — `LZW-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LZW-DEC-001` | empty round-trip `decode(encode(b"")) == b""` | ISO 32000-1 §7.4.4.2 | green |
| `LZW-DEC-002` | `decode(encode(b"hello..")) == input` | ISO 32000-1 §7.4.4.2 | green |
| `LZW-DEC-003` | known spec example (`-----A---B`) decodes to precomputed | ISO 32000-1 §7.4.4.2 | green |
| `LZW-DEC-004` | EarlyChange=1 (default) vs EarlyChange=0 differ; each round-trips | ISO 32000-1 §7.4.4.2 | green |
| `LZW-DEC-005` | larger random round-trip (EarlyChange=1) | ISO 32000-1 §7.4.4.2 | green |
| `LZW-DEC-006` | truncated/corrupt code stream → typed `Err`, no panic | PRD §8.3 | green |
| `LZW-DEC-007` | declared/effective output > tiny limit → `LimitExceeded` | PRD §9.6.2 | green |
| `LZW-DEC-008` | predictor applies to LZW output (PNG Up over LZW) | ISO 32000-1 §7.4.4 | green |

### ASCIIHexDecode (`filters::ascii_hex`) — `AHX-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `AHX-DEC-001` | empty / lone `>` → empty | ISO 32000-1 §7.4.2 | green |
| `AHX-DEC-002` | `48656C6C6F>` → `b"Hello"` | ISO 32000-1 §7.4.2 | green |
| `AHX-DEC-003` | whitespace between digits skipped | ISO 32000-1 §7.4.2 | green |
| `AHX-DEC-004` | odd digit count before `>` → pad trailing `0` | ISO 32000-1 §7.4.2 | green |
| `AHX-DEC-005` | bytes after `>` ignored; missing `>` tolerated at EOF | ISO 32000-1 §7.4.2 | green |
| `AHX-DEC-006` | non-hex non-whitespace char → typed `Err`, no panic | PRD §8.3 | green |
| `AHX-DEC-007` | round-trip `decode(encode(x)) == x` | ISO 32000-1 §7.4.2 | green |

### ASCII85Decode (`filters::ascii85`) — `A85-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `A85-DEC-001` | empty / lone `~>` → empty | ISO 32000-1 §7.4.3 | green |
| `A85-DEC-002` | known group → precomputed 4 bytes | ISO 32000-1 §7.4.3 | green |
| `A85-DEC-003` | `z` shortcut → 4 zero bytes | ISO 32000-1 §7.4.3 | green |
| `A85-DEC-004` | partial final group (2/3/4 chars) decodes to 1/2/3 bytes | ISO 32000-1 §7.4.3 | green |
| `A85-DEC-005` | whitespace skipped; `~>` terminator; optional `<~` lead tolerated | ISO 32000-1 §7.4.3 | green |
| `A85-DEC-006` | out-of-range char / `z` mid-group / 1-char final group → typed `Err` | PRD §8.3 | green |
| `A85-DEC-007` | round-trip `decode(encode(x)) == x` | ISO 32000-1 §7.4.3 | green |

### RunLengthDecode (`filters::run_length`) — `RL-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `RL-DEC-001` | empty / lone `128` (EOD) → empty | ISO 32000-1 §7.4.5 | green |
| `RL-DEC-002` | literal run (length 0..127 → copy n+1 bytes) | ISO 32000-1 §7.4.5 | green |
| `RL-DEC-003` | replicate run (length 129..255 → 257-n copies) | ISO 32000-1 §7.4.5 | green |
| `RL-DEC-004` | `128` byte terminates; trailing bytes ignored | ISO 32000-1 §7.4.5 | green |
| `RL-DEC-005` | truncated run (length byte then EOF) → typed `Err`, no panic | PRD §8.3 | green |
| `RL-DEC-006` | round-trip `decode(encode(x)) == x` | ISO 32000-1 §7.4.5 | green |

### Dispatcher (`filters::decode_stream`) — `DISPATCH-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `DISPATCH-001` | single `/Filter /FlateDecode` decodes | ISO 32000-1 §7.4.1 | green |
| `DISPATCH-002` | no `/Filter` → bytes returned verbatim | ISO 32000-1 §7.4.1 | green |
| `DISPATCH-003` | filter chain `[ASCII85Decode FlateDecode]` applied in order | ISO 32000-1 §7.4.1 | green |
| `DISPATCH-004` | abbreviations `Fl/LZW/AHx/A85/RL` accepted | ISO 32000-1 §7.4.1 (inline) | green |
| `DISPATCH-005` | `/DecodeParms` predictor applied to its filter | ISO 32000-1 §7.4.4.4 | green |
| `DISPATCH-006` | `/DecodeParms` array with null entries handled | ISO 32000-1 §7.4.1 | green |
| `DISPATCH-007` | image filter (`DCTDecode`) → leave-encoded outcome, not error | PRD §8.3 | green |
| `DISPATCH-008` | image filter mid-chain → leave-encoded from that point | PRD §8.3 | green |
| `DISPATCH-009` | unknown filter name → typed `Err`, no panic | PRD §8.3 | green |
| `DISPATCH-010` | `StreamObj::decoded` produces `StreamData::Decoded` lazily | PRD §9.2 | green |

### Limits / decompression-bomb guard — `LIMITS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LIMITS-DEFAULT-001` | `Limits::default()` matches pinned §9.6.2 values | PRD §9.6.2 | green |
| `LIMITS-BOMB-001` | Flate bomb (small input, huge output) > limit → `LimitExceeded`, bounded mem | PRD §9.6.2 | green |
| `LIMITS-BOMB-002` | LZW bomb > limit → `LimitExceeded`, bounded mem | PRD §9.6.2 | green |
| `LIMITS-BOMB-003` | RunLength bomb > limit → `LimitExceeded`, bounded mem | PRD §9.6.2 | green |

### Property (`filters_property.rs`) — `FILTER-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FLATE-PROP-001` | `flate::decode(flate::encode(x)) == x ∀x` | PRD §10.7 | green |
| `FLATE-PROP-002` | `unpredict(predict(rows,cfg)) == rows ∀ rows,cfg` (PNG + TIFF2) | PRD §10.7 | green |
| `FLATE-PROP-003` | `flate::decode` on arbitrary bytes never panics | PRD §10.7 | green |
| `LZW-PROP-001` | `lzw::decode(lzw::encode(x)) == x ∀x` (EarlyChange=1) | PRD §10.7 | green |
| `LZW-PROP-002` | `lzw::decode` on arbitrary bytes never panics | PRD §10.7 | green |
| `AHX-PROP-001` | `ascii_hex` round-trip + never panics on arbitrary bytes | PRD §10.7 | green |
| `A85-PROP-001` | `ascii85` round-trip + never panics on arbitrary bytes | PRD §10.7 | green |
| `RL-PROP-001` | `run_length` round-trip + never panics on arbitrary bytes | PRD §10.7 | green |

---

## M1c — Xref machinery + `DocumentStore` + lazy object access (`pdf-core`)

Spec source of truth: PRD §8.2 (cross-reference machinery), §9.2 (core data
model / `DocumentStore`), §9.6 / §9.6.1 (security, mmap-truncation, never-panic).
Fixtures are **self-built** in-test (M1a serializer + hand-written xref); no
external/PyMuPDF files. Tests live in `crates/pdf-core/tests/source_unit.rs`,
`xref_unit.rs`, `objstm_unit.rs`, `document_unit.rs`,
`document_property.rs`.

### `Source` — bounds-checked backing bytes (`source.rs`) — `SOURCE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SOURCE-001` | `Source::from_bytes` exposes the bytes verbatim via `bytes()` | PRD §9.2 | green |
| `SOURCE-002` | `Source::Empty` is zero-length, never panics | PRD §9.6.1 | green |
| `SOURCE-003` | `slice(off,len)` returns the in-range subslice | PRD §9.6.1 | green |
| `SOURCE-004` | out-of-bounds offset/len → `Error::Source`, no panic | PRD §9.6.1 | green |
| `SOURCE-005` | `slice` length overflow (off+len wraps) → typed error | PRD §9.6.1 | green |
| `SOURCE-006` | `open(path, mmap: Never)` reads owned bytes (hard-safe mode) | PRD §9.6.1 | green |
| `SOURCE-007` | truncated-tail buffer handled gracefully (no startxref) | PRD §9.6.1 | green |

### Xref — classic table (`xref/table.rs`) — `XREF-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `XREF-001` | `startxref` discovery scans the file tail | PRD §8.2 | green |
| `XREF-002` | classic single-subsection table parses; entries map num→offset | PRD §8.2 | green |
| `XREF-003` | multi-subsection table (disjoint ranges) merges correctly | PRD §8.2 | green |
| `XREF-004` | free entry (`f`) recorded as `XrefEntry::Free` | PRD §8.2 | green |
| `XREF-005` | generation numbers preserved on in-use entries | PRD §8.2 | green |
| `XREF-006` | trailer dict parses (`/Size /Root /Prev …`) | PRD §8.2 | green |
| `XREF-007` | object resolved by offset matches the serialized object | PRD §8.2 | green |
| `XREF-008` | 19-byte / bare-LF entry variant tolerated | PRD §8.2 | green |
| `XREF-009` | multiple `%%EOF` → last `startxref` wins | PRD §8.2 | green |
| `XREF-010` | missing/garbage `startxref` → typed `Error::Xref`, no panic | PRD §8.2 | green |

### Xref — streams (`xref/stream.rs`) — `XREFSTM-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `XREFSTM-001` | `/Type /XRef`, `/W [1 2 1]` decodes all 3 entry types | PRD §8.2 | green |
| `XREFSTM-002` | `/Index` ranges honoured (non-zero start) | PRD §8.2 | green |
| `XREFSTM-003` | predictor-encoded (PNG-up) xref stream decodes | PRD §8.2 | green |
| `XREFSTM-004` | varied `/W` widths (e.g. `[1 3 2]`) parse | PRD §8.2 | green |
| `XREFSTM-005` | type-0 (free) / type-1 (uncompressed) / type-2 (compressed) | PRD §8.2 | green |
| `XREFSTM-006` | default `/W` field of width 0 → default value applied | PRD §8.2 | green |
| `XREFSTM-007` | object resolved through an xref stream matches expected | PRD §8.2 | green |
| `XREFSTM-008` | malformed `/W` (wrong length) → typed error | PRD §8.2 | green |

### Object streams (`objstm.rs`) — `OBJSTM-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `OBJSTM-001` | compressed object resolves identically to an uncompressed one | PRD §8.2 | green |
| `OBJSTM-002` | `/N` / `/First` header pairs parsed; multiple members | PRD §8.2 | green |
| `OBJSTM-003` | second member (index 1) resolves to its object | PRD §8.2 | green |
| `OBJSTM-004` | `/N` exceeding `Limits::max_objstm_objects` → `LimitExceeded` | PRD §9.6.2 | green |
| `OBJSTM-005` | corrupt offset table → typed error, no panic | PRD §8.2 | green |

### `/Prev` chains + multi-revision (`xref`) — `PREV-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PREV-001` | `/Prev` chain followed; older section objects visible | PRD §8.2 | green |
| `PREV-002` | newest-wins: object overridden in later section resolves to new | PRD §8.2 | green |
| `PREV-003` | later section re-frees an object → resolves to free/missing | PRD §8.2 | green |
| `PREV-004` | `/Prev` cycle terminates (no infinite loop), typed handling | PRD §8.2 | green |

### Hybrid-reference (`xref`) — `HYBRID-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `HYBRID-001` | `/XRefStm` overlay: object only in stream resolves | PRD §8.2 | green |
| `HYBRID-002` | object in classic table still resolves (both ways) | PRD §8.2 | green |

### Resolution + lazy arena (`document.rs`) — `RESOLVE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `RESOLVE-001` | first `resolve` parses + caches the `Arc<Object>` | PRD §9.2 | green |
| `RESOLVE-002` | second `resolve` returns the cached `Arc` (same pointer) | PRD §9.2 | green |
| `RESOLVE-003` | reference→reference→value followed transparently | PRD §8.1 | green |
| `RESOLVE-004` | direct self-reference cycle → `Error::ReferenceCycle` | PRD §9.3 | green |
| `RESOLVE-005` | indirect (A→B→A) cycle → `Error::ReferenceCycle` | PRD §9.3 | green |
| `RESOLVE-006` | nesting past `max_recursion_depth` → `LimitExceeded` | PRD §9.6.2 | green |
| `RESOLVE-007` | dangling reference (no xref entry) → `Error::MissingObject` (Strict; Lenient→Null per MODE-006) | PRD §9.3 / §8.2 | green |
| `RESOLVE-008` | `resolve_dict_key` resolves a dict value that is a reference | PRD §9.2 | green |
| `RESOLVE-009` | `root()` returns the catalog ref from the trailer | PRD §9.2 | green |
| `RESOLVE-010` | `get_object(num,gen)` returns the raw (unresolved) object | PRD §9.2 | green |

### Source-backed stream `Raw` decode (`document.rs`) — `STREAM-RAW-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `STREAM-RAW-001` | `StreamData::Raw{off,len}` slices body from `Source` | PRD §9.2 | green |
| `STREAM-RAW-002` | a Flate stream parsed from source decodes to expected bytes | PRD §8.3 | green |
| `STREAM-RAW-003` | `Raw` body length validated against source bounds | PRD §9.6.1 | green |

### Open / header / store (`document.rs`) — `OPEN-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `OPEN-001` | `%PDF-1.7` header → `version == (1,7)`, `header_offset == 0` | PRD §8.2 | green |
| `OPEN-002` | junk before header → `header_offset` bias recorded | PRD §8.2 | green |
| `OPEN-003` | `from_bytes` does not eagerly load all objects (arena empty) | PRD §9.2 | green |
| `OPEN-004` | `parse_was_repaired == false` on a clean file | PRD §8.2 | green |
| `OPEN-005` | catalog `/Version` overrides header version | PRD §8.2 | green |
| `OPEN-006` | full open → resolve `/Root` → catalog dict, end-to-end | PRD §8.2 | green |

### Property / robustness (`document_property.rs`) — `OPEN-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `OPEN-PROP-001` | opening arbitrary bytes never panics (typed `Err` or `Ok`) | PRD §9.6 | green |
| `OPEN-PROP-002` | truncating a valid file at any offset never panics | PRD §9.6 | green |
| `OPEN-PROP-003` | `resolve` of arbitrary obj nums on opened doc never panics | PRD §9.6 | green |

---

## M1d — Malformed-PDF repair / reconstruction (`pdf-core::repair`, `document.rs`)

Spec source of truth: PRD §8 intro (design center: tolerate the garbage),
§8.1 (object-model tolerance), §8.2 (cross-reference + repair subsystem +
Strict/Lenient modes + `parse_was_repaired`), §9.3 (stable error/warning kinds),
§9.6 (never-panic / never-OOM / bounded-work). Tests live in
`crates/pdf-core/tests/repair_unit.rs` (unit) and
`crates/pdf-core/tests/repair_property.rs` (property / never-panic).

### Parse mode plumbing (`document.rs`) — `MODE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `MODE-001` | default `open`/`from_bytes` parse mode is `Lenient` | PRD §8.2 | green |
| `MODE-002` | `open_with(Strict)` on a clean file opens identically | PRD §8.2 | green |
| `MODE-003` | Strict: broken xref (missing startxref) surfaces typed `Error::Xref` | PRD §8.2 | green |
| `MODE-004` | Lenient: same broken xref repairs and opens | PRD §8.2 | green |
| `MODE-005` | Strict: dangling ref → typed `Error::MissingObject` (no Null) | PRD §8.2 | green |
| `MODE-006` | Lenient: dangling ref resolves to `Null` | PRD §8.2 | green |

### Full-file object scan / synthetic xref (`repair.rs`) — `REPAIR-XREF-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-XREF-001` | missing `startxref` → scan rebuilds xref, objects resolve | PRD §8.2 | green |
| `REPAIR-XREF-002` | garbage `startxref` offset → scan recovers | PRD §8.2 | green |
| `REPAIR-XREF-003` | xref entries point at wrong offsets → scan finds true offsets | PRD §8.2 | green |
| `REPAIR-XREF-004` | object value after repair equals original value | PRD §8.2 | green |
| `REPAIR-XREF-005` | objects inside an ObjStm are recovered during scan | PRD §8.2 | green |
| `REPAIR-XREF-006` | scan recovers gen numbers; `N G obj` with G>0 found | PRD §8.2 | green |

### Stream `/Length` repair under reconstruction — `REPAIR-LEN-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-LEN-001` | wrong `/Length` (too short) → body re-derived to `endstream` | PRD §8.2 | green |
| `REPAIR-LEN-002` | missing `/Length` → body recovered by scan | PRD §8.2 | green |
| `REPAIR-LEN-003` | recovered stream decodes to original bytes (Flate) | PRD §8.3 | green |

### Garbage prefix / header bias — `REPAIR-PREFIX-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-PREFIX-001` | N bytes of junk before `%PDF-` + broken xref → opens via scan | PRD §8.2 | green |
| `REPAIR-PREFIX-002` | scanned offsets are absolute (resolve correct under bias) | PRD §8.2 | green |

### Truncated tail — `REPAIR-TRUNC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-TRUNC-001` | file cut after some objects (no trailer) → salvages survivors | PRD §8.2 | green |
| `REPAIR-TRUNC-002` | truncation mid-object → complete objects still resolve | PRD §8.2 | green |
| `REPAIR-TRUNC-003` | catalog survives truncation → doc opens, Root resolves | PRD §8.2 | green |

### Trailer reconstruction — `REPAIR-TRAILER-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-TRAILER-001` | missing trailer → `/Root` rebuilt from `/Type /Catalog` | PRD §8.2 | green |
| `REPAIR-TRAILER-002` | synthetic trailer carries a `/Size` ≥ max obj num + 1 | PRD §8.2 | green |
| `REPAIR-TRAILER-003` | multiple catalogs → last (by obj num order) wins as `/Root` | PRD §8.2 | green |

### Dangling references — `REPAIR-DANGLING-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-DANGLING-001` | Lenient: ref to non-existent object resolves to `Null` | PRD §8.1 | green |
| `REPAIR-DANGLING-002` | Lenient: dangling ref inside a dict value → `Null` | PRD §8.1 | green |

### Duplicate object numbers (revisions) — `REPAIR-DUP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-DUP-001` | duplicate `N G obj` across body → last definition wins | PRD §8.2 | green |
| `REPAIR-DUP-002` | last-wins survives header bias / prefix | PRD §8.2 | green |

### Validation gate (`document.rs`) — `REPAIR-GATE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-GATE-001` | clean parse whose `/Root` is unreachable → auto-repairs | PRD §8.2 | green |
| `REPAIR-GATE-002` | clean parse whose `/Pages` is unreachable → auto-repairs | PRD §8.2 | green |
| `REPAIR-GATE-003` | valid file passes gate without triggering repair | PRD §8.2 | green |

### Diagnostics / report — `REPAIR-REPORT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-REPORT-001` | `parse_was_repaired == true` after a scan-path open | PRD §8.2 | green |
| `REPAIR-REPORT-002` | `repair_report()` lists the reconstruction actions taken | PRD §8.2 | green |
| `REPAIR-REPORT-003` | `warnings()` collects `Warning { offset, kind, detail }` | PRD §9.3 | green |
| `REPAIR-REPORT-004` | warning `kind` discriminant strings are stable / English | PRD §9.3 | green |
| `REPAIR-REPORT-005` | clean open → empty report, `parse_was_repaired == false` | PRD §8.2 | green |

### Never-panic / never-hang / bounded-work (`repair_property.rs`) — `REPAIR-PANIC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REPAIR-PANIC-001` | opening arbitrary `Vec<u8>` (Lenient) never panics, terminates | PRD §9.6 | green |
| `REPAIR-PANIC-002` | opening arbitrary `Vec<u8>` (Strict) never panics, terminates | PRD §9.6 | green |
| `REPAIR-PANIC-003` | bit-flipped valid PDF never panics; opens or typed `Err` | PRD §9.6 | green |
| `REPAIR-PANIC-004` | truncate-at-any-offset of valid PDF never panics | PRD §9.6 | green |
| `REPAIR-PANIC-005` | object scan honors `max_objects` (no unbounded growth) | PRD §9.6.2 | green |
| `REPAIR-PANIC-006` | resolve of arbitrary obj nums on a repaired doc never panics | PRD §9.6 | green |

---

## M1e — Encryption: Standard Security Handler READ path (`pdf-crypto`)

Spec source of truth: PRD §8.4 (Standard Security Handler R2–R6; per-object key
`min(len+5,16)`; `sAlT` for AESV2 only; R5-read / R6-write; `/ID`-absent
fallback; exemptions), §9.1 (`pdf-core` uses `pdf-crypto` behind the
`encryption` feature), §6.4 (RustCrypto licenses). Tests live in
`crates/pdf-crypto/tests/{kdf_unit,roundtrip_unit,perobj_unit,auth_unit,crypto_property}.rs`
(crypto engine) and `crates/pdf-core/tests/encryption_unit.rs` (DocumentStore
integration, `--features encryption`). Fixtures are **self-generated** via
`pdf_crypto::testsupport` (no external/AGPL files).

### Primitives & KDF known-answers — `CRYPT-KDF-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-KDF-001` | MD5 / SHA-256 / SHA-384 / SHA-512 known-answer vectors | RustCrypto | green |
| `CRYPT-KDF-002` | hand-rolled RC4 matches standard test vectors | ISO 32000 §7.6.2 | green |
| `CRYPT-KDF-003` | AES-128/256-CBC PKCS#7 round-trip; no-pad round-trip | NIST CBC | green |
| `CRYPT-KDF-004` | 32-byte password pad (Algorithm 2 step a) | PRD §8.4 | green |
| `CRYPT-KDF-005` | R2 file key = first 5 bytes of single MD5 | PRD §8.4 | green |
| `CRYPT-KDF-006` | R3/R4 file key iterates MD5 50× to `/Length`/8 | PRD §8.4 | green |
| `CRYPT-KDF-007` | R4 `!EncryptMetadata` appends `0xFFFFFFFF` (key differs) | PRD §8.4 | green |
| `CRYPT-KDF-008` | R6 Algorithm 2.B hardened hash is deterministic / stable len | PRD §8.4 | green |
| `CRYPT-KDF-009` | R5 single-SHA-256 hash differs from R6 hardened hash | PRD §8.4 | green |
| `CRYPT-KDF-010` | `/UE` AES-256 no-pad unwrap recovers the planted file key (user) | PRD §8.4 | green |
| `CRYPT-KDF-011` | `/OE` AES-256 no-pad unwrap recovers the planted file key (owner) | PRD §8.4 | green |

### Per-object key derivation — `CRYPT-PEROBJ-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-PEROBJ-001` | RC4 object key = `min(len+5,16)` of MD5(key‖num‖gen) (no sAlT) | PRD §8.4 | green |
| `CRYPT-PEROBJ-002` | AESV2 object key appends `"sAlT"` → differs from the RC4 key | PRD §8.4 | green |
| `CRYPT-PEROBJ-003` | object key truncation caps at 16 bytes for a 16-byte file key | PRD §8.4 | green |
| `CRYPT-PEROBJ-004` | AESV3 uses the file key directly (no per-object derivation) | PRD §8.4 | green |
| `CRYPT-PEROBJ-005` | num/gen are little-endian 3/2 bytes (object-number sensitivity) | PRD §8.4 | green |

### Round-trip decrypt (encrypt → reopen → authenticate → bytes equal) — `CRYPT-{RC4,AESV2,AESV3,R5}-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-RC4-40-001` | R2 RC4-40: string + stream round-trip, empty pwd | PRD §8.4 | green |
| `CRYPT-RC4-128-001` | R3 RC4-128: string + stream round-trip, empty pwd | PRD §8.4 | green |
| `CRYPT-RC4-128-002` | R4 RC4-128 via crypt filters (`/StmF`=`/StrF`=`StdCF` V2) | PRD §8.4 | green |
| `CRYPT-AESV2-001` | R4 AES-128: IV-prepended, PKCS#7 round-trip, empty pwd | PRD §8.4 | green |
| `CRYPT-AESV2-002` | R4 AES-128: distinct objects use distinct per-object keys | PRD §8.4 | green |
| `CRYPT-AESV3-R6-001` | R6 AES-256: string + stream round-trip, empty pwd | PRD §8.4 | green |
| `CRYPT-AESV3-R6-002` | R6 AES-256: non-empty user password round-trip | PRD §8.4 | green |
| `CRYPT-R5-001` | R5 AES-256 transitional: round-trip decrypt (read-only) | PRD §8.4 | green |

### Authentication roles — `CRYPT-OWNER-*` / `CRYPT-WRONGPW-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-OWNER-001` | R3/R4: owner password authenticates as `Owner` | PRD §8.4 | green |
| `CRYPT-OWNER-002` | R6: owner password authenticates as `Owner`, recovers key | PRD §8.4 | green |
| `CRYPT-OWNER-003` | user password authenticates as `User` (role reported) | PRD §8.4 | green |
| `CRYPT-WRONGPW-001` | R4: wrong password → `Err(NeedsPassword)`, no panic | PRD §8.4 | green |
| `CRYPT-WRONGPW-002` | R6: wrong password → `Err(NeedsPassword)`, no panic | PRD §8.4 | green |
| `CRYPT-WRONGPW-003` | decrypt before authenticate → `Err(NeedsPassword)` | PRD §8.4 | green |

### `/ID`-absent fallback — `CRYPT-ID-ABSENT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-ID-ABSENT-001` | R3 with empty `/ID[0]` still derives a key & round-trips | PRD §8.4 | green |

### Exemptions (what is NOT decrypted) — `CRYPT-EXEMPT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-EXEMPT-001` | `/Identity` crypt method is a verbatim no-op | PRD §8.4 | green |
| `CRYPT-EXEMPT-002` | DocumentStore: `/Encrypt` dict strings (`/O`/`/U`) not decrypted | PRD §8.4 | green |
| `CRYPT-EXEMPT-003` | DocumentStore: XRef stream (`/Type /XRef`) not decrypted | PRD §8.4 | green |
| `CRYPT-EXEMPT-004` | `EncryptMetadata=false` leaves the `/Metadata` stream clear | PRD §8.4 | green |
| `CRYPT-EXEMPT-005` | strings inside an ObjStm are decrypted via the container only | PRD §8.4 | green |

### Never-panic / typed-error (proptest) — `CRYPT-PANIC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-PANIC-001` | garbage `/Encrypt` fields → typed error, never panic | PRD §9.6 | green |
| `CRYPT-PANIC-002` | random key material / data → decrypt is typed `Err` or bytes, no panic | PRD §9.6 | green |
| `CRYPT-PANIC-003` | random AES object data (< IV, bad padding) → typed `Err`, no panic | PRD §9.6 | green |
| `CRYPT-PANIC-004` | arbitrary password against a valid fixture → `Ok`/`NeedsPassword`, no panic | PRD §9.6 | green |

### DocumentStore integration (`--features encryption`) — `CRYPT-DOC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-DOC-001` | encrypted doc opens; `needs_pass()` true before authenticate | PRD §9.1 | green |
| `CRYPT-DOC-002` | `authenticate("")` then `resolve()` yields decrypted strings | PRD §8.4 | green |
| `CRYPT-DOC-003` | `authenticate("")` then `decode_stream()` yields decrypted bytes | PRD §8.4 | green |
| `CRYPT-DOC-004` | unencrypted doc: `needs_pass()` false, resolve unchanged | PRD §9.1 | green |
| `CRYPT-DOC-005` | default build (no `encryption` feature) compiles & opens plain docs | PRD §9.1 | green |

---

## M1f — Page tree + Document/Page facade + PyO3 + fitz shim

Spec source: PRD §7 (M1 rows), §8.6.1 (rotation), §9.2 (`Page` shape), §9.4
(PyO3 handle/index), §9.5 (fitz shim). Rust tests live in
`crates/pdf-core/tests/pagetree_unit.rs` and
`crates/pdf-api/tests/document_unit.rs`; Python tests in `python/tests/`.

### Page tree + inheritance — `PAGE-*` (`pdf-core::pagetree`)

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGE-COUNT-001` | `page_count` via nested `/Kids` + `/Count` (multi-level tree) | PRD §7 | green |
| `PAGE-COUNT-002` | `page_refs` order is document order across subtrees | PRD §7 | green |
| `PAGE-INHERIT-001` | leaf inherits `/MediaBox` from ancestor `/Pages` | PRD §8.2 | green |
| `PAGE-INHERIT-002` | leaf inherits `/Rotate` from ancestor; own value overrides | PRD §8.2 | green |
| `PAGE-INHERIT-003` | leaf `/MediaBox` overrides inherited ancestor box | PRD §8.2 | green |
| `PAGE-BOX-001` | `rect`/`bound` == `CropBox ∩ MediaBox` | PRD §9.2 | green |
| `PAGE-BOX-002` | absent `/MediaBox` → US Letter default (612×792) | PRD §9.2 | green |
| `PAGE-BOX-003` | absent `/CropBox` → equals `MediaBox` | PRD §9.2 | green |
| `PAGE-ROT-001` | rotation normalizes `-90/450 → 270/90`; non-multiple-of-90 → 0 | PRD §8.6.1 | green |
| `PAGE-LIMITS-001` | `/Kids` cycle is broken (no hang); depth/count bounded | PRD §9.6 | green |

### Broken page-tree fallback — `PAGETREE-FALLBACK-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGETREE-FALLBACK-001` | unreachable `/Pages` → scan `/Type /Page` recovers pages | PRD §8.2 | green |
| `PAGETREE-FALLBACK-002` | recovered pages are in object-number order | PRD §8.2 | green |

### Document/Page facade — `DOC-*` (`pdf-api`)

| ID | feature | spec ref | status |
|---|---|---|---|
| `DOC-OPEN-001` | `Document::open_bytes` opens; `page_count`/`load_page` work | PRD §7 | green |
| `DOC-OPEN-002` | `Document::open` (path) opens a self-written file | PRD §7 | green |
| `DOC-PAGE-001` | `load_page` out of range → typed error, no panic | PRD §7 | green |
| `DOC-PAGE-002` | `pages()` iterator yields every page with correct `number` | PRD §7 | green |
| `DOC-META-001` | `metadata` parses `/Info` (title/author/producer/dates) | PRD §7 | green |
| `DOC-META-002` | `metadata.format` == `"PDF 1.7"`; absent fields empty | PRD §7 | green |
| `DOC-META-003` | UTF-16BE BOM `/Info` value decodes to text | PRD §8.7 | green |
| `DOC-REPAIR-001` | broken file → `is_repaired()` true after repair open | PRD §8.2 | green |
| `DOC-XREF-001` | `xref_length` == `/Size`; `xref_object` round-trips a dict | PRD §7 | green |
| `DOC-XREF-002` | `xref_get_key`/`xref_is_stream`/`xref_stream` on a stream | PRD §7 | green |

### Encrypted Document flow (`--features encryption`) — `DOC-CRYPT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `DOC-CRYPT-001` | encrypted doc: `is_encrypted`/`needs_pass` true; `permissions` | PRD §8.4 | green |
| `DOC-CRYPT-002` | `authenticate("")` → `needs_pass` false; pages load | PRD §8.4 | green |
| `DOC-CRYPT-003` | wrong password → `authenticate` false, no panic | PRD §8.4 | green |

### Python wheel (`oxipdf` / `fitz`) — `PYDOC-*` / `PYFITZ-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYDOC-001` | `oxipdf.open(path)`: `page_count`/`len`/index/`load_page` | PRD §9.4 | green |
| `PYDOC-002` | `page.rect`/`rotation`/`number`/`bound()`/`mediabox`/`cropbox` | PRD §9.2 | green |
| `PYDOC-003` | `doc.metadata` dict has all PyMuPDF keys | PRD §9.5 | green |
| `PYDOC-004` | unimplemented known method raises `PdfUnsupportedError` | PRD §9.5 | green |
| `PYFITZ-001` | `fitz.open(...)`: `page_count`/`doc[n]`/`metadata`/geometry | PRD §9.5 | green |
| `PYFITZ-002` | encrypted: `needs_pass`→`authenticate`→pages (fitz names) | PRD §8.4 | green |
| `PYFITZ-003` | `fitz.Rect`/`Matrix` value types match PyMuPDF arithmetic | PRD §9.5 | green |

---

## M2a — Font mapping layer (`pdf-fonts`)

Spec source of truth: PRD §8.5 (Fonts — mapping only, no rasterization) + ISO
32000-1 §9.6–§9.7 + §9.10 (encodings, CMaps, CID fonts, ToUnicode), Annex D
(base encodings — public-domain facts) and the Adobe Glyph List + ZapfDingbats
glyph list (both BSD-3-Clause Adobe, vendored byte-for-byte in
`crates/pdf-fonts/data/` with provenance in `data/PROVENANCE.md` /
`data/NOTICE`). The `FontMapper` is built from a resolved
font dict + `&DocumentStore`; it answers `iter_codes`, `to_unicode(code)` and
`width(code)`. No rasterization (that is M6). Tests live in
`crates/pdf-fonts/tests/`.

> **Core-14 AFM gap (PRD §6.5 #2 / §8.5.2).** No recognized-permissive (SPDX
> MIT/BSD/Apache) source for Core-14 AFM width metrics was established for this
> milestone; per the project's license-cleanliness thesis no license-uncertain
> width data is embedded. The Core-14 framework (font-name normalization +
> lookup hook) is implemented but the bundled width table is empty, so unembedded
> standard-14 fonts without `/Widths` fall back to `/MissingWidth` then the
> notdef width. Documented as `WIDTHS-CORE14-GAP`.

### Base encodings + `/Differences` (`encodings.rs`) — `ENCODING-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ENCODING-001` | WinAnsi `0x41`→`A`→U+0041; `0x80`→`Euro`→U+20AC | ISO Annex D | green |
| `ENCODING-002` | StandardEncoding `0xA1`→`exclamdown`→U+00A1 | ISO Annex D | green |
| `ENCODING-003` | MacRoman `0x80`→`Adieresis`→U+00C4 | ISO Annex D | green |
| `ENCODING-004` | PDFDocEncoding `0xA0`→`Euro`→U+20AC; `0x18`→breve | ISO Annex D | green |
| `ENCODING-005` | Symbol built-in `0x61`→`alpha`→U+03B1 | ISO Annex D | green |
| `ENCODING-006` | ZapfDingbats built-in `0x41`→`a10`→U+2721, `0x61`→`a60`→U+2741 | ISO Annex D | green |
| `ENCODING-007` | `/Encoding` name → that base table | ISO §9.6.6 | green |
| `ENCODING-008` | `/Encoding` dict `/BaseEncoding`+`/Differences` override | ISO §9.6.6 | green |
| `ENCODING-009` | `/Differences` over implicit base (no `/BaseEncoding`) | ISO §9.6.6 | green |
| `ENCODING-010` | TrueType symbolic w/o `/Encoding` → Standard default | ISO §9.6.6 | green |
| `ENCODING-011` | unmapped simple code → `to_unicode` None, never panic | PRD §8.5 | green |

### Glyph-name → Unicode (AGL + algorithmic) (`glyphlist.rs`) — `GLYPHLIST-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GLYPHLIST-001` | AGL `quotedblleft`→U+201C; `Euro`→U+20AC | AGL / Adobe | green |
| `GLYPHLIST-002` | AGL ligature `fi`→U+FB01 | AGL / Adobe | green |
| `GLYPHLIST-003` | `uniXXXX` (`uni20AC`→U+20AC) | AGL algorithm | green |
| `GLYPHLIST-004` | `uXXXXXX` (`u1F600`→U+1F600) | AGL algorithm | green |
| `GLYPHLIST-005` | underscore ligature `f_f_i`→ U+0066 U+0066 U+0069 | AGL algorithm | green |
| `GLYPHLIST-006` | `.`-suffix strip (`a.sc`→ glyph `a`→U+0061) | AGL algorithm | green |
| `GLYPHLIST-007` | `cidNN` / `gNN` / `.notdef` → unresolved (None) | PRD §8.5 | green |
| `GLYPHLIST-008` | unknown name → None, never panic | PRD §8.5 | green |

### CMap parser (shared) (`cmap.rs`) — `CMAP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CMAP-001` | ToUnicode `beginbfchar` single byte → U+ | ISO §9.10.3 | green |
| `CMAP-002` | ToUnicode `beginbfrange` (lo,hi,base) increment form | ISO §9.10.3 | green |
| `CMAP-003` | ToUnicode `beginbfrange` array-of-dst form | ISO §9.10.3 | green |
| `CMAP-004` | UTF-16BE multi-unit value (surrogate pair → astral) | ISO §9.10.3 | green |
| `CMAP-005` | 1-to-many (ligature) bf value → multi-char string | ISO §9.10.3 | green |
| `CMAP-006` | `begincodespacerange` drives 1- vs 2-byte decode | ISO §9.7.6 | green |
| `CMAP-007` | `begincidchar` / `begincidrange` parse → CID | ISO §9.7.5 | green |
| `CMAP-008` | `usecmap` chaining merges parent ranges | ISO §9.7.5 | green |
| `CMAP-009` | malformed CMap tokens skipped, never panic | PRD §8.5 | green |
| `CMAP-010` | mixed 1-and-2-byte codespace ranges decode by prefix | ISO §9.7.6 | green |

### `iter_codes` (codespace-driven) (`mapper.rs`) — `ITERCODES-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ITERCODES-001` | simple font: 1 byte/code over whole string | ISO §9.4.3 | green |
| `ITERCODES-002` | Identity-H: 2 bytes/code, code==CID | ISO §9.7.5 | green |
| `ITERCODES-003` | embedded codespace: variable-length per prefix | ISO §9.7.6 | green |
| `ITERCODES-004` | odd trailing byte consumed as 1-byte (no panic) | PRD §8.5 | green |

### Simple-font widths (`widths.rs`) — `WIDTHS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `WIDTHS-001` | `/Widths` indexed by `code - /FirstChar` | ISO §9.2.4 | green |
| `WIDTHS-002` | code outside `/Widths` range → `/MissingWidth` | ISO §9.2.4 | green |
| `WIDTHS-003` | absent `/MissingWidth` → 0 | ISO §9.2.4 | green |
| `WIDTHS-004` | NaN / negative / absurd width clamped to 0 | PRD §8.5 | green |
| `WIDTHS-CORE14-GAP` | unembedded std-14, no `/Widths` → MissingWidth fallback (AFM gap) | PRD §8.5.2 | green |

### Type0 / CID fonts (`mapper.rs` + `widths.rs`) — `CID-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CID-001` | Identity-H code==CID; `/ToUnicode` extraction | ISO §9.7.4 | green |
| `CID-002` | `/W` array form `[c [w0 w1 …]]` | ISO §9.7.4.3 | green |
| `CID-003` | `/W` range form `[c_first c_last w]` | ISO §9.7.4.3 | green |
| `CID-004` | `/DW` default applied to CID outside `/W` | ISO §9.7.4.3 | green |
| `CID-005` | absent `/DW` → default 1000 | ISO §9.7.4.3 | green |
| `CID-006` | CIDToGIDMap Identity (default) | ISO §9.7.4.3 | green |
| `CID-007` | CIDToGIDMap stream maps CID→GID | ISO §9.7.4.3 | green |
| `CID-008` | embedded CMap stream `/Encoding` code→CID | ISO §9.7.5.3 | green |
| `CID-009` | Type0 without `/ToUnicode` → None (documented CJK gap) | PRD §8.5 | green |

### `FontMapper` orchestration (`mapper.rs`) — `FONTMAP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FONTMAP-001` | simple Type1: `/ToUnicode` OVERRIDES encoding+AGL | PRD §8.5 | green |
| `FONTMAP-002` | Type3 simple-font path (encoding/Widths) | PRD §8.5 | green |
| `FONTMAP-003` | predefined CMap framework: Identity-H/V resolved | ISO §9.7.5.2 | green |
| `FONTMAP-004` | unknown predefined CMap name → documented gap, no panic | PRD §8.5 | green |

### Property / never-panic (`fontmap_property.rs`) — `FONTMAP-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FONTMAP-PROP-001` | `iter_codes` covers whole input, no overlap, lengths sum | PRD §8.5 | green |
| `FONTMAP-PROP-002` | `iter_codes` never panics on arbitrary bytes | PRD §8.5 | green |
| `FONTMAP-PROP-003` | `to_unicode` on arbitrary code never panics → Option | PRD §8.5 | green |
| `FONTMAP-PROP-004` | `width` on arbitrary code never panics, finite ≥ 0 | PRD §8.5 | green |

---

## M2b — Content-stream interpreter → positioned glyphs (`pdf-text`)

Spec source of truth: PRD §8.6.1 (Trm math, row-vector convention) + §8.6.2
(interpreter operator subset) + ISO 32000-1 §9.4 (text objects/operators), §8.4
(graphics state). The `ContentInterpreter` runs a page's decoded content
stream(s) and emits a flat `Vec<PositionedGlyph>` in **PDF user space** (no page
transform / no layout grouping — that is M2c/M2d). Self-constructed content +
font fixtures only (we control every byte; no PyMuPDF files). Tests live in
`crates/pdf-text/tests/`.

### Operator interpreter + advance (`interp.rs`) — `INTERP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INTERP-001` | `Tj` at a known `Tm` → glyph origins at expected user-space coords | PRD §8.6.1 | green |
| `INTERP-002` | per-glyph advance `tx = (w0/1000·Tfs + Tc)·Th` | ISO §9.4.4 | green |
| `INTERP-003` | `Tw` adds to advance only on single-byte code 0x20 | ISO §9.4.3 | green |
| `INTERP-004` | `Tz` horizontal scaling scales advance + Trm x-scale | ISO §9.4.4 | green |
| `INTERP-005` | `Tc` char spacing adds to every glyph advance | ISO §9.4.4 | green |
| `INTERP-006` | `TJ` numeric kerning shifts by `-adj/1000·Tfs·Th` | ISO §9.4.3 | green |
| `INTERP-007` | `Td` moves text line matrix; origin shifts | ISO §9.4.2 | green |
| `INTERP-008` | `TD` sets leading = `-ty` then `Td` | ISO §9.4.2 | green |
| `INTERP-009` | `T*` advances one line by current leading `TL` | ISO §9.4.2 | green |
| `INTERP-010` | `Tm` replaces text + line matrix absolutely | ISO §9.4.2 | green |
| `INTERP-011` | `'` operator = `T*` then `Tj` | ISO §9.4.3 | green |
| `INTERP-012` | `"` operator sets `Tw`/`Tc` then `'` | ISO §9.4.3 | green |
| `INTERP-013` | `q`/`Q` save/restore CTM + text state | ISO §8.4.2 | green |
| `INTERP-014` | `cm` pre-concats CTM; composes with `Tm` | ISO §8.3.4 | green |
| `INTERP-015` | `Ts` text rise offsets glyph origin in y | ISO §9.4.4 | green |
| `INTERP-016` | `Tr` render mode recorded on glyph | ISO §9.4.4 | green |
| `INTERP-017` | `Tr 3` (invisible) glyph still emitted, tagged | PRD §8.6.2 | green |
| `INTERP-018` | fill color `g`/`rg`/`k` → packed sRGB on glyph | ISO §8.6.8 | green |
| `INTERP-019` | multiple `/Contents` streams concatenated w/ separator | PRD §8.6.2 | green |
| `INTERP-020` | Type0 Identity-H 2-byte show + `/W` advance | ISO §9.7.4 | green |

### Text rendering matrix + rotation envelope (`interp.rs`) — `TRM-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `TRM-001` | `Trm = params·Tm·CTM`; glyph origin = `(0,0)·Trm` | PRD §8.6.1 | green |
| `TRM-002` | bbox height from `/Ascent`/`/Descent` scaled by size | PRD §8.6.2 | green |
| `TRM-003` | font-size scaling scales bbox + advance linearly | ISO §9.4.4 | green |
| `TRM-004` | translation `Tm` offsets origin/bbox | PRD §8.6.1 | green |
| `COORD-ROT-90-TRM` | 90°-rotated `Tm` → correct axis-aligned bbox envelope | PRD §8.6.1 | green |
| `COORD-ROT-180-TRM` | 180°-rotated `Tm` → correct envelope + origin | PRD §8.6.1 | green |

### Form XObject recursion (`interp.rs`) — `INTERP-FORM-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INTERP-FORM-001` | `Do` Form XObject places nested text with form `/Matrix` | ISO §8.10 | green |
| `INTERP-FORM-002` | nested form `/Resources` resolves its own fonts | ISO §8.10 | green |
| `INTERP-FORM-003` | recursion depth cap halts deep nesting (no overflow) | PRD §8.6.2 | green |
| `INTERP-FORM-004` | self-referential form cycle guarded (no infinite loop) | PRD §8.6.2 | green |
| `INTERP-FORM-005` | Image XObject `Do` records presence, emits no glyph | PRD §8.6.2 | green |

### Inline images (`interp.rs`) — `INTERP-INLINE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INTERP-INLINE-001` | `BI/ID/EI` binary body skipped; following `Tj` intact | ISO §8.9.7 | green |
| `INTERP-INLINE-002` | inline-image presence/metadata captured (not decoded) | PRD §8.6.2 | green |
| `INTERP-INLINE-003` | `EI`-like bytes inside the body don't terminate early | ISO §8.9.7 | green |

### Robustness / never-panic (`interp_property.rs`) — `INTERP-ROBUST-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INTERP-ROBUST-001` | arbitrary bytes as content never panic | PRD §8.1 | green |
| `INTERP-ROBUST-002` | unknown operators skipped; operand underflow tolerated | PRD §8.6.2 | green |
| `INTERP-ROBUST-003` | truncated `BT`/string/`TJ` array never panic | PRD §8.6.2 | green |
| `INTERP-ROBUST-004` | every emitted glyph has finite bbox/origin | PRD §8.6.2 | green |

### End-to-end (`interp_e2e.rs`) — `INTERP-E2E-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INTERP-E2E-001` | 1-page PDF, two words on two lines → unicode seq + positions | PRD §8.6 | green |
| `INTERP-E2E-002` | `interpret_page` resolves `/Contents` array + `/Resources` | PRD §8.6 | green |

---

## M2c — Layout reconstruction → `TextPage` model (`pdf-text`)

Spec source of truth: PRD §8.6 (text extraction & layout), §8.6.1 (device/page
transform incl. `/Rotate`), §8.6.2 (glyphs→spans→lines→blocks, reading order,
word segmentation, flags), §10.7 (`WORDS-*` shape + dict/rawdict nesting). M2c
groups the interpreter's `Vec<PositionedGlyph>` (PDF user space) into a
PyMuPDF-shaped `TextPage` in **device space** (origin top-left, y down, `/Rotate`
applied), plus a word segmenter — **no serialization (M2d), no search (M2e)**.
Tests live in `crates/pdf-text/tests/layout_*.rs`; glyph lists + small
self-built PDFs (reuse `tests/common`). No PyMuPDF files.

### Device/page transform (`layout.rs`) — `LAYOUT-DEVICE-*` / `COORD-ROT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-DEVICE-001` | y-flip: glyph near page top has small device y | PRD §8.6.1 | green |
| `LAYOUT-DEVICE-002` | `page_transform(r=0)` == `[1,0,0,-1,-x0,y1]`; size `w×h` | PRD §8.6.1 | green |
| `COORD-ROT-0-PAGE` | r=0 device coords inside `[0,w]×[0,h]` | PRD §8.6.1 | green |
| `COORD-ROT-90-PAGE` | `page_transform(r=90)` == `[0,1,1,0,-y0,-x0]`; size `h×w` | PRD §8.6.1 | green |
| `COORD-ROT-180-PAGE` | `page_transform(r=180)` == `[-1,0,0,1,x1,-y0]`; size `w×h` | PRD §8.6.1 | green |
| `COORD-ROT-270-PAGE` | `page_transform(r=270)` == `[0,-1,-1,0,y1,x1]`; size `h×w` | PRD §8.6.1 | green |
| `COORD-ROT-MEDIABOX` | non-zero MediaBox origin baked into transform | PRD §8.6.1 | green |
| `LAYOUT-DEVICE-003` | TextPage width/height match rotated page size | PRD §8.6.1 | green |

### Line grouping (`layout.rs`) — `LAYOUT-LINE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-LINE-001` | glyphs on one baseline → one line | PRD §8.6.2 | green |
| `LAYOUT-LINE-002` | two distinct baselines → two lines | PRD §8.6.2 | green |
| `LAYOUT-LINE-003` | small super/sub rise stays on same line | PRD §8.6.2 | green |
| `LAYOUT-LINE-004` | large vertical gap → separate lines | PRD §8.6.2 | green |
| `LAYOUT-LINE-005` | within a line glyphs sorted by advance order | PRD §8.6.2 | green |

### Span splitting (`layout.rs`) — `LAYOUT-SPAN-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-SPAN-001` | contiguous same-style glyphs merge to one span | PRD §8.6.2 | green |
| `LAYOUT-SPAN-002` | font-name change splits spans | PRD §8.6.2 | green |
| `LAYOUT-SPAN-003` | font-size change splits spans | PRD §8.6.2 | green |
| `LAYOUT-SPAN-004` | color change splits spans | PRD §8.6.2 | green |
| `LAYOUT-SPAN-005` | span text == concatenation of its chars | PRD §10.7 | green |

### Block grouping + reading order (`layout.rs`) — `LAYOUT-BLOCK-*` / `LAYOUT-ORDER-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-BLOCK-001` | lines with small vertical gap group into one block | PRD §8.6.2 | green |
| `LAYOUT-BLOCK-002` | large vertical gap → separate blocks | PRD §8.6.2 | green |
| `LAYOUT-BLOCK-003` | image inventory → image blocks (device bbox) | PRD §8.6.2 | green |
| `LAYOUT-ORDER-001` | single column blocks ordered top-to-bottom | PRD §8.6.2 | green |
| `LAYOUT-ORDER-002` | two-column page → XY-cut yields column-by-column order | PRD §8.6.2 | green |
| `LAYOUT-ORDER-003` | block numbers monotonic in reading order | PRD §8.6.2 | green |

### Word segmentation (`words.rs`) — `WORDS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `WORDS-001` | split a line on literal space chars | PRD §10.7 | green |
| `WORDS-002` | `TJ`-kerned gap with no space char → still split | PRD §8.6.2 | green |
| `WORDS-003` | small inter-glyph gap does NOT split a word | PRD §8.6.2 | green |
| `WORDS-004` | per-word bbox is the union of its char bboxes | PRD §10.7 | green |
| `WORDS-005` | `(block_no, line_no, word_no)` monotonic, word_no resets | PRD §10.7 | green |
| `WORDS-006` | NBSP (`0xA0`) treated as a separator | PRD §8.6.2 | green |

### Span flags (`layout.rs`) — `LAYOUT-FLAGS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-FLAGS-001` | bold font-name heuristic sets bit4 (16) | PRD §8.6.2 | green |
| `LAYOUT-FLAGS-002` | italic/oblique name sets bit1 (2) | PRD §8.6.2 | green |
| `LAYOUT-FLAGS-003` | serif name sets bit2 (4); mono sets bit3 (8) | PRD §8.6.2 | green |
| `LAYOUT-FLAGS-004` | superscript rise sets bit0 (1) | PRD §8.6.2 | green |

### Edge cases (`layout.rs`) — `LAYOUT-EDGE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-EDGE-001` | rotated text grouped along its own axis (`dir`) | PRD §8.6.2 | green |
| `LAYOUT-EDGE-002` | vertical writing → wmode=1, grouped along y | PRD §8.6.2 | green |
| `LAYOUT-EDGE-003` | predominantly-RTL run → visual (right-to-left) order | PRD §8.6.2 | green |
| `LAYOUT-EDGE-004` | empty glyph list → empty TextPage, no panic | PRD §8.6.2 | green |

### Property / containment (`layout_property.rs`) — `LAYOUT-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-PROP-001` | char bbox ⊆ span ⊆ line ⊆ block (containment) | PRD §8.6.2 | green |
| `LAYOUT-PROP-002` | words-concat (space-joined) ≈ text-mode whitespace-normalized | PRD §8.6.2 | green |
| `LAYOUT-PROP-003` | arbitrary glyph list never panics; finite bboxes | PRD §8.1 | green |

### End-to-end (`layout_e2e.rs`) — `LAYOUT-E2E-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LAYOUT-E2E-001` | 2-line/2-word PDF → exact block/line/span/word + text | PRD §8.6 | green |
| `LAYOUT-E2E-002` | `build_textpage` from a real page → device-space structure | PRD §8.6 | green |

## M2d — `get_text` serializers + TEXTFLAGS (`pdf-text`)

Serializes a `&TextPage` into every PyMuPDF `get_text` output (text / blocks /
words / dict / rawdict / json / rawjson / html / xhtml / xml + `get_textbox`)
and pins the per-method `TEXTFLAGS_*` default flag sets (PRD §8.6.2, §10.7).
dict/rawdict/blocks/words/json shapes match PyMuPDF's **documented** shape
(Tier-A, §6.1); html/xhtml/xml are **oxipdf-defined** valid serializations with
their own inline goldens (Tier-B, §6.1). TextPages are built from self-made
glyph lists via `textpage_from_glyphs` (no PyMuPDF files). Tests live in
`crates/pdf-text/tests/serialize_*.rs`.

### TEXTFLAGS values + per-method defaults (`serialize.rs`) — `TEXTFLAGS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `TEXTFLAGS-VALUE-001` | `TEXT_*` bit values match PyMuPDF (1,2,4,8,16,32,64,128) | PRD §8.6.2 | green |
| `TEXTFLAGS-DEFAULT-001` | `text`/`blocks`/`words` default = LIGATURES\|WHITESPACE\|MEDIABOX_CLIP (67) | PRD §8.6.2 | green |
| `TEXTFLAGS-DEFAULT-002` | `dict`/`rawdict`/`json`/`rawjson` default = +PRESERVE_IMAGES (71) | PRD §8.6.2 | green |
| `TEXTFLAGS-DEFAULT-003` | `html`/`xhtml` default = 71 (images on) | PRD §8.6.2 | green |
| `TEXTFLAGS-DEFAULT-004` | `xml` default = LIGATURES\|WHITESPACE\|MEDIABOX_CLIP (67) | PRD §8.6.2 | green |

### Plain text (`serialize.rs`) — `SERIAL-TEXT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SERIAL-TEXT-001` | words on a line joined; line ends with `\n` | PRD §8.6 | green |
| `SERIAL-TEXT-002` | two lines in a block → `\n`-separated, trailing `\n` | PRD §8.6 | green |
| `SERIAL-TEXT-003` | two blocks → separated by a blank line | PRD §8.6 | green |
| `SERIAL-TEXT-004` | empty page → empty string, no panic | PRD §8.6 | green |
| `SERIAL-TEXT-005` | hyphen kept by default (no dehyphenation) | PRD §8.6.2 | green |
| `SERIAL-TEXT-006` | DEHYPHENATE flag joins a line-broken hyphenated word | PRD §8.6.2 | green |
| `SERIAL-TEXT-007` | image block contributes no text | PRD §8.6.2 | green |

### get_textbox clip (`serialize.rs`) — `SERIAL-TEXTBOX-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SERIAL-TEXTBOX-001` | clip rect selects only intersecting lines | PRD §8.6.2 | green |
| `SERIAL-TEXTBOX-002` | clip outside all content → empty string | PRD §8.6.2 | green |

### blocks (`serialize.rs`) — `SERIAL-BLOCKS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SERIAL-BLOCKS-001` | tuple arity 7 `(x0,y0,x1,y1,text,no,type)` | PRD §8.6.2 | green |
| `SERIAL-BLOCKS-002` | text block type=0; block_no monotonic | PRD §8.6.2 | green |
| `SERIAL-BLOCKS-003` | block text is its lines joined by `\n` (trailing `\n`) | PRD §8.6.2 | green |
| `SERIAL-BLOCKS-004` | image block type=1 when PRESERVE_IMAGES on | PRD §8.6.2 | green |
| `SERIAL-BLOCKS-005` | image block omitted when PRESERVE_IMAGES off | PRD §8.6.2 | green |

### words (`serialize.rs`) — `SERIAL-WORDS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SERIAL-WORDS-001` | tuple arity 8 `(x0,y0,x1,y1,word,b,l,w)` | PRD §10.7 | green |
| `SERIAL-WORDS-002` | `(block,line,word)` numbering matches segmenter | PRD §10.7 | green |
| `SERIAL-WORDS-003` | image blocks contribute no words | PRD §8.6.2 | green |

### dict / rawdict tree (`serialize.rs`) — `DICT-*` / `RAWDICT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `DICT-001` | top has width/height/blocks | PRD §10.7 | green |
| `DICT-002` | text block keys type/bbox/number/lines | PRD §10.7 | green |
| `DICT-003` | line keys bbox/wmode/dir/spans | PRD §10.7 | green |
| `DICT-004` | span keys size/flags/font/color/ascender/descender/origin/bbox/text | PRD §10.7 | green |
| `DICT-005` | span color is an int (sRGB) | PRD §10.7 | green |
| `DICT-006` | dict-mode span carries `text`, no `chars` | PRD §10.7 | green |
| `DICT-007` | image block keys (type=1, width/height/ext/colorspace/bpc/transform/size/image) | PRD §10.7 | green |
| `DICT-008` | empty page → blocks empty, width/height set | PRD §10.7 | green |
| `RAWDICT-001` | rawdict span carries `chars`, not `text` | PRD §10.7 | green |
| `RAWDICT-002` | each char has origin/bbox/c | PRD §10.7 | green |
| `RAWDICT-003` | char `c` is a single-scalar string | PRD §10.7 | green |

### json / rawjson (`serialize.rs`) — `JSON-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `JSON-001` | output parses as valid JSON | PRD §8.6.2 | green |
| `JSON-002` | bbox serialized as a 4-array | PRD §8.6.2 | green |
| `JSON-003` | json span has `text`; rawjson span has `chars` | PRD §8.6.2 | green |
| `JSON-004` | image block `image` is a base64 string (placeholder) | PRD §8.6.2 | green |
| `JSON-005` | top width/height/blocks present, deterministic key order | PRD §8.6.2 | green |

### html / xhtml / xml goldens (`serialize_golden.rs`) — `HTML-*` / `XHTML-*` / `XML-*`

oxipdf-defined valid serializations (Tier-B, §6.1); inline goldens human-validated.

| ID | feature | spec ref | status |
|---|---|---|---|
| `HTML-001` | positioned-block html golden (well-formed, oxipdf-defined) | PRD §6.1 | green |
| `XHTML-001` | semantic xhtml golden (well-formed, oxipdf-defined) | PRD §6.1 | green |
| `XML-001` | char-level xml golden (well-formed, oxipdf-defined) | PRD §6.1 | green |
| `XML-002` | xml escapes `<`/`>`/`&`/quotes in char data and attrs | PRD §6.1 | green |

### Properties (`serialize_property.rs`) — `SERIAL-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SERIAL-PROP-001` | words-concat ≈ text (whitespace-normalized) | PRD §8.6.2 | green |
| `SERIAL-PROP-002` | dict block/line/span counts == model | PRD §10.7 | green |
| `SERIAL-PROP-003` | every serializer never panics on arbitrary glyph lists | PRD §8.1 | green |
| `SERIAL-PROP-004` | json always parses for arbitrary pages | PRD §8.6.2 | green |

## M2e — search + inventory + reusable TextPage + PyO3/fitz wiring (M2 exit)

Completes M2 (PRD §8.6, §9.4, §9.5, §12). Adds `search` over a `TextPage`,
`get_fonts`/`get_images` page inventory, a reusable `TextPage` handle, the PyO3
`get_text`/`search_for`/`get_fonts`/`get_images`/`get_textpage` surface (native
Python objects, GIL released around the heavy work), the `fitz`-shim text
methods, and the **M2 accuracy exit gate**. Self-generated fixtures only
(PRD §10). Rust tests live in `crates/pdf-text/tests/search_*.rs` and
`crates/pdf-api/tests/inventory_unit.rs` / `textpage_reuse.rs`; Python tests in
`python/tests/test_text.py`.

### Search over a TextPage (`search.rs`) — `SEARCH-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SEARCH-001` | single hit → one quad overlapping the word | PRD §8.6 | green |
| `SEARCH-002` | multiple hits on a page → one quad each, in reading order | PRD §8.6 | green |
| `SEARCH-003` | case-insensitive by default (`Hello` finds `hello`) | PRD §8.6 | green |
| `SEARCH-004` | Unicode-normalized compare (NFC vs NFD) | PRD §8.6 | green |
| `SEARCH-005` | match across spans within a line → one quad | PRD §8.6 | green |
| `SEARCH-006` | match spanning a line break → one quad per line | PRD §8.6 | green |
| `SEARCH-007` | `hit_max` caps the number of returned hits | PRD §8.6 | green |
| `SEARCH-008` | `clip` rect restricts hits to intersecting geometry | PRD §8.6 | green |
| `SEARCH-009` | not found → empty Vec | PRD §8.6 | green |
| `SEARCH-010` | `quads=false` enclosing `Rect`; `quads=true` `Quad`s | PRD §8.6 | green |
| `SEARCH-011` | empty needle → empty Vec (no panic) | PRD §8.6 | green |

### Page font inventory (`inventory.rs`) — `FONTS-INV-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FONTS-INV-001` | one `/Resources /Font` entry → one 7-tuple | PyMuPDF get_fonts | green |
| `FONTS-INV-002` | tuple = (xref, ext, type, basefont, name, encoding, referencer) | PyMuPDF get_fonts | green |
| `FONTS-INV-003` | subset tag retained in basefont (full name) | PyMuPDF get_fonts | green |
| `FONTS-INV-004` | Type0 reports descendant subtype + encoding | PyMuPDF get_fonts | green |
| `FONTS-INV-005` | no fonts → empty Vec | PyMuPDF get_fonts | green |
| `FONTS-INV-006` | two fonts → two tuples, deduped by xref | PyMuPDF get_fonts | green |

### Page image inventory (`inventory.rs`) — `IMAGES-INV-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `IMAGES-INV-001` | one `/Resources /XObject` image → one 10-tuple | PyMuPDF get_images | green |
| `IMAGES-INV-002` | tuple = (xref, smask, w, h, bpc, cs, alt_cs, name, filter, referencer) | PyMuPDF get_images | green |
| `IMAGES-INV-003` | non-image XObject (Form) excluded | PyMuPDF get_images | green |
| `IMAGES-INV-004` | no images → empty Vec | PyMuPDF get_images | green |
| `IMAGES-INV-005` | smask xref reported when `/SMask` present | PyMuPDF get_images | green |

### Reusable TextPage (`pdf-api`) — `TEXTPAGE-REUSE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `TEXTPAGE-REUSE-001` | `Page::textpage` builds once; reused by get_text + search | PRD §9.4 | green |
| `TEXTPAGE-REUSE-002` | reused TextPage yields identical text to a fresh build | PRD §9.4 | green |
| `TEXTPAGE-REUSE-003` | search over a reused TextPage equals a fresh search | PRD §9.4 | green |

### Python text surface (`test_text.py`) — `PYTEXT-*` / `PYSEARCH-*` / `PYINV-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYTEXT-001` | `get_text("text")` returns known text content | PRD §9.4 | green |
| `PYTEXT-002` | `get_text("words")` arity-8 tuples with content | PRD §9.4 | green |
| `PYTEXT-003` | `get_text("dict")` key set + types (bbox tuple, color int, nested) | PRD §9.4 | green |
| `PYTEXT-004` | `get_text("blocks")` arity-7 tuples | PRD §9.4 | green |
| `PYTEXT-005` | `get_text("json")` parses to the dict structure | PRD §9.4 | green |
| `PYTEXT-006` | `get_text("rawdict")` span carries `chars` | PRD §9.4 | green |
| `PYTEXT-007` | html/xhtml/xml return `str` | PRD §9.4 | green |
| `PYTEXT-008` | `get_textpage()` handle reused via `textpage=` | PRD §9.4 | green |
| `PYTEXT-009` | `sort=True` orders blocks by (y, x) | PRD §9.4 | green |
| `PYSEARCH-001` | `search_for` returns Rect overlapping the known location | PRD §9.4 | green |
| `PYSEARCH-002` | `quads=True` returns `Quad`s | PRD §9.4 | green |
| `PYSEARCH-003` | `hit_max` caps results | PRD §9.4 | green |
| `PYINV-001` | `get_fonts()` returns the expected tuple(s) | PRD §9.4 | green |
| `PYINV-002` | `get_images()` returns the expected tuple(s) | PRD §9.4 | green |
| `PYFITZ-TEXT-001` | `fitz.open(...).load_page(0).get_text("dict")` parity | PRD §9.5 | green |
| `PYFITZ-TEXT-002` | `fitz` search returns fitz `Rect`/`Quad` value types | PRD §9.5 | green |

### M2 accuracy exit gate (`test_text.py`) — `ACCURACY-GT-*`

Normalized-Levenshtein similarity of `get_text("text")` vs known ground truth.

| ID | feature | spec ref | status |
|---|---|---|---|
| `ACCURACY-GT-001` | ASCII multi-line PDF → similarity ≥ 0.98 | PRD §12 (~971) | green |
| `ACCURACY-GT-002` | WinAnsi specials PDF → similarity ≥ 0.98 | PRD §12 (~971) | green |
| `ACCURACY-GT-003` | Type0/Identity-H CID + ToUnicode PDF → similarity ≥ 0.95 | PRD §12 (~971) | green |
