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

## M1+ (placeholder)

Catalogs for `XREF-*`, `OBJSTM-*`, `REPAIR-*`, `CRYPT-*` (M1c–M1f),
`WORDS-*` / text formats (M2), etc. are enumerated at the start of each
milestone per PRD §10.1.1 before implementation begins.
