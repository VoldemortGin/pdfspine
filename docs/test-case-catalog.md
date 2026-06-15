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

### Object-model accessors / `Name` / `StreamData` (`object/`) — `OBJACC-*` / `NAME-*` / `STREAMDATA-*`

Tests live in `crates/pdf-core/tests/objmodel_accessor_unit.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `OBJACC-001` | `is_null` / `as_bool` / `as_i64` Some-and-None arms | ISO 32000-1 §7.3 | green |
| `OBJACC-002` | `as_f64` widens `Integer` and accepts `Real`; `None` for other types | ISO 32000-1 §7.3.3 | green |
| `OBJACC-003` | `as_string` / `as_name` / `as_array` / `as_stream` / `as_reference` Some-and-None arms | ISO 32000-1 §7.3 | green |
| `OBJACC-004` | `as_dict` returns the dict for both `Dictionary` and `Stream`; `None` otherwise | ISO 32000-1 §7.3.8 | green |
| `OBJACC-005` | all `From<…>` impls (bool/i64/i32/f64/PdfString/Name/Vec/Dict/StreamObj/ObjRef) build the right variant | PRD §9.2 | green |
| `OBJACC-006` | `ObjRef::new` field round-trip | ISO 32000-1 §7.3.10 | green |
| `NAME-001` | `new` / `from_decoded` / `as_bytes` round-trip; ordering + equality as `BTreeMap` key | ISO 32000-1 §7.3.5 | green |
| `NAME-002` | `as_str` returns UTF-8; `None` for invalid UTF-8 bytes | ISO 32000-1 §7.3.5 | green |
| `NAME-003` | `is_empty` for `/`; `From<&str>` / `From<String>` | ISO 32000-1 §7.3.5 | green |
| `NAME-004` | `Debug` valid-UTF-8 (`Name(/x)`) and invalid-UTF-8 byte arms | — | green |
| `STREAMDATA-001` | `owned_bytes` Some for Encoded/Decoded, None for Raw | PRD §9.2 | green |
| `STREAMDATA-002` | `bytes` returns for Encoded/Decoded; panics on Raw | PRD §9.2 | green |
| `STREAMDATA-003` | `len` / `is_empty` across Raw / Encoded / Decoded | PRD §9.2 | green |
| `STREAMDATA-004` | `StreamObj::new_encoded` / `raw_bytes` (panics on Raw) | ISO 32000-1 §7.3.8 | green |
| `STREAMDATA-005` | `decode`: Decoded verbatim, Encoded-no-filter, Raw → `Unsupported` | PRD §8.3 | green |
| `STREAMDATA-006` | `decoded` clones an Encoded-no-filter stream to a `Decoded` payload | PRD §9.2 | green |

### `NameInterner` de-dup pool (`interner.rs`) — `INTERN-*`

Tests live in `crates/pdf-core/tests/interner_unit.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `INTERN-001` | `new` / `default` start empty | PRD §9.2 | green |
| `INTERN-002` | first `intern` inserts and returns an equal `Name` | PRD §9.2 | green |
| `INTERN-003` | interning equal bytes twice keeps `len == 1` | PRD §9.2 | green |
| `INTERN-004` | distinct names grow the pool | PRD §9.2 | green |
| `INTERN-005` | `intern_bytes` equivalent to `intern(&Name::from_decoded)` | PRD §9.2 | green |
| `INTERN-006` | second `intern` returns the canonical pooled clone | PRD §9.2 | green |
| `INTERN-007` | empty name is internable | PRD §9.2 | green |
| `INTERN-008` | `Clone` preserves pooled state | PRD §9.2 | green |
| `INTERN-009` | property: `len` equals the number of distinct inputs (never panics) | PRD §9.2 / §10.2 | green |

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

#### Xref-stream `/W` & `/Index` edge cases — `XREFSTM-EDGE-*`

In-module tests in `crates/pdf-core/src/xref/stream.rs`; public-path tests in
`crates/pdf-core/tests/xrefstm_edge_unit.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `XREFSTM-EDGE-001` | `read_index` default `[0 Size]` when `/Index` absent | ISO 32000-1 §7.5.8.2 | green |
| `XREFSTM-EDGE-002` | `read_index` explicit `(start,count)` pairs | ISO 32000-1 §7.5.8.2 | green |
| `XREFSTM-EDGE-003` | `/Index` odd length / non-array / non-u32 start → typed error | ISO 32000-1 §7.5.8.2 | green |
| `XREFSTM-EDGE-004` | `read_w` parses widths; non-int element / missing → `None` | ISO 32000-1 §7.5.8.2 | green |
| `XREFSTM-EDGE-005` | `split_fields` big-endian fields incl. zero-width field reads 0 | ISO 32000-1 §7.5.8.2 | green |
| `XREFSTM-EDGE-006` | missing `/W` / `/Size`, `/W` all-zero → typed error | PRD §8.2 | green |
| `XREFSTM-EDGE-007` | all three entry kinds decode to Free/Uncompressed/Compressed | ISO 32000-1 §7.5.8.3 | green |
| `XREFSTM-EDGE-008` | field-1 width 0 → type defaults to 1; unknown type → Free | ISO 32000-1 §7.5.8.3 | green |
| `XREFSTM-EDGE-009` | data shorter than `/Index` implies → typed error | PRD §8.2 | green |
| `XREFSTM-EDGE-010` | `Raw` (unmaterialized) body → typed error, no panic | PRD §9.2 | green |
| `XREFSTM-EDGE-011` | public chain: non-zero `/Index` start + varied `/W` resolve objects | PRD §8.2 | green |

### Object streams (`objstm.rs`) — `OBJSTM-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `OBJSTM-001` | compressed object resolves identically to an uncompressed one | PRD §8.2 | green |
| `OBJSTM-002` | `/N` / `/First` header pairs parsed; multiple members | PRD §8.2 | green |
| `OBJSTM-003` | second member (index 1) resolves to its object | PRD §8.2 | green |
| `OBJSTM-004` | `/N` exceeding `Limits::max_objstm_objects` → `LimitExceeded` | PRD §9.6.2 | green |
| `OBJSTM-005` | corrupt offset table → typed error, no panic | PRD §8.2 | green |

#### `ObjStm::decode` / `object_at` direct unit edges — `OBJSTM-EDGE-*`

In-module tests in `crates/pdf-core/src/objstm.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `OBJSTM-EDGE-001` | happy 2-member decode: `len`/`object_number_at`/`member_nums`/`object_at` | PRD §8.2 | green |
| `OBJSTM-EDGE-002` | `/N 0` empty stream → `is_empty` | PRD §8.2 | green |
| `OBJSTM-EDGE-003` | missing `/N` / missing `/First` → typed error | PRD §8.2 | green |
| `OBJSTM-EDGE-004` | `/N` over limit → `LimitExceeded(ObjstmObjects)` | PRD §9.6.2 | green |
| `OBJSTM-EDGE-005` | `Raw` (unmaterialized) body → typed error | PRD §9.2 | green |
| `OBJSTM-EDGE-006` | header too few pairs for `/N` → typed error | PRD §8.2 | green |
| `OBJSTM-EDGE-007` | `object_at` out-of-range index → typed error | PRD §8.2 | green |
| `OBJSTM-EDGE-008` | `object_at` offset past body → typed error | PRD §8.2 | green |
| `OBJSTM-EDGE-009` | malformed object body → typed error, no panic | PRD §8.2 | green |

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

### `CryptoError` Display + `kind()` discriminants — `CRYPTO-ERR-*`

Tests live in `crates/pdf-crypto/tests/error_unit.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPTO-ERR-001` | `Malformed` Display = `malformed /Encrypt: …` | PRD §8.4 | green |
| `CRYPTO-ERR-002` | `Unsupported` Display = `unsupported security handler: …` | PRD §8.4 | green |
| `CRYPTO-ERR-003` | `NeedsPassword` Display = `password required or incorrect` | PRD §8.4 | green |
| `CRYPTO-ERR-004` | `DecryptFailed` Display = `decrypt failed: …` | PRD §8.4 | green |
| `CRYPTO-ERR-005` | `kind()` returns stable discriminant for all four variants | PRD §8.4 / §9.3 | green |
| `CRYPTO-ERR-006` | `std::error::Error` trait-object Display path | PRD §8.4 | green |
| `CRYPTO-ERR-007` | `Clone` + `PartialEq` (equal clone; distinct variants `!=`) | PRD §8.4 | green |
| `CRYPTO-ERR-008` | `Debug` carries the variant name | PRD §8.4 | green |

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

### Python wheel (`oxide_pdf` / `fitz`) — `PYDOC-*` / `PYFITZ-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYDOC-001` | `oxide_pdf.open(path)`: `page_count`/`len`/index/`load_page` | PRD §9.4 | green |
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
(Tier-A, §6.1); html/xhtml/xml are **oxide-pdf-defined** valid serializations with
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

oxide-pdf-defined valid serializations (Tier-B, §6.1); inline goldens human-validated.

| ID | feature | spec ref | status |
|---|---|---|---|
| `HTML-001` | positioned-block html golden (well-formed, oxide-pdf-defined) | PRD §6.1 | green |
| `XHTML-001` | semantic xhtml golden (well-formed, oxide-pdf-defined) | PRD §6.1 | green |
| `XML-001` | char-level xml golden (well-formed, oxide-pdf-defined) | PRD §6.1 | green |
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

---

## M3a — PDF writer / full save + object-edit API (`pdf-core::changeset`, `pdf-core::writer`)

Spec source of truth: PRD §8.7 (object-edit API + full save), §9.2 (`ChangeSet`
on `DocumentStore`), §9.3 (typed errors). The primary correctness oracle is our
own reparse (open → edit → save → reopen → assert); an optional `qpdf --check`
runs only when `qpdf` is on `PATH`. Tests live in
`crates/pdf-core/tests/changeset_unit.rs`, `crates/pdf-core/tests/writer_unit.rs`
and `crates/pdf-core/tests/save_e2e.rs`.

### ChangeSet object-edit API (`changeset.rs`) — `EDIT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `EDIT-001` | `add_object` allocates a fresh number past current max; `is_dirty` flips | PRD §8.7/§9.2 | green |
| `EDIT-002` | `add_object` then `resolve` returns the new value (no save) | PRD §8.7 | green |
| `EDIT-003` | `add_object` then save → reopen → object present + equal | PRD §8.7 | green |
| `EDIT-004` | `update_object` reflected by an immediate `resolve` | PRD §8.7 | green |
| `EDIT-005` | `update_object` reflected after save → reopen | PRD §8.7 | green |
| `EDIT-006` | `update_stream` (deflate off) body round-trips after save → reopen | PRD §8.7 | green |
| `EDIT-007` | `update_stream` (deflate on) body decodes to original after reopen | PRD §8.7 | green |
| `EDIT-008` | `delete_object` → `resolve` yields Null; gone after save → reopen | PRD §8.7 | green |
| `EDIT-009` | edit on an unmodified doc: `is_dirty` false, `changes` empty | PRD §9.2 | green |
| `EDIT-010` | `update_object` on a never-resolved original num overlays correctly | PRD §8.7 | green |
| `EDIT-011` | add/update/delete reflected in `changes()` list (M3b basis) | PRD §9.2 | green |

### Full save / Writer (`writer.rs`) — `SAVE-FULL-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SAVE-FULL-001` | `save_to_vec` of a simple doc → reopen → equal `page_count` | PRD §8.7 | green |
| `SAVE-FULL-002` | every original in-use object survives save → reopen (value-equal) | PRD §8.7 | green |
| `SAVE-FULL-003` | extracted text equal across save → reopen | PRD §8.7 | green |
| `SAVE-FULL-004` | output begins with `%PDF-` + binary-comment line | PRD §8.7 | green |
| `SAVE-FULL-005` | trailer `/Root` preserved; `/Size` == max obj num + 1 | PRD §8.7 | green |
| `SAVE-FULL-006` | `/ID` present, 2 elements (both 16-byte hex strings) | PRD §8.7 | green |
| `SAVE-FULL-007` | `/Info` ref carried over when present | PRD §8.7 | green |
| `SAVE-FULL-008` | minimal/empty-page doc saves and reopens | PRD §8.7 | green |
| `SAVE-FULL-009` | save → reopen → save again: identical live-object set (idempotent) | PRD §8.7 | green |
| `SAVE-FULL-010` | `xref_style=Table` output ends with `startxref`/`%%EOF` | PRD §8.7 | green |

### Stream deflate policy (`writer.rs`) — `SAVE-STREAM-DEFLATE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SAVE-STREAM-DEFLATE-001` | a plain stream saved with `deflate=true` carries `/FlateDecode` | PRD §8.7 | green |
| `SAVE-STREAM-DEFLATE-002` | deflated stream reopens + decodes to the original bytes | PRD §8.7 | green |
| `SAVE-STREAM-DEFLATE-003` | already-`/FlateDecode` stream not double-deflated | PRD §8.7 | green |
| `SAVE-STREAM-DEFLATE-004` | image-filtered stream (`/DCTDecode`) left untouched | PRD §8.7 | green |
| `SAVE-STREAM-DEFLATE-005` | `deflate=false` keeps bodies as-is; `/Length` recomputed | PRD §8.7 | green |

### Xref-style output (`writer.rs`) — `SAVE-XREF-*` / `SAVE-XREFSTREAM-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SAVE-XREF-001` | classic table has object-0 free head (gen 65535 f) | PRD §8.7 | green |
| `SAVE-XREF-002` | classic-table output reopens; objects intact | PRD §8.7 | green |
| `SAVE-XREFSTREAM-001` | xref-stream output has `/Type /XRef`, `/W`, `/Size` | PRD §8.7 | green |
| `SAVE-XREFSTREAM-002` | xref-stream output is parseable by the M1c xref-stream reader | PRD §8.7 | green |
| `SAVE-XREFSTREAM-003` | xref-stream output reopens via `DocumentStore`; objects intact | PRD §8.7 | green |

### Save robustness / determinism (`writer.rs`) — `SAVE-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SAVE-PROP-001` | `Table, deflate=false` is deterministic for same input+options | PRD §8.7 | green |
| `SAVE-PROP-002` | save never panics on a freshly-opened simple doc (both styles) | PRD §8.7 | green |
| `SAVE-PROP-003` | first `/ID` element stable per doc; second varies per save | PRD §8.7 | green |
| `SAVE-PROP-004` | optional `qpdf --check` passes on a saved file (skipped if absent) | PRD §8.7 | green |

## M3b — Incremental save + garbage collection (`pdf-core::writer`, `pdf-core::gc`)

Spec source of truth: PRD §8.7 (incremental save, clean-parse precondition),
§8.7.1 (GC level-3 dedup exclusion list + COW-unshare), §12 M3 exit gate. The
primary correctness oracle is our own reparse (open → edit → save_incremental /
save(garbage=N) → reopen → assert) plus a byte-exactness assertion
`out[..orig.len()] == orig`; an optional `qpdf --check` runs only when `qpdf` is
on `PATH`. Tests live in `crates/pdf-core/tests/incremental_e2e.rs` and
`crates/pdf-core/tests/gc_e2e.rs`.

### Incremental save — byte exactness (`writer.rs`) — `INCR-BYTES-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INCR-BYTES-001` | after `update_object`, `out[..orig.len()] == orig` (prefix byte-exact) | PRD §8.7 | green |
| `INCR-BYTES-002` | after `add_object`, `out[..orig.len()] == orig`; new obj appended | PRD §8.7 | green |
| `INCR-BYTES-003` | after `delete_object`, `out[..orig.len()] == orig`; deleted obj freed | PRD §8.7 | green |
| `INCR-BYTES-004` | no-op (no edits) incremental save still byte-exact-prefixes the original | PRD §8.7 | green |
| `INCR-BYTES-005` | a single small edit appends little (`out.len() - orig.len()` bounded) | PRD §8.7 | green |
| `INCR-BYTES-006` | xref-stream style: `out[..orig.len()] == orig` holds too | PRD §8.7 | green |

### Incremental save — `/Prev` chain + multi-revision (`writer.rs`) — `INCR-PREV-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INCR-PREV-001` | new section `/Prev` == prior `startxref` (table style) | PRD §8.7 | green |
| `INCR-PREV-002` | both revisions reopen; updated object resolves to the NEW value | PRD §8.7 | green |
| `INCR-PREV-003` | the new `startxref` points at the appended xref section | PRD §8.7 | green |
| `INCR-PREV-004` | new trailer carries `/Root`, `/Size` = max+1, two-element `/ID` | PRD §8.7 | green |
| `INCR-PREV-005` | xref-stream style: `/Prev` == prior `startxref`; reopens to new value | PRD §8.7 | green |
| `INCR-PREV-006` | added object resolves after reopen; new number continues from max | PRD §8.7 | green |
| `INCR-PREV-007` | two successive incremental saves chain `/Prev` correctly; reopen final | PRD §8.7 | green |

### Incremental save — clean-parse precondition (`writer.rs`) — `INCR-CLEAN-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INCR-CLEAN-001` | clean parse → `can_save_incrementally()` true; succeeds | PRD §8.7 | green |
| `INCR-CLEAN-002` | repaired doc → `can_save_incrementally()` false | PRD §8.7 | green |
| `INCR-CLEAN-003` | repaired doc + `on_repaired: Reject` → typed `IncrementalRequiresCleanParse` | PRD §8.7 | green |
| `INCR-CLEAN-004` | repaired doc + `on_repaired: Upgrade` → full save fallback (reopens) | PRD §8.7 | green |

### Incremental save — signature preservation (`writer.rs`) — `INCR-SIG-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INCR-SIG-001` | clean signed-marker doc edited incrementally: signed byte range bytes unchanged | PRD §8.7 | green |
| `INCR-SIG-002` | the `/ByteRange`-covered prefix is identical pre/post incremental edit | PRD §8.7 | green |

### GC level 1 — mark & sweep (`gc.rs`) — `GC-1-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GC-1-001` | unreachable orphan object dropped; object count falls | PRD §8.7 | green |
| `GC-1-002` | reachable set preserved; page_count + extracted text unchanged after reopen | PRD §8.7 | green |
| `GC-1-003` | `/Info` / `/ID` trailer roots kept reachable | PRD §8.7 | green |

### GC level 2 — compact / renumber (`gc.rs`) — `GC-2-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GC-2-001` | object numbers densified (no gaps) after dropping orphans | PRD §8.7 | green |
| `GC-2-002` | all refs remapped consistently; reopen → text + page_count intact | PRD §8.7 | green |
| `GC-2-003` | `/Size` == survivor count + 1 (dense) | PRD §8.7 | green |

### GC level 3 — dedup identical objects + exclusion (`gc.rs`) — `GC-3-*` / `GC3-EXCLUDE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GC-3-001` | two identical non-excluded dicts merge to one; count falls vs level 2 | PRD §8.7.1 | green |
| `GC-3-002` | reachability + text preserved after dedup → reopen | PRD §8.7.1 | green |
| `GC3-EXCLUDE-001` | two identical-content `/Type /Page` objects are NOT merged | PRD §8.7.1 | green |
| `GC3-EXCLUDE-002` | `/Type /Pages` and the Catalog are NOT merged | PRD §8.7.1 | green |

### GC level 4 — dedup identical streams (`gc.rs`) — `GC-4-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GC-4-001` | two identical streams (dict+body) merge to one; count falls vs level 3 | PRD §8.7 | green |
| `GC-4-002` | reachability + decoded stream bytes preserved after reopen | PRD §8.7 | green |

### GC COW-unshare after merge (`gc.rs`) — `GC3-COW-*` / `GC4-COW-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GC3-COW-001` | save(garbage=3) leaves live model unmerged; `update_object` to one user doesn't affect other | PRD §8.7.1 | green |
| `GC3-COW-002` | after such an edit, reopen confirms the two users are independent | PRD §8.7.1 | green |
| `GC4-COW-001` | save(garbage=4) is save-time only; `update_stream` to one user doesn't affect other | PRD §8.7.1 | green |

### GC properties (`gc.rs`) — `GC-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `GC-PROP-001` | GC never drops a reachable object (all roots survive every level) | PRD §8.7 | green |
| `GC-PROP-002` | GC never panics across levels 0..=4 on a simple doc | PRD §8.7 | green |
| `GC-PROP-003` | garbage=0 is identity (no objects dropped vs plain full save) | PRD §8.7 | green |

## M3c — Page operations + `insert_pdf` merge (`pdf-edit`)

Spec source: PRD §8.7 "Page ops" + `insert_pdf` (lines ~543–567), §12 M3 exit
(merge order/count/refs correct, shared font deduped single, saved fixtures
reparse clean). Tests live in `crates/pdf-edit/tests/`. The page tree is
**normalized to a single-level flat `/Kids` list under the root `/Pages`** on
first edit (PRD §8.7: flatten is the v1 default, round-trip-safe because
inherited attributes are materialized onto leaves). `/Pages /Count` and every
kid's `/Parent` are kept consistent at every step; the live page list is
re-read from the document on each query.

### Page ops — new / insert / delete (`page_ops.rs`) — `PAGEOPS-NEW-*` / `PAGEOPS-INSERT-*` / `PAGEOPS-DELETE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGEOPS-NEW-001` | `new_page(index, w, h)` → page_count += 1; new leaf has MediaBox [0 0 w h] + empty Contents | PRD §8.7 | green |
| `PAGEOPS-NEW-002` | new page inserted at `index`; surrounding page order preserved | PRD §8.7 | green |
| `PAGEOPS-NEW-003` | after save→reopen, `/Pages /Count` == new count; new page's MediaBox intact | PRD §8.7 | green |
| `PAGEOPS-NEW-004` | `new_page` at end (index == count) appends | PRD §8.7 | green |
| `PAGEOPS-INSERT-001` | `insert_page(index, leaf_ref)` splices an existing leaf; count += 1, `/Parent` repointed | PRD §8.7 | green |
| `PAGEOPS-INSERT-002` | inserted page appears at `index` by identifiable content after reopen | PRD §8.7 | green |
| `PAGEOPS-DELETE-001` | `delete_page(index)` → count -= 1; the right page removed (by content) | PRD §8.7 | green |
| `PAGEOPS-DELETE-002` | delete first / last / middle each yield correct remaining order | PRD §8.7 | green |
| `PAGEOPS-DELETE-003` | after save→reopen, `/Count` consistent and removed content absent | PRD §8.7 | green |
| `PAGEOPS-DELETE-004` | delete out-of-range index → typed error, no mutation | PRD §8.7 | green |

### Page ops — copy / move (`page_ops.rs`) — `PAGEOPS-COPY-*` / `PAGEOPS-MOVE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGEOPS-COPY-001` | `copy_page(from, to)` → count += 1; copy shares the source leaf ref (count occurrences) | PRD §8.7 | green |
| `PAGEOPS-COPY-002` | copied page content equals source content after reopen | PRD §8.7 | green |
| `PAGEOPS-MOVE-001` | `move_page(from, to)` keeps count; page order reflects the move (by content) | PRD §8.7 | green |
| `PAGEOPS-MOVE-002` | move is a no-op when from == to | PRD §8.7 | green |
| `PAGEOPS-MOVE-003` | move backward and forward both correct after reopen | PRD §8.7 | green |

### Page ops — select / subset+reorder (`page_ops.rs`) — `PAGEOPS-SELECT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGEOPS-SELECT-001` | `select([2,0])` yields exactly those pages in that order; count == 2 | PRD §8.7 | green |
| `PAGEOPS-SELECT-002` | duplicate indices in `select` duplicate the page | PRD §8.7 | green |
| `PAGEOPS-SELECT-003` | empty `select([])` yields a zero-page document; `/Count` == 0 | PRD §8.7 | green |
| `PAGEOPS-SELECT-004` | select identity (`[0,1,2]`) preserves order + content after reopen | PRD §8.7 | green |
| `PAGEOPS-SELECT-005` | out-of-range index in `select` → typed error | PRD §8.7 | green |

### Page ops — box / rotation setters (`page_ops.rs`) — `PAGEOPS-BOX-*` / `PAGEOPS-ROTATE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGEOPS-BOX-001` | `set_mediabox(rect)` reflected on `Page::mediabox()` after reopen | PRD §8.7 | green |
| `PAGEOPS-BOX-002` | `set_cropbox(rect)` clipped to mediabox; reflected after reopen | PRD §8.7 | green |
| `PAGEOPS-ROTATE-001` | `set_rotation(90)` reflected on `Page::rotation()` after reopen | PRD §8.7 | green |
| `PAGEOPS-ROTATE-002` | rotation normalized to {0,90,180,270} (e.g. 450 → 90, -90 → 270) | PRD §8.7 | green |

### Page ops — flatten / consistency (`page_ops.rs`) — `PAGEOPS-FLATTEN-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGEOPS-FLATTEN-001` | a nested page tree normalizes to a flat `/Kids` under root on first edit; order preserved | PRD §8.7 | green |
| `PAGEOPS-FLATTEN-002` | inherited MediaBox/Rotate materialized onto leaves after flatten | PRD §8.7 | green |
| `PAGEOPS-FLATTEN-003` | every leaf's `/Parent` points at the root `/Pages` after flatten | PRD §8.7 | green |

### insert_pdf — count / range / position (`merge.rs`) — `MERGE-COUNT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `MERGE-COUNT-001` | `insert_pdf(src)` appends all src pages → dst count += src count | PRD §8.7 | green |
| `MERGE-COUNT-002` | `from_page`/`to_page` subset inserts only the selected range | PRD §8.7 | green |
| `MERGE-COUNT-003` | `start_at` splices the copied pages at that position | PRD §8.7 | green |
| `MERGE-COUNT-004` | after save→reopen, `/Pages /Count` == merged count | PRD §8.7 | green |
| `MERGE-COUNT-005` | reversed range (`from > to`) inserts pages in reverse order | PRD §8.7 | green |

### insert_pdf — order (`merge.rs`) — `MERGE-ORDER-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `MERGE-ORDER-001` | appended pages' text appears after dst text, in src order, after reopen | PRD §12 | green |
| `MERGE-ORDER-002` | `start_at=0` prepends; interleaved order correct by content | PRD §12 | green |
| `MERGE-ORDER-003` | a page-range subset preserves intra-range order | PRD §12 | green |

### insert_pdf — refs / extractability (`merge.rs`) — `MERGE-REFS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `MERGE-REFS-001` | all references in copied pages resolve in dst (no dangling) | PRD §12 | green |
| `MERGE-REFS-002` | copied objects get fresh numbers, no collision with dst objects | PRD §12 | green |
| `MERGE-REFS-003` | `get_text` on a merged page returns the source page's text | PRD §12 | green |
| `MERGE-REFS-004` | saved merged doc reparses clean (reopen + optional `qpdf --check`) | PRD §12 | green |

### insert_pdf — shared-object dedup (`merge.rs`) — `MERGE-DEDUP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `MERGE-DEDUP-001` | a font shared by two src pages is copied **once**; both copies reference it | PRD §12 | green |
| `MERGE-DEDUP-002` | a shared XObject is copied once (count copies in dst) | PRD §12 | green |
| `MERGE-DEDUP-003` | a cyclic ref graph in src is copied without infinite loop | PRD §8.7 | green |

### insert_pdf — inherited attrs / rotate / robustness (`merge.rs`) — `MERGE-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `MERGE-PROP-001` | inherited MediaBox on src pages is materialized onto copied leaves | PRD §8.7 | green |
| `MERGE-PROP-002` | `rotate` option applied to inserted pages (reflected after reopen) | PRD §8.7 | green |
| `MERGE-PROP-003` | self-insert (src structurally == dst) never panics; count doubles | PRD §8.7 | green |
| `MERGE-PROP-004` | inserting from an empty range is a no-op (count unchanged) | PRD §8.7 | green |

### split / extract (`merge.rs`) — `SPLIT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SPLIT-001` | `extract_pages([1])` → new 1-page doc bytes; reopens; text == source page 1 | PRD §8.7 | green |
| `SPLIT-002` | `extract_pages([0,2])` → 2-page doc in that order | PRD §8.7 | green |
| `SPLIT-003` | extracted doc has its own self-contained object graph (no dangling refs) | PRD §8.7 | green |

## M3d — Metadata / TOC / links / PageLabels + encryption-write + PyO3/fitz wiring (`pdf-edit`, `pdf-crypto`, `pdf-core`, `py-bindings`, `python/`)

Spec source: PRD §8.9 (metadata/TOC/links ~592-595), §8.4 (encryption write rules
~450-476), §8.7 (~543-567), §12 M3 exit (~973): TOC round-trip == input,
level-jump rejected, named dest resolves under `/PageLabels`, saved fixtures
reparse clean, encrypted round-trip. Rust tests live in
`crates/pdf-edit/tests/{metadata_e2e.rs,toc_e2e.rs,links_e2e.rs,pagelabel_e2e.rs,nameddest_e2e.rs}`
and `crates/pdf-crypto/tests/authoring_unit.rs` +
`crates/pdf-core/tests/crypt_write_e2e.rs`; Python in `python/tests/`.

The catalog/`/Info` mutation gap (writer carries `/Info`/`/Encrypt` only as
pre-existing trailer refs) is bridged by `DocumentStore::set_trailer_ref(key,
ref)` (interior-mutable trailer-key overlay consulted by the writer). The
catalog is always a GC root, so `/Outlines`, `/Names`, `/PageLabels` survive a
full save by mutating the catalog dict via `update_object(root, …)`.

### Metadata — `/Info` write + XMP (`metadata.rs`) — `META-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `META-INFO-001` | `set_metadata` on a doc with no `/Info` creates one; save→reopen reads back title/author | PRD §8.9 | green |
| `META-INFO-002` | `set_metadata` on a doc with an existing indirect `/Info` updates it; save→reopen reflects new values | PRD §8.9 | green |
| `META-INFO-003` | all keys round-trip (title/author/subject/keywords/creator/producer/creationDate/modDate) | PRD §8.9 | green |
| `META-INFO-004` | clearing a key (empty/None) removes it from `/Info`; absent on reopen | PRD §8.9 | green |
| `META-INFO-005` | non-ASCII title written as UTF-16BE (`FE FF` BOM), read back equal | PRD §8.9 | green |
| `META-INFO-006` | PDF date string `D:YYYYMMDDHHmmSS` written verbatim, read back verbatim | PRD §8.9 | green |
| `META-INFO-007` | reading via `pdf-api` `Metadata` stays consistent with what was written (M1f read path) | PRD §8.9 | green |
| `META-XMP-001` | `set_xml_metadata` creates a `/Metadata` XMP stream in the catalog; `get_xml_metadata` reads it back | PRD §8.9 | green |
| `META-XMP-002` | `set_xml_metadata` replaces an existing `/Metadata` stream; reopen reads the new XMP | PRD §8.9 | green |
| `META-XMP-003` | `get_xml_metadata` on a doc with no `/Metadata` returns empty/None | PRD §8.9 | green |

### TOC / outlines — get / set / level-jump (`toc.rs`) — `TOC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `TOC-GET-001` | `get_toc` on a hand-built `/Outlines` returns `(level,title,page)` rows in document order | PRD §8.9 | green |
| `TOC-GET-002` | nested First/Next/Parent chain produces correct levels (1,2,2,1) | PRD §8.9 | green |
| `TOC-GET-003` | page computed from `/Dest [pageref /XYZ …]` and from `/A << /S /GoTo /D … >>` | PRD §8.9 | green |
| `TOC-GET-004` | empty / absent `/Outlines` → empty list | PRD §8.9 | green |
| `TOC-SET-001` | `set_toc` then `get_toc` == input (flat 1-level list) | PRD §12 | green |
| `TOC-SET-002` | nested levels (1,2,3,2,1) round-trip == input | PRD §12 | green |
| `TOC-SET-003` | built `/Outlines` has correct `/Count` (signed), `/First`/`/Last`, sibling `/Next`/`/Prev`, child `/Parent` | PRD §8.9 | green |
| `TOC-SET-004` | each entry's `/Dest` resolves to the right physical page after reopen | PRD §8.9 | green |
| `TOC-SET-005` | `set_toc([])` removes `/Outlines`; `get_toc` → empty | PRD §8.9 | green |
| `TOC-SET-006` | save→reopen→`get_toc` still equals input (persisted tree) | PRD §12 | green |
| `TOC-JUMP-001` | a level jump (1→3) is rejected with a typed error; document unmutated | PRD §12 | green |
| `TOC-JUMP-002` | first entry with level != 1 is rejected | PRD §8.9 | green |

### Page labels — number-tree read + `get_label` (`pagelabel.rs`) — `PAGELABEL-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PAGELABEL-001` | decimal style `D` with prefix+start → labels per physical page (e.g. `A-1`,`A-2`) | PRD §8.9 §3.5 | green |
| `PAGELABEL-002` | lowercase-roman `r` style → `i`,`ii`,`iii` | PRD §8.9 | green |
| `PAGELABEL-003` | uppercase-roman `R` and lowercase/uppercase-alpha `a`/`A` styles | PRD §8.9 | green |
| `PAGELABEL-004` | multiple ranges in the `/Nums` tree apply to the right page spans | PRD §8.9 | green |
| `PAGELABEL-005` | no `/PageLabels` → `get_label` returns the empty string (PyMuPDF behavior) | PRD §8.9 | green |
| `PAGELABEL-006` | `/St` start value honored (range starting at 5 → `5`,`6`,…) | PRD §8.9 | green |

### Named destinations → physical page (`dest.rs`) — `NAMEDDEST-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `NAMEDDEST-001` | resolve a name in catalog `/Dests` dict → correct physical page index | PRD §8.9 | green |
| `NAMEDDEST-002` | resolve a name in the `/Names /Dests` name-tree → correct page | PRD §8.9 | green |
| `NAMEDDEST-003` | name-tree with `/Kids` (intermediate nodes + `/Limits`) traverses to the leaf | PRD §8.9 | green |
| `NAMEDDEST-004` | a named dest still resolves to the correct **physical** page under a non-trivial `/PageLabels` | PRD §12 | green |
| `NAMEDDEST-005` | unknown name → `None` (no panic) | PRD §8.9 | green |
| `NAMEDDEST-006` | `resolve_link` on a `/GoTo` action with a named `/D` resolves to a page | PRD §8.9 | green |

### Links — read / insert / update / delete (`links.rs`) — `LINK-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `LINK-GET-001` | `get_links` reads a `/Annots` Link with `/A /URI` → `{kind:uri, from:Rect, uri}` | PRD §8.9 | green |
| `LINK-GET-002` | `get_links` reads a GoTo link (`/Dest` or `/A /GoTo`) → `{kind:goto, from:Rect, page}` | PRD §8.9 | green |
| `LINK-GET-003` | a page with no `/Annots` → empty list | PRD §8.9 | green |
| `LINK-GET-004` | named-dest GoTo link resolves to a page index | PRD §8.9 | green |
| `LINK-INSERT-001` | `insert_link` (uri) adds a Link annot; reopen→`get_links` shows it with the rect+uri | PRD §8.9 | green |
| `LINK-INSERT-002` | `insert_link` (goto) adds a GoTo Link; reopen→page target correct | PRD §8.9 | green |
| `LINK-INSERT-003` | inserting on a page with no `/Annots` creates the array | PRD §8.9 | green |
| `LINK-UPDATE-001` | `update_link` changes the rect / uri of an existing link; reopen reflects it | PRD §8.9 | green |
| `LINK-DELETE-001` | `delete_link` removes the annot; reopen→`get_links` count decremented | PRD §8.9 | green |

### Encryption authoring — `pdf-crypto` public API (`authoring.rs`) — `CRYPT-AUTH-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-AUTH-001` | `author_rc4_128` builds an `/Encrypt` config a `Decryptor` authenticates with `""` | PRD §8.4 | green |
| `CRYPT-AUTH-002` | `author_aes128` (AESV2) config authenticates with `""`; per-object enc/dec round-trips | PRD §8.4 | green |
| `CRYPT-AUTH-003` | `author_aes256_r6` config authenticates with `""`; R6 file key used directly (no salt) | PRD §8.4 | green |
| `CRYPT-AUTH-004` | owner-only password: `author_*` with owner pw, empty user pw authenticates as Owner | PRD §8.4 | green |
| `CRYPT-AUTH-005` | wrong password → `NeedsPassword` | PRD §8.4 | green |
| `CRYPT-AUTH-006` | salts/IVs come from a real RNG (two authorings of the same doc differ in `/U` salt) | PRD §8.4 | green |
| `CRYPT-AUTH-007` | `author_aes256_r6` sets `/R 6` and `/V 5`; **never emits R5** | PRD §8.4 | green |
| `CRYPT-AUTH-008` | `EncryptSpec::method` maps RC4_128/AES_128/AES_256_R6 → correct `(v,r,cfm)` | PRD §8.4 | green |

### Encryption on save — writer integration (`crypt_write_e2e.rs`) — `CRYPT-WRITE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CRYPT-WRITE-RC4` | save encrypted (RC4-128) → reopen + `authenticate("")` → page text equals plaintext | PRD §8.4 | green |
| `CRYPT-WRITE-AES128` | save encrypted (AES-128) → reopen + auth → text equals; AES IV prefix present | PRD §8.4 | green |
| `CRYPT-WRITE-AES256` | save encrypted (AES-256 R6) → reopen + auth → text equals | PRD §8.4 | green |
| `CRYPT-WRITE-STR` | `/Info /Title` string is encrypted on disk (ciphertext != plaintext), decrypts on reopen | PRD §8.4 | green |
| `CRYPT-WRITE-OWNER` | owner-only password save → wrong user pw fails, owner pw authenticates | PRD §8.4 | green |
| `CRYPT-WRITE-WRONGPW` | reopen + `authenticate("wrong")` → false; `authenticate("")` succeeds | PRD §8.4 | green |
| `CRYPT-WRITE-EXEMPT-ID` | the trailer `/ID` strings are NOT encrypted (readable as plain hex) | PRD §8.4 | green |
| `CRYPT-WRITE-EXEMPT-ENC` | the `/Encrypt` dict's own strings (`/O`/`/U`) are NOT encrypted | PRD §8.4 | green |
| `CRYPT-WRITE-EXEMPT-XREF` | when xref-stream style is used, the xref stream body is NOT encrypted (reparses) | PRD §8.4 | green |
| `CRYPT-WRITE-NEVER-R5` | the authored `/Encrypt` for AES-256 has `/R 6` (assert never 5) | PRD §8.4 | green |
| `CRYPT-WRITE-QPDF` | (optional) `qpdf --decrypt` on the saved file succeeds when `qpdf` present | PRD §12 | green |

### PyO3 / fitz wiring — save + edit surface (`python/tests/`) — `PYSAVE-*` / `PYMETA-*` / `PYTOC-*` / `PYMERGE-*` / `PYLINK-*` / `PYENC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYSAVE-001` | `Document.save(path)` then `oxide_pdf.open(path)` reopens with same page_count + text | PRD §8.9 | green |
| `PYSAVE-002` | `Document.tobytes()` → `open(stream=…)` round-trips | PRD §8.9 | green |
| `PYSAVE-003` | `Document.save(incremental=True)` / `saveIncr()` appends; both revisions reopen | PRD §8.9 | green |
| `PYSAVE-004` | `garbage`/`deflate` kwargs accepted; saved file reparses | PRD §8.9 | green |
| `PYMETA-001` | `set_metadata({...})` → `metadata` round-trips after reopen | PRD §8.9 | green |
| `PYMETA-002` | `setMetadata` deprecated alias works (via shim) | PRD §8.9 | green |
| `PYMETA-003` | `get_xml_metadata`/`set_xml_metadata` round-trip | PRD §8.9 | green |
| `PYTOC-001` | `set_toc(list)` then `get_toc()` == input (nested) | PRD §12 | green |
| `PYTOC-002` | `getToC`/`setToC` deprecated aliases work | PRD §8.9 | green |
| `PYTOC-003` | level-jump in `set_toc` raises `PdfError` (mapped) | PRD §12 | green |
| `PYMERGE-001` | `insert_pdf(src)` merges; page_count grows; text from both present | PRD §8.9 | green |
| `PYMERGE-002` | `insertPDF` deprecated alias works | PRD §8.9 | green |
| `PYEDIT-001` | a page op (`delete_page`/`select`) reflected on reopen | PRD §8.9 | green |
| `PYEDIT-002` | `new_page`/`newPage` adds a page; reopen count grows | PRD §8.9 | green |
| `PYLINK-001` | `Page.get_links()` returns links with `fitz.Rect` `from` + kind/uri/page | PRD §8.9 | green |
| `PYLABEL-001` | `Page.get_label()` returns the page label under a `/PageLabels` doc | PRD §8.9 | green |
| `PYENC-001` | `save(encryption=AES_256, user_pw="")` → reopen → `is_encrypted`, `authenticate("")`, text equals | PRD §8.4 | green |
| `PYENC-002` | encrypted save with owner pw: wrong user pw → `authenticate` false | PRD §8.4 | green |

---

## M4a — Content insertion (text / image / vector drawing) + font embedding (`pdf-edit`)

Spec source of truth: PRD §8.8 (content emission), §8.5 / §8.5.2 (font embedding,
full-embed fallback), §7 (insert_text/insert_textbox/insert_image/draw_*/Shape).
All content appends to a page's `/Contents` (the existing content is wrapped in a
`q … Q` balanced pair, a new content stream is appended, resources are merged
into `/Resources`). The strongest correctness oracle is **round-trip through the
M2 pipeline**: insert → full save → reopen → `pdf_text::interpret_page` /
`search`. Tests live in `crates/pdf-edit/tests/insert_text_e2e.rs`,
`insert_image_e2e.rs`, `draw_e2e.rs`, plus `pdf-fonts` width-table unit tests.

### Core-14 standard widths (`pdf-fonts::widths`) — `WIDTHS-STD14-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `WIDTHS-STD14-001` | `helvetica` widths: space=278, `A`=667, `i`=222 (factual AFM metrics) | PRD §8.5 | green |
| `WIDTHS-STD14-002` | `times-roman` widths: space=250, `A`=722, `.`=250 | PRD §8.5 | green |
| `WIDTHS-STD14-003` | Courier (mono) all glyphs = 600 | PRD §8.5 | green |
| `WIDTHS-STD14-004` | `string_width("Hello", helv, 12)` sums per-glyph advances scaled by size/1000 | PRD §8.5 | green |
| `WIDTHS-STD14-005` | unknown glyph code falls back to the font's default (space) width, never panics | PRD §8.5 | green |

### insert_text — Base-14 (`text.rs`) — `INSERT-TEXT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INSERT-TEXT-001` | `insert_text(blank, point, "Hello", helv)` → save → reopen → `get_text` contains "Hello" | PRD §8.8 | green |
| `INSERT-TEXT-002` | inserted glyph origin lands at the PyMuPDF top-left `point` (y-down → PDF y-up conversion) | PRD §8.6.1 | green |
| `INSERT-TEXT-003` | multi-line text (`\n`) emits one positioned line per split, leading = fontsize·1.2 | PRD §8.8 | green |
| `INSERT-TEXT-004` | color (rgb) is reflected on the extracted glyph span color | PRD §8.8 | green |
| `INSERT-TEXT-005` | a Base-14 `/Type1 /BaseFont /Helvetica` font resource is registered (no embedding) | PRD §8.5 | green |
| `INSERT-TEXT-006` | inserting onto a page with existing content leaves the existing text extractable | PRD §8.8 | green |
| `INSERT-TEXT-007` | parentheses / backslashes in text are escaped; reopen extracts them verbatim | PRD §8.8 | green |
| `INSERT-TEXT-008` | `fontname` aliases (`tiro`→Times, `cour`→Courier) register the right BaseFont | PRD §8.5 | green |

### insert_text — TTF full-embed (`fontfile.rs`) — `INSERT-TTF-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INSERT-TTF-001` | embedding a user TTF emits a `/Type0` Identity-H font with a `/CIDFontType2` descendant + `FontFile2` | PRD §8.5.2 | green |
| `INSERT-TTF-002` | a `/ToUnicode` CMap is written; reopen → glyphs map back to the original text | PRD §8.5.2 | green |
| `INSERT-TTF-003` | per-glyph `/W` widths come from the TTF `hmtx` table (ttf-parser) | PRD §8.5 | green |
| `INSERT-TTF-004` | the whole font program is embedded (FontFile2 length == input length) — full-embed fallback | PRD §8.5.2 | green |
| `INSERT-TTF-005` | a malformed / non-font byte blob is rejected with a typed error (never panics) | PRD §8.5.2 | green |

### insert_textbox — wrap / align / overflow (`text.rs`) — `INSERT-TEXTBOX-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INSERT-TEXTBOX-001` | text wraps to multiple lines within `rect` width; all words extractable | PRD §8.8 | green |
| `INSERT-TEXTBOX-002` | `align=center` centers each line; `align=right` right-justifies (origin offsets differ) | PRD §8.8 | green |
| `INSERT-TEXTBOX-003` | returns the unused height (>0 when text fits) | PRD §8.8 | green |
| `INSERT-TEXTBOX-004` | returns a negative overflow value when text does not fit (PyMuPDF convention) | PRD §8.8 | green |
| `INSERT-TEXTBOX-005` | explicit `\n` forces a line break inside the box | PRD §8.8 | green |

### insert_image — JPEG passthrough + raw (`image.rs`) — `INSERT-IMAGE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INSERT-IMAGE-001` | JPEG bytes → image XObject with `/Filter /DCTDecode` (no re-encode; raw passthrough) | PRD §8.8 | green |
| `INSERT-IMAGE-002` | image placed with a `cm` matrix mapping the unit square to `rect`; reopen → `interpret_page` lists it with the right CTM | PRD §8.8 | green |
| `INSERT-IMAGE-003` | the XObject is registered under `/Resources /XObject` and emitted as `q cm /Img Do Q` | PRD §8.8 | green |
| `INSERT-IMAGE-004` | raw RGB pixels → `/FlateDecode` XObject with `/ColorSpace /DeviceRGB`, `/BitsPerComponent 8` | PRD §8.8 | green |
| `INSERT-IMAGE-005` | non-JPEG / bad bytes for the JPEG path are rejected with a typed error (never panics) | PRD §8.8 | green |

### draw_* primitives + Shape (`drawing.rs`) — `DRAW-*` / `SHAPE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `DRAW-LINE-001` | `draw_line(p1, p2)` emits `m … l … S`; reopen content shows the path operators | PRD §8.8 | green |
| `DRAW-RECT-001` | `draw_rect` emits `re` + stroke; coordinates converted from top-left space | PRD §8.8 | green |
| `DRAW-CIRCLE-001` | `draw_circle` emits 4 cubic Béziers (κ≈0.5523) closed with `h` | PRD §8.8 | green |
| `DRAW-OVAL-001` | `draw_oval(rect)` emits 4 Béziers fitting the rect | PRD §8.8 | green |
| `DRAW-BEZIER-001` | `draw_bezier` emits a single `c` curve | PRD §8.8 | green |
| `DRAW-POLYLINE-001` | `draw_polyline` emits `m` + chained `l`; `draw_curve` emits a smooth `c` | PRD §8.8 | green |
| `DRAW-FILL-001` | a fill color → `rg`/`f`; stroke color → `RG`/`S`; both → `B` | PRD §8.8 | green |
| `DRAW-WIDTH-001` | line width emits `w`; dashes emit `d` | PRD §8.8 | green |
| `SHAPE-001` | `Shape` accumulates several path ops then `finish` + `commit` emits one balanced `q … Q` chunk | PRD §8.8 | green |
| `SHAPE-002` | multiple `finish` blocks with different colors are all committed | PRD §8.8 | green |

### insertion robustness (`*_e2e.rs`) — `INSERT-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `INSERT-PROP-001` | inserting never corrupts existing content (existing text still extractable after save→reopen) | PRD §8.8 | green |
| `INSERT-PROP-002` | inserting onto a page whose `/Contents` is an array (multi-stream) works | PRD §8.8 | green |
| `INSERT-PROP-003` | a saved file with inserted content reparses clean (no dangling refs; valid xref) | PRD §8.8 | green |
| `INSERT-PROP-003-QPDF` | mixed text+image+vector save passes `qpdf --check` (skipped if qpdf absent) | PRD §8.8 | green |
| `INSERT-PROP-004` | repeated insertions on the same page accumulate (idempotent resource-name allocation) | PRD §8.8 | green |

---

## M4b — Annotations + `/AP /N` appearance streams (`pdf-edit`)

Spec source of truth: PRD §8.8 (annotation family + `/AP` generation) and §12 M4
exit (each subtype reopens with subtype/geometry/`/AP /N`; `update()` reflects
color in AP). Annotations are indirect dicts in the page `/Annots` array added
via the ChangeSet; each `add_*_annot` sets `/Subtype` + geometry + defaults,
generates a `/AP /N` Form XObject (reusing the `drawing.rs` / `content.rs`
operator emitters), and appends to `/Annots`. The correctness oracle is
**round-trip through full save → reopen**: subtype, geometry (rect / quadpoints /
vertices / line / inklist) and a present-and-non-empty `/AP /N` Form XObject are
asserted on the reopened document; `update()` is verified by grepping the decoded
AP for the new color operator; `qpdf --check` gates the saved file (qpdf 12.3.2).
Tests live in `crates/pdf-edit/tests/annot_e2e.rs`.

### per-subtype create → reopen → `/Subtype` + geometry + `/AP /N` — `ANNOT-<TYPE>-001`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ANNOT-TEXT-001` | `add_text_annot` → `/Text` + note-icon `/AP /N`; `/Contents` preserved | PRD §8.8 | green |
| `ANNOT-FREETEXT-001` | `add_freetext_annot` → `/FreeText` + bordered box + text (`BT…Tj`) AP | PRD §8.8 | green |
| `ANNOT-HIGHLIGHT-001` | `add_highlight_annot` → `/Highlight` + 8-num QuadPoints + filled-quad AP w/ Multiply ExtGState | PRD §8.8 | green |
| `ANNOT-UNDERLINE-001` | `add_underline_annot` → `/Underline` + baseline line AP (`m l S`) | PRD §8.8 | green |
| `ANNOT-STRIKEOUT-001` | `add_strikeout_annot` → `/StrikeOut` + mid-line AP | PRD §8.8 | green |
| `ANNOT-SQUIGGLY-001` | `add_squiggly_annot` → `/Squiggly` + zig-zag polyline AP | PRD §8.8 | green |
| `ANNOT-SQUARE-001` | `add_rect_annot` → `/Square` + stroked+filled `re` AP (inset by border) | PRD §8.8 | green |
| `ANNOT-CIRCLE-001` | `add_circle_annot` → `/Circle` + 4-Bézier ellipse AP | PRD §8.8 | green |
| `ANNOT-LINE-001` | `add_line_annot` → `/Line` + `/L` endpoints + segment AP | PRD §8.8 | green |
| `ANNOT-POLYGON-001` | `add_polygon_annot` → `/Polygon` + `/Vertices` + closed-path (`h`) AP | PRD §8.8 | green |
| `ANNOT-POLYLINE-001` | `add_polyline_annot` → `/PolyLine` + `/Vertices` + open-path AP | PRD §8.8 | green |
| `ANNOT-INK-001` | `add_ink_annot` → `/Ink` + `/InkList` (per stroke) + multi-path AP | PRD §8.8 | green |
| `ANNOT-STAMP-001` | `add_stamp_annot` → `/Stamp` + bordered label-box AP with text | PRD §8.8 | green |
| `ANNOT-REDACT-001` | `add_redact_annot` → `/Redact` + QuadPoints + OverlayText (create only; apply is M4d) | PRD §8.8 | green |
| `ANNOT-FILE-001` | `add_file_annot` → `/FileAttachment` + `/AP /N`; embedded bytes extractable | PRD §8.8 | green |

### `update()` reflects properties in AP — `ANNOT-UPDATE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ANNOT-UPDATE-001` | change `/C` + `update()` → reopen → AP stroke op shows the new color; stale color gone | PRD §12 M4 | green |
| `ANNOT-UPDATE-002` | `set_opacity` + `update()` writes `/CA`; reopen reflects it | PRD §8.8 | green |

### CRUD over `/Annots` — `ANNOT-CRUD-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ANNOT-CRUD-001` | `annot_count` after adds; `delete_annot` removes one; iterate order preserved on reopen | PRD §8.8 | green |
| `ANNOT-CRUD-002` | `first_annot` / `annot_names` reflect added annots + `/NM` | PRD §8.8 | green |
| `ANNOT-CRUD-003` | `delete_annot` frees the `/AP /N` stream object (resolves to Null) | PRD §8.8 | green |

### text-markup quadpoints geometry — `ANNOT-MARKUP-QUAD-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ANNOT-MARKUP-QUAD-001` | multi-quad highlight: 16-num QuadPoints (Acrobat order) + AP fills each quad | PRD §8.8 | green |

### robustness / preservation — `ANNOT-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `ANNOT-PROP-001` | adding annots preserves existing page content/text (still extractable after reopen) | PRD §8.8 | green |
| `ANNOT-PROP-002` | degenerate inputs (empty quads/vertices/strokes/box) never panic; reopen clean | PRD §8.8 | green |
| `ANNOT-PROP-003-QPDF` | mixed-subtype annotated save passes `qpdf --check` (skipped if qpdf absent) | PRD §12 M4 | green |
| `ANNOT-PROP-004` | `Annot` accessors/mutators round-trip (color/fill/opacity/border/flags/info) never panic | PRD §8.8 | green |

---

## M4c — AcroForm forms (read / fill / flatten) + `Widget` API (`pdf-edit`)

Spec source of truth: PRD §8.8 (forms / `Widget` API) and §12 M4 exit. Self-built
AcroForm fixtures only (PRD §10): a text field, a checkbox with `/AP /N
<</On …/Off …>>`, a radio group with two kids, a combo/list choice. Tests live in
`crates/pdf-edit/tests/form_e2e.rs`; oracle = reparse + `qpdf --check`.

### AcroForm read: field tree, FQN, type, flags — `FORM-READ-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-READ-001` | `doc.form_fields()` enumerates all fields (incl. `/Kids`-nested) | PRD §8.8 | green |
| `FORM-READ-002` | fully-qualified name joins `/T` up the `/Parent` chain with `.` | PRD §8.8 | green |
| `FORM-READ-003` | field-type detection: `Tx`/`Btn`/`Ch`/`Sig` from `/FT` (inherited) | PRD §8.8 | green |
| `FORM-READ-004` | button sub-type: checkbox vs radio (`/Ff` 32768) vs pushbutton (`/Ff` 65536) | PRD §8.8 | green |
| `FORM-READ-005` | choice sub-type: combo (`/Ff` 131072) vs list | PRD §8.8 | green |
| `FORM-READ-006` | current value `/V`, default `/DV`, flags `/Ff` readable | PRD §8.8 | green |
| `FORM-READ-007` | `/NeedAppearances`, `/DA`, `/DR` parsed off `/AcroForm` | PRD §8.8 | green |

### Widget API — `WIDGET-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `WIDGET-001` | `page.widgets()` iterator + `page.first_widget` return `/Widget` annots | PRD §8.8 | green |
| `WIDGET-002` | `field_type`/`field_type_string`/`field_name`/`field_value`/`field_flags`/`rect`/`xref` | PRD §8.8 | green |
| `WIDGET-003` | `field_label` (`/TU`), `choice_values` (Ch), `button_states` (on-states from `/AP /N`) | PRD §8.8 | green |
| `WIDGET-004` | `doc.is_form_pdf` true for AcroForm; `false` + empty list for non-form | PRD §8.8 | green |

### Fill text field — `FORM-TEXT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-TEXT-001` | set value → `/V` updated; reopen persists | PRD §12 M4 | green |
| `FORM-TEXT-002` | `/AP /N` regenerated; decoded AP contains the text (`Tj`) | PRD §8.8 | green |
| `FORM-TEXT-003` | `/Q` alignment (left/center/right) reflected in AP `Tm` x | PRD §8.8 | green |
| `FORM-TEXT-004` | multiline (`/Ff` 4096) wraps to multiple `Tj` lines | PRD §8.8 | green |

### Checkbox — `FORM-CHECK-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-CHECK-001` | check → `/V` + `/AS` == on-state name discovered from `/AP /N` (not assumed `/Yes`) | PRD §12 M4 | green |
| `FORM-CHECK-002` | uncheck → `/V` + `/AS` == `/Off` | PRD §12 M4 | green |

### Radio group — `FORM-RADIO-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-RADIO-001` | select one kid → group `/V` == on-state; only that kid `/AS` on, others `/Off` | PRD §12 M4 | green |

### Choice (combo / list) — `FORM-CHOICE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-CHOICE-001` | set combo `/V`; `choice_values` readable; AP shows selected | PRD §8.8 | green |
| `FORM-CHOICE-002` | set list `/V`; reopen persists | PRD §8.8 | green |

### Flatten — `FORM-FLATTEN-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-FLATTEN-001` | flatten removes `/Root /AcroForm` and all `/Widget` annots | PRD §12 M4 | green |
| `FORM-FLATTEN-002` | filled value baked into page content (widget `/AP` drawn as Form XObject `Do`); value visible | PRD §12 M4 | green |
| `FORM-FLATTEN-003` | flattened output reopens valid + passes `qpdf --check` (skipped if absent) | PRD §12 M4 | green |

### Robustness / policy — `FORM-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `FORM-PROP-001` | non-form PDF: `is_form_pdf` false, `form_fields()`/`widgets()` empty; never panics | PRD §8.8 | green |
| `FORM-PROP-002` | read-only field (`/Ff` 1) set → typed error; value unchanged | PRD §8.8 | green |
| `FORM-PROP-003` | degenerate dicts (missing `/FT`, `/Rect`, `/AP`) never panic | PRD §8.8 | green |
| `FORM-PROP-004-QPDF` | filled form full-save passes `qpdf --check` (skipped if qpdf absent) | PRD §12 M4 | green |

---

## M4d — Redaction (multi-surface destructive) + `get_drawings` (`pdf-edit`)

Spec source of truth: PRD §8.8 (redaction multi-surface destructive guarantee +
acceptance gate; `get_drawings`/`get_cdrawings`) and §12 M4 exit. Self-built
fixtures only (PRD §10). The acceptance gate runs over the **fully-decompressed**
corpus (every stream + objstm expanded) — a compressed-only grep is forbidden.
Tests live in `crates/pdf-edit/tests/{redact_e2e.rs,drawings_e2e.rs}`.

### Redaction security gate — `REDACT-SECURITY-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REDACT-SECURITY-001` | secret over a known rect → apply → full save → decompress every stream + objstm → secret bytes appear **nowhere** in the decompressed corpus AND not in `get_text()`; surrounding text intact + unshifted | PRD §12 M4 | green |
| `REDACT-SECURITY-002` | gate over the **compressed** save would false-pass without decompression — assert decompressed corpus is what catches it (deflate=1) | PRD §8.8 | green |

### Text removal — `REDACT-TEXT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REDACT-TEXT-001` | partial-line rect drops only the intersecting glyphs; survivors remain with unshifted positions | PRD §8.8 | green |
| `REDACT-TEXT-002` | multiple redaction rects on one page each remove their glyphs | PRD §8.8 | green |
| `REDACT-TEXT-003` | glyph drawn via a Form XObject under the rect is removed from the saved bytes | PRD §8.8 | green |
| `REDACT-TEXT-004` | redaction count / changed-status reported; non-overlapping text fully preserved | PRD §8.8 | green |

### Image redaction — `REDACT-IMAGE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REDACT-IMAGE-001` | fully-covered image XObject `Do` removed from content; cover box drawn | PRD §8.8 | green |
| `REDACT-IMAGE-002` | raw Flate RGB image partially covered → covered pixels zeroed + re-encoded (decode & verify) | PRD §8.8 | green |
| `REDACT-IMAGE-003` | undecodable (DCT/JBIG2/JPX) image under the rect → fail-closed `Error::Redaction` | PRD §8.8 | green |

### Cover + annot cleanup — `REDACT-COVER-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REDACT-COVER-001` | redaction fill box (default black) drawn over each region in page content | PRD §8.8 | green |
| `REDACT-COVER-002` | `/Redact` annotations removed after apply; reopen has none | PRD §8.8 | green |

### Incremental-after-redaction — `REDACT-INCR-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REDACT-INCR-001` | `save_incremental` after redaction rejected (`IncrementalRequiresCleanParse`); `can_save_incrementally` false | PRD §12 M4 | green |
| `REDACT-INCR-002` | `OnRepaired::Upgrade` auto-upgrades a post-redaction incremental save to a full rewrite (secret absent) | PRD §12 M4 | green |

### Robustness / properties — `REDACT-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `REDACT-PROP-001` | no redaction annots → `apply_redactions` is a no-op (count 0); page unchanged | PRD §8.8 | green |
| `REDACT-PROP-002` | redacting an empty region (no overlap) preserves all glyphs | PRD §8.8 | green |
| `REDACT-PROP-003` | degenerate inputs never panic; redacted save passes `qpdf --check` (skipped if absent) | PRD §12 M4 | green |

### Vector path extraction — `DRAWINGS-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `DRAWINGS-001` | `draw_rect` (stroke) → one `type "s"` path with an `("re", rect)` item + stroke color + width | PRD §8.8 | green |
| `DRAWINGS-002` | `draw_line` → `type "s"` path with an `("l", p1, p2)` item; rect spans the segment | PRD §8.8 | green |
| `DRAWINGS-003` | filled rect → `type "f"` path with `fill` color set, `color` None | PRD §8.8 | green |
| `DRAWINGS-004` | fill+stroke rect → `type "fs"` with both colors | PRD §8.8 | green |
| `DRAWINGS-005` | even-odd fill (`f*`) sets `even_odd`; closed polyline sets `close_path` | PRD §8.8 | green |
| `DRAWINGS-006` | `get_cdrawings` (raw user-space variant) returns the same item geometry pre device transform | PRD §8.8 | green |
| `DRAWINGS-007` | curve (`c`) captured as a `("c", p1,p2,p3,p4)` item | PRD §8.8 | green |
| `DRAWINGS-PROP-001` | empty / text-only page → no drawings; never panics | PRD §8.8 | green |

## M4e — scrub / bake (pdf-edit)

`scrub` is a conservative PyMuPDF-style sanitizer over the catalog + trailer;
`bake` flattens widgets (via `form::flatten`) and non-widget annotations (drawing
each `/AP /N` as a Form XObject `Do` into page content) into static content.
Tests live in `crates/pdf-edit/tests/scrub_e2e.rs`.

### Scrub — `SCRUB-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `SCRUB-META-001` | `scrub(metadata=true)` detaches the trailer `/Info` (reopen: none) and removes catalog `/Metadata` (XMP); secret bytes absent from the decompressed corpus | PRD §8.8 | green |
| `SCRUB-JS-001` | `scrub(javascript=true)` removes catalog `/OpenAction`, `/AA` and the `/Names /JavaScript` name-tree (asserted after reopen) | PRD §8.8 | green |
| `SCRUB-EMBFILE-001` | `scrub(attached_files=true)` removes `/Names /EmbeddedFiles` | PRD §8.8 | green |
| `SCRUB-LINKS-001` | `scrub(remove_links=true)` drops `/Link` annots from every page; non-link annots survive | PRD §8.8 | green |
| `SCRUB-IDEMPOTENT-001` | running a full `scrub` twice is safe (no panic, no error); result stable | PRD §8.8 | green |
| `SCRUB-PROP-001` | `scrub` on a minimal blank doc (none of those features) → no-op, never panics, nothing invented | PRD §8.8 | green |

### Bake — `BAKE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `BAKE-WIDGETS-001` | `bake(widgets=true)` on the AcroForm fixture → `/AcroForm` removed, no widgets remain, a Form XObject `Do` baked into page content | PRD §8.8 | green |
| `BAKE-ANNOTS-001` | `bake(annots=true)` draws a markup annotation's `/AP /N` into page content via `Do`, removes it from `/Annots`, and frees the annotation object | PRD §8.8 | green |

---

## M4e — Embedded files (`pdf-edit`)

Spec source of truth: PRD §8.8 (embedded-file collection over the catalog
`/Names /EmbeddedFiles` name-tree). Self-built fixtures only (PRD §10). The core
oracle is a **byte-exact** add → get round trip; persistence is asserted through
the save/reopen reparse oracle. Reads walk a general (flat-leaf or
`/Kids`+`/Limits`) tree; writes collapse to a single sorted flat leaf. Tests
live in `crates/pdf-edit/tests/embfile_e2e.rs`.

### Embedded files — `EMBFILE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `EMBFILE-ADD-GET-001` | add then get returns the byte-exact original payload | PRD §8.8 | green |
| `EMBFILE-ADD-002` | filename/ufilename/desc stored on the filespec + readable via `info` | PRD §8.8 | green |
| `EMBFILE-NAMES-001` | `names()` lists added keys sorted byte-wise; `count()` matches | PRD §8.8 | green |
| `EMBFILE-MULTI-001` | multiple files (4) each round-trip byte-exact (incl. empty/binary); names sorted | PRD §8.8 | green |
| `EMBFILE-DEL-001` | `del` removes the key; `get` errors; count decremented; survivors intact | PRD §8.8 | green |
| `EMBFILE-INFO-001` | `info` reports filename/ufilename/desc/size (size == decoded length; length aliases size) | PRD §8.8 | green |
| `EMBFILE-PERSIST-001` | add → full save → reopen → get still byte-exact; metadata preserved; del survives reopen | PRD §12 M4 | green |
| `EMBFILE-PROP-001` | get/del/info on a non-existent name → typed `InvalidArgument`, never panics; duplicate add rejected; no-`/Names` doc enumerates empty | PRD §8.8 | green |
| `EMBFILE-EXISTING-TREE-001` | reading a pre-built multi-level (`/Kids`+`/Limits`) tree enumerates all keys; add collapses to flat leaf keeping every key (survives reopen) | PRD §8.8 | green |

---

## M4e — PyO3 / fitz wiring for the full M4 edit surface (Python gates) — `PYM4-*`

Spec source of truth: PRD §9.4 (PyO3 handle/GIL), §9.5 (fitz shim), §8.8 (annot
/ redaction / forms / drawings / embfile / scrub), and §12 M4 exit (Python
redaction gone-after-reopen; annot `/AP` portability). These exercise the native
`oxide_pdf` package and the `fitz` deprecated-alias shim end-to-end (build →
edit → `tobytes`/`save` → reopen → assert). All fixtures self-generated in-test
(PRD §10); the secret-bearing fixture uses a font with explicit `/Widths` so the
interpreter can measure glyph advances (same convention as the Rust harness).
Tests live in `python/tests/test_m4.py`.

### Content insert / draw / Shape — `PYM4-INSERT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYM4-INSERT-001` | `Page.insert_text(point, "X")` → `tobytes` → reopen → `get_text` contains it | PRD §8.8 | green |
| `PYM4-INSERT-002` | `Page.insert_textbox(rect, text)` returns a float; reopen extracts the text | PRD §8.8 | green |
| `PYM4-DRAW-001` | `draw_rect`/`draw_line` then reopen valid; `get_drawings()` lists the path | PRD §8.8 | green |
| `PYM4-SHAPE-001` | `Page.new_shape()` → `draw_rect`+`finish`+`commit` → reopen valid; drawing present | PRD §8.8 | green |

### Annotations + `/AP` portability — `PYM4-ANNOT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYM4-ANNOT-001` | `add_highlight_annot`+`add_freetext_annot` → `annots()` lists both with correct `.type`/`.rect` | PRD §8.8 | green |
| `PYM4-ANNOT-002` | `annot.set_colors(stroke=…)` then `update()` reflects in `.colors`; reopen persists subtype + `/AP /N` | PRD §12 M4 | green |
| `PYM4-ANNOT-003` | `delete_annot` removes it; `annot_xrefs` shrinks; reopen lacks it | PRD §8.8 | green |
| `PYM4-ANNOT-004` | every added subtype reopens with an appearance stream (`/AP /N`) present (portability gate) | PRD §12 M4 | green |

### Redaction Python gate (gone-after-reopen) — `PYM4-REDACT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYM4-REDACT-001` | `add_redact_annot` over a secret → `apply_redactions()` → save to tmp → reopen → `get_text()` lacks the secret; neighbouring text intact | PRD §12 M4 | green |
| `PYM4-REDACT-002` | `apply_redactions` on a page with no redaction annots → returns 0 (no-op) | PRD §8.8 | green |

### Forms / Widget — `PYM4-WIDGET-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYM4-WIDGET-001` | self-built AcroForm: `page.widgets()` lists the field with `field_name`/`field_type_string` | PRD §8.8 | green |
| `PYM4-WIDGET-002` | `widget.update("new")` sets the value; reopen reflects `field_value`; `is_form_pdf` true | PRD §12 M4 | green |

### Embedded files / scrub via Python — `PYM4-EMBFILE-*` / `PYM4-SCRUB-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYM4-EMBFILE-001` | `embfile_add`/`embfile_get`/`embfile_names`/`embfile_count`/`embfile_info` round-trip; persists across `tobytes`+reopen | PRD §8.8 | green |
| `PYM4-SCRUB-001` | `set_metadata` then `scrub(metadata=True)` clears `metadata` (title/author empty after) | PRD §8.8 | green |

### fitz deprecated-alias parity — `PYM4-FITZ-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYM4-FITZ-001` | `page.addHighlightAnnot`/`applyRedactions`/`getDrawings`/`insertText`/`newShape`/`firstAnnot` resolve and behave as the snake_case methods | PRD §9.5 | green |
| `PYM4-FITZ-002` | `Annot`/`Widget`/`Shape` are exposed as `fitz` classes (identity with `oxide_pdf`) | PRD §9.5 | green |

---

## M5 — Image documents, codecs, Pixmap (`pdf-image`)

Spec source of truth: PRD §8.4 / §8.4.1 (image-XObject codecs + documented-subset
degradation contract), §8.10 (image-document loader / `convert_to_pdf`), §3.3 /
§9.4 (`Pixmap` / `get_pixmap` / `extract_image` + PyO3 buffer protocol), §9.6.2
(pixel cap / never-OOM). Every codec is **total**: arbitrary / truncated / corrupt
input yields a typed `Err` (`decode` / `unsupported` / `limit-exceeded`), never a
panic, and a declared-huge raster trips the 256 Mpx pixel cap before allocating.
Fixtures are **self-built** in-test (round-tripped through pure-Rust encoders, or
hand-assembled segment/IFD grammars); no external/PyMuPDF files (PRD §10). Tests
live in `crates/pdf-image/tests/{dct,ccitt,jbig2,jpx,dispatch,codec_property,imagedoc,pixmap,getpixmap}.rs`
and `python/tests/test_pixmap.py`.

### DCTDecode / JPEG (`codecs::dct`) — `DCT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `DCT-RGB-001` | baseline RGB: dimensions + components + plausible pixels | PRD §8.4 | green |
| `DCT-GRAY-001` | baseline grayscale → 1 component, `Gray` hint | PRD §8.4 | green |
| `DCT-PROG-001` | progressive JPEG decodes to the right geometry | PRD §8.4 | green |
| `DCT-XCHECK-001` | zune-jpeg vs `jpeg-decoder` oracle agree within IDCT tolerance | PRD §8.4.1 | green |
| `DCT-CMYK-001` | native 4-component CMYK preserved (`Cmyk` hint) | PRD §8.4 | green |
| `DCT-CMYK-DECODE-001` | `/Decode [1 0 …]` inverts; APP14 Adobe default un-inversion matches | PRD §8.4 | green |
| `DCT-ERR-001` | garbage (non-JPEG) → typed `decode` error, no panic | PRD §8.4.1 | green |
| `DCT-ERR-002` | truncated JPEG fails closed (never a wrong-size `Ok`) | PRD §8.4.1 | green |

### CCITTFaxDecode / Group 4 (`codecs::ccitt`) — `CCITT-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CCITT-G4-001` | G4 (K=-1) round-trips a known bitmap; 1-bpc `Gray` | PRD §8.4 | green |
| `CCITT-G4-002` | G4 round-trips a richer diagonal + block pattern | PRD §8.4 | green |
| `CCITT-BLACKIS1-001` | `/BlackIs1` inverts every pixel | PRD §8.4 | green |
| `CCITT-DEFAULT-COLUMNS-001` | absent `/Columns` defaults to 1728 | PRD §8.4 | green |
| `CCITT-ERR-001` | non-fax bytes → no panic; error or bounded declared-size raster | PRD §8.4.1 | green |

### JBIG2Decode (`codecs::jbig2`) — `JBIG2-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `JBIG2-GENERIC-001` | embedded generic-region (MMR) bitmap round-trips | PRD §8.4.1 | green |
| `JBIG2-GENERIC-002` | a richer generic-region pattern round-trips | PRD §8.4.1 | green |
| `JBIG2-ERR-001` | garbage → typed `unsupported`/`decode`/`limit-exceeded`, no panic | PRD §8.4.1 | green |
| `JBIG2-ERR-002` | empty input fails closed | PRD §8.4.1 | green |

### JPXDecode / JPEG 2000 (`codecs::jpx`) — `JPX-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `JPX-GRAY-001` | baseline grayscale JP2 → 1 component, `Gray` hint | PRD §8.4.1 | green |
| `JPX-RGB-001` | baseline sRGB JP2 → 3 components, `Rgb` hint | PRD §8.4.1 | green |
| `JPX-ERR-001` | garbage → typed `unsupported`/`decode`/`limit-exceeded`, no panic | PRD §8.4.1 | green |
| `JPX-ERR-002` | empty input fails closed | PRD §8.4.1 | green |

### Dispatcher + raw samples + caps (`codecs::decode_image_xobject`) — `CODEC-DISPATCH-*` / `CODEC-CAP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CODEC-DISPATCH-001` | `DCTDecode` and the `DCT` abbreviation both route to the JPEG codec | PRD §8.4 | green |
| `CODEC-DISPATCH-002` | unknown filter name → typed `unsupported` error, no panic | PRD §8.4.1 | green |
| `CODEC-DISPATCH-003` | raw/Flate samples interpreted by `/ColorSpace` (DeviceRGB 8bpc) | PRD §8.4 | green |
| `CODEC-DISPATCH-004` | 1-bpp `/ImageMask` raw samples preserved | PRD §8.4 | green |
| `CODEC-DISPATCH-005` | 16-bit big-endian gray raw samples preserved | PRD §8.4 | green |
| `CODEC-DISPATCH-006` | too-few raw bytes for declared geometry → typed `decode` error | PRD §8.4.1 | green |
| `CODEC-CAP-001` | declared-huge raster (raw path) trips the pixel cap, no OOM | PRD §9.6.2 | green |
| `CODEC-CAP-002` | cap applies to codec filters too (huge JBIG2 page rejected) | PRD §9.6.2 | green |

### Codec totality (proptest, `codec_property.rs`) — `CODEC-PROP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `CODEC-PROP-001` | `dct::decode` never panics on arbitrary bytes | PRD §8.4.1 | green |
| `CODEC-PROP-002` | `ccitt::decode` never panics on arbitrary bytes + dims | PRD §8.4.1 | green |
| `CODEC-PROP-003` | `jbig2::decode` never panics on arbitrary bytes | PRD §8.4.1 | green |
| `CODEC-PROP-004` | `jpx::decode` never panics on arbitrary bytes | PRD §8.4.1 | green |
| `CODEC-PROP-005` | dispatcher respects the pixel cap + total for any filter/bytes/dims | PRD §9.6.2 | green |

### Image-document loader / `convert_to_pdf` (`imagedoc`) — `IMGDOC-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `IMGDOC-SNIFF-001` | `ImageFormat::sniff` detects PNG/JPEG/TIFF/GIF/BMP | PRD §8.10 | green |
| `IMGDOC-SNIFF-002` | sniff rejects non-image / empty / PDF bytes → `None` | PRD §8.10 | green |
| `IMGDOC-PNG-001` | PNG → single page; MediaBox == pixel dims (1px = 1pt, no DPI) | PRD §8.10 | green |
| `IMGDOC-PNG-002` | PNG image XObject: Width/Height/`DeviceRGB`/8bpc/`FlateDecode` | PRD §8.10 | green |
| `IMGDOC-PNG-003` | grayscale PNG → `/ColorSpace /DeviceGray` | PRD §8.10 | green |
| `IMGDOC-PNG-004` | `open_image_document` PNG → 1 page, RGB pixmap (n=3, no alpha) | PRD §8.10 | green |
| `IMGDOC-JPEG-001` | JPEG → `/DCTDecode` passthrough; embedded stream byte-equal | PRD §8.10 | green |
| `IMGDOC-JPEG-002` | 3-component baseline JPEG → `/ColorSpace /DeviceRGB` from SOF | PRD §8.10 | green |
| `IMGDOC-ALPHA-001` | RGBA PNG → `/SMask` DeviceGray image of matching dims | PRD §8.10 | green |
| `IMGDOC-ALPHA-002` | `open_image_document` RGBA → pixmap reports alpha (n=4) | PRD §8.10 | green |
| `IMGDOC-ALPHA-003` | LumaA PNG → `DeviceGray` + `/SMask` present | PRD §8.10 | green |
| `IMGDOC-PALETTE-001` | native palette PNG → `[/Indexed /DeviceRGB hival lut]` colorspace | PRD §8.10 | green |
| `IMGDOC-TIFF-001` | single-IFD TIFF → 1 page, correct dims | PRD §8.10 | green |
| `IMGDOC-TIFF-002` | multi-IFD TIFF → `page_count == IFD count`, per-page dims | PRD §8.10 | green |
| `IMGDOC-TIFF-003` | multi-IFD TIFF → one PDF page per IFD with per-page MediaBox | PRD §8.10 | green |
| `IMGDOC-GIF-001` | animated GIF → one page per frame (loader + convert) | PRD §8.10 | green |
| `IMGDOC-BMP-001` | BMP → single page; MediaBox == pixel dims | PRD §8.10 | green |
| `IMGDOC-FORMAT-001` | `format = None` auto-detects via sniff (convert + open) | PRD §8.10 | green |
| `IMGDOC-CONVERT-001` | PNG/JPEG/BMP convert output reparses clean, page found | PRD §8.10 | green |
| `IMGDOC-CONVERT-002` | converted output passes `qpdf --check` (skipped if qpdf absent) | PRD §8.10 | green |
| `IMGDOC-PROP-001` | non-image bytes → typed `invalid-argument`, no panic | PRD §8.10 | green |
| `IMGDOC-PROP-002` | truncated PNG → typed `decode`/`invalid-argument` error | PRD §8.10 | green |
| `IMGDOC-PROP-003` | corrupt JPEG (SOI, no SOF) → typed `decode` error | PRD §8.10 | green |
| `IMGDOC-PROP-004` | arbitrary byte patterns never panic (open + convert) | PRD §9.6 | green |
| `IMGDOC-PROP-005` | self-referential TIFF IFD cycle → typed `decode`, no hang | PRD §9.6 | green |

### `Pixmap` value type (`pixmap`) — `PIXMAP-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PIXMAP-NEW-001` | `Pixmap::new` from raw RGB → width/height/n/stride/colorspace | PRD §3.3 | green |
| `PIXMAP-NEW-002` | alpha bumps `n` and `stride` | PRD §3.3 | green |
| `PIXMAP-NEW-003` | `try_new` rejects a wrong-length buffer | PRD §3.3 | green |
| `PIXMAP-BLANK-001` | `blank` fills + sizes; zero dimension rejected | PRD §3.3 | green |
| `PIXMAP-DECODED-001` | from a `DecodedImage` (8-bit RGB) preserves samples | PRD §8.10 | green |
| `PIXMAP-DECODED-002` | 1-bit gray upscales to 0/255 | PRD §8.10 | green |
| `PIXMAP-DECODED-003` | 16-bit takes the high byte | PRD §8.10 | green |
| `PIXMAP-SAVE-001` | `to_png_bytes` RGB round-trips through the `image` decoder | PRD §3.3 | green |
| `PIXMAP-SAVE-002` | gray+alpha PNG round-trips | PRD §3.3 | green |
| `PIXMAP-TOBYTES-001` | `tobytes("png")` == `to_png_bytes`; PAM carries alpha; bad fmt errors | PRD §3.3 | green |
| `PIXMAP-PIXEL-001` | pixel get/set; out-of-range / wrong-arity rejected | PRD §3.3 | green |
| `PIXMAP-COW-001` | mutation does not disturb an exported (Arc) clone | PRD §9.4 | green |
| `PIXMAP-ALPHA-001` | `set_alpha` touches only the alpha lane; no-op without alpha | PRD §3.3 | green |
| `PIXMAP-SMASK-001` | attach a gray `/SMask` as the alpha channel | PRD §8.10 | green |
| `PIXMAP-INVERT-001` | `invert_irect` flips color, keeps alpha | PRD §3.3 | green |
| `PIXMAP-CMYK-001` | CMYK pixmap saves as an RGB PNG | PRD §3.3 | green |

### `get_pixmap` / `extract_image` on pages (`getpixmap`) — `PIXMAP-IMGONLY-*` / `EXTRACT-IMAGE-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PIXMAP-IMGONLY-001` | classify a single-image page as image-only | PRD §8.10 | green |
| `PIXMAP-IMGONLY-002` | `page_pixmap` == decoder output (pixel-equality) | PRD §8.10 | green |
| `PIXMAP-IMGONLY-003` | vector page (path paint) → typed `unsupported` error | PRD §8.10 | green |
| `PIXMAP-IMGONLY-004` | text page (`BT…Tj`) classified vector | PRD §8.10 | green |
| `PIXMAP-IMGONLY-005` | scale arg scales the output dimensions | PRD §8.10 | green |
| `PIXMAP-IMGONLY-006` | `alpha=true` adds an opaque alpha channel | PRD §8.10 | green |
| `PIXMAP-IMGONLY-007` | undecodable image-only page → typed `decode`/`unsupported` error | PRD §8.4.1 | green |
| `EXTRACT-IMAGE-001` | raw raster → PNG-encoded descriptor (dims/bpc/colorspace/components) | PRD §8.10 | green |
| `EXTRACT-IMAGE-002` | DCT image → JPEG passthrough (verbatim bytes) | PRD §8.10 | green |
| `EXTRACT-IMAGE-003` | non-image xref → typed `invalid-argument` error | PRD §8.10 | green |

### Python `Pixmap` / `get_pixmap` / `extract_image` + buffer protocol — `PYPIXMAP-*` / `PIXMAP-BUF-LIFETIME` / `PYEXTRACT-IMAGE-*` / `PYFITZ-PIXMAP`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYPIXMAP-001` | image-only page `get_pixmap` → correct w/h/n/colorspace + pixel-equal samples | PRD §9.4 | green |
| `PYPIXMAP-002` | `pix.save()` writes a PNG whose IHDR geometry round-trips | PRD §9.4 | green |
| `PYPIXMAP-003` | `memoryview(pix)` is readonly `B`; `samples_mv` is the same zero-copy view | PRD §9.4 | green |
| `PIXMAP-BUF-LIFETIME` | live view survives dropping the Pixmap; in-place mutate copies-on-write | PRD §9.4 | green |
| `PYPIXMAP-VECTOR` | vector page `get_pixmap` now RENDERS full-page (M6d); fill color present | PRD §8.11 | green |
| `PYPIXMAP-UNDECODABLE` | broken image skipped (degradation contract); `get_text` + `get_pixmap` both work | PRD §8.4.1 | green |
| `PYPIXMAP-SCALE` | `dpi=144` and `matrix=2` both double the output dims; `alpha=True` opaque | PRD §9.4 | green |
| `PYPIXMAP-BLANK` | `Pixmap` constructor + `pixel`/`set_pixel` | PRD §9.4 | green |
| `PYEXTRACT-IMAGE-001` | `doc.extract_image(xref)` → dict (ext/width/height/bpc/colorspace/n/image) | PRD §9.4 | green |
| `PYFITZ-PIXMAP` | `fitz.Pixmap is oxide_pdf.Pixmap`; `get_pixmap`/`getPixmap` + `extract_image`/`extractImage` parity | PRD §9.5 | green |

### M6d — full-page render (`render_page` / `DisplayList`) — `RENDER-PAGE-*` / `DISPLAYLIST-*`

The M6d integration: the `pdf-text` content interpreter, run with its opt-in
ordered render-op sink ([`interpret_page_render`]), emits a document-order
[`RenderOp`] stream; `render_page` replays it onto a `Canvas` in z-order,
dispatching to the M6a/b/c primitives, and `into_pixmap`s the result. The
`DisplayList` is the recorded stream (record once → replay). Tests live in
`crates/pdf-render/tests/render_page.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `RENDER-PAGE-001` | a self-built text page renders → non-blank Pixmap of the scaled CropBox size | PRD §8.11 | green |
| `RENDER-PAGE-002` | a filled rect paints the fill color at its interior; white elsewhere | PRD §8.11 | green |
| `RENDER-PAGE-003` | z-order: a later fill paints over an earlier one at the overlap | PRD §8.11 | green |
| `RENDER-PAGE-004` | embedded-TTF text paints glyph pixels where the text sits | PRD §8.11 | green |
| `RENDER-PAGE-005` | text + vector + image composed in one page; each region present | PRD §8.11 | green |
| `RENDER-PAGE-006` | `dpi`/matrix scale changes the output pixel dimensions | PRD §8.11 | green |
| `RENDER-PAGE-007` | `/Rotate 90` swaps the output width/height | PRD §8.11 | green |
| `RENDER-PAGE-008` | a `W n` clip restricts a following fill to the clip region | PRD §8.11 | green |
| `RENDER-PAGE-009` | an embedded image XObject paints its colors at its placement | PRD §8.11 | green |
| `RENDER-PAGE-010` | a vector page renders (no `Unsupported`); output is non-blank | PRD §8.11 | green |
| `RENDER-PAGE-011` | `alpha=true` output carries a 4-channel transparent background | PRD §8.11 | green |
| `RENDER-PAGE-012` | non-embedded font → no glyph pixels, but the page still renders | PRD §8.11 | green |
| `RENDER-PAGE-PROP-001` | arbitrary page content never panics; always returns a Pixmap | PRD §8.1 | green |
| `DISPLAYLIST-001` | `DisplayList::from_page` records the ordered op stream (non-empty) | PRD §8.11 | green |
| `DISPLAYLIST-002` | `dl.get_pixmap()` == `render_page()` (pixel-equal replay) | PRD §8.11 | green |
| `DISPLAYLIST-003` | replay at two scales yields the two correct dimensions | PRD §8.11 | green |
| `DISPLAYLIST-004` | `dl.rect()` is the page CropBox | PRD §8.11 | green |

### M6d — Python `get_pixmap` on rendered pages + `get_displaylist` — `PYRENDER-*`

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYRENDER-001` | `page.get_pixmap()` on a self-built text page → non-blank Pixmap of expected size | PRD §9.4 | green |
| `PYRENDER-002` | a vector page renders (no `PdfUnsupportedError`); `pix.save(png)` works | PRD §9.4 | green |
| `PYRENDER-003` | `dpi=144` doubles the rendered dimensions of a vector page | PRD §9.4 | green |
| `PYRENDER-004` | `page.get_displaylist().get_pixmap()` matches `page.get_pixmap()` | PRD §9.4 | green |
| `PYRENDER-005` | `fitz.open(...).load_page(0).get_pixmap()` parity on a rendered page | PRD §9.5 | green |

---

## M7 — Tables / Optional Content (OCG) / SVG export

Spec source of truth: PyMuPDF `Page.find_tables`/`Table`/`TableFinder`,
`Document` OCG/layer API, and `Page.get_svg_image` (PRD §7). The Rust engine
rows below were merged from the parallel M7 worktrees and are green; the
`pdf-api` facade + PyO3 + `fitz` shim rows are the M7 consolidation.

### M7 — table detection (`pdf_text::tables`) — `TABLES-LINES-*` / `TABLES-TEXT-*` / `TABLES-HTML-*` / `TABLES-SPAN-*` / `TABLES-NONE-*`

Tests live in `crates/pdf-text/tests/tables_{lines,text,html,none}.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `TABLES-LINES-001` | detect a single 2×3 ruled table | PRD §7 | green |
| `TABLES-LINES-002` | `extract()` returns cell strings in order | PRD §7 | green |
| `TABLES-LINES-003` | `to_markdown()` pipe-delimited shape | PRD §7 | green |
| `TABLES-LINES-004` | every cell rect lies within `bbox` | PRD §7 | green |
| `TABLES-LINES-005` | `lines_strict` also detects the ruled grid | PRD §7 | green |
| `TABLES-LINES-006` | header is the first row | PRD §7 | green |
| `TABLES-TEXT-001` | detect a grid from word spacing (no rulings) | PRD §7 | green |
| `TABLES-TEXT-002` | `text` strategy extracts cell text | PRD §7 | green |
| `TABLES-TEXT-003` | `to_markdown` uses the first row as header | PRD §7 | green |
| `TABLES-TEXT-004` | `lines` strategy falls back to `text` when no rulings | PRD §7 | green |
| `TABLES-HTML-001` | regular table → one `<td>` per cell | PRD §7 | green |
| `TABLES-HTML-002` | HTML escapes `<`/`>`/`&` in cell text | PRD §7 | green |
| `TABLES-HTML-003` | multi-line cell uses `<br>` | PRD §7 | green |
| `TABLES-HTML-004` | well-formed balanced tags | PRD §7 | green |
| `TABLES-SPAN-LINES-001` | merged header detected as a colspan | PRD §7 | green |
| `TABLES-SPAN-LINES-002` | HTML emits `colspan` once (origin only) | PRD §7 | green |
| `TABLES-SPAN-ROW-001` | rowspan detected | PRD §7 | green |
| `TABLES-SPAN-ROW-002` | HTML emits `rowspan` once (origin only) | PRD §7 | green |
| `TABLES-SPAN-003` | markdown over a merged grid is still valid GFM | PRD §7 | green |
| `TABLES-SPAN-004` | `extract` origin holds text, continuation is `None` | PRD §7 | green |
| `TABLES-NONE-001` | empty page → no tables | PRD §7 | green |
| `TABLES-NONE-002` | prose (no grid) → no tables | PRD §7 | green |
| `TABLES-NONE-003` | a single stroke is not a grid | PRD §7 | green |
| `TABLES-PROP-001` | arbitrary content never panics | PRD §8.1 | green |

### M7 — optional content (`pdf_core::ocg` / `pdf_edit::ocg`) — `OCG-READ-*` / `OCG-ADD-*` / `OCG-TOGGLE-*` / `OCG-BIND-*`

Tests live in `crates/pdf-core/tests/ocg_unit.rs` (read) and
`crates/pdf-edit/tests/ocg_e2e.rs` (write round-trip).

| ID | feature | spec ref | status |
|---|---|---|---|
| `OCG-READ-COUNT` | `get_ocgs` counts the layers | PRD §7 | green |
| `OCG-READ-NAME` | reads each OCG `/Name` | PRD §7 | green |
| `OCG-READ-STATE` | reads the default ON/OFF state | PRD §7 | green |
| `OCG-READ-LOCKED` | reads the `/Locked` flag | PRD §7 | green |
| `OCG-READ-INTENT` | reads `/Intent` (defaults to View) | PRD §7 | green |
| `OCG-READ-UI-CONFIG` | `layer_ui_configs` rows (number/text/depth/type/on/locked) | PRD §7 | green |
| `OCG-READ-ORDER-LABEL` | `/Order` nesting-label rows | PRD §7 | green |
| `OCG-READ-BASESTATE-OFF` | `/BaseState /OFF` honored | PRD §7 | green |
| `OCG-NONE-EMPTY` | no `/OCProperties` → empty | PRD §7 | green |
| `OCG-NONE-MALFORMED` | malformed OCG data → empty, no panic | PRD §8.1 | green |
| `OCG-ADD-FRESH` | `add_ocg` → save → reopen → present in `get_ocgs` | PRD §7 | green |
| `OCG-ADD-OFF` | add with `on=false` lands in `/OFF` | PRD §7 | green |
| `OCG-ADD-MULTI` | adding several OCGs | PRD §7 | green |
| `OCG-ADD-INTENT` | add with a custom `/Intent` | PRD §7 | green |
| `OCG-ADD-CONFIG-LABEL` | add under a labelled `/Order` group | PRD §7 | green |
| `OCG-ADD-EXISTING` | add into a pre-existing `/OCProperties` | PRD §7 | green |
| `OCG-TOGGLE-OFF` | `set_layer(off=[…])` turns a layer OFF | PRD §7 | green |
| `OCG-TOGGLE-ON` | `set_layer(on=[…])` turns a layer ON | PRD §7 | green |
| `OCG-TOGGLE-BULK` | bulk on/off in one call | PRD §7 | green |
| `OCG-BIND-XOBJECT` | `set_oc` binds an XObject stream | PRD §7 | green |
| `OCG-BIND-DICT` | `set_oc` binds a dictionary object | PRD §7 | green |
| `OCG-NOOP` | `set_layer` on a non-layered doc is a no-op | PRD §7 | green |

### M7 — SVG export (`pdf_render::svg`) — `SVG-BASIC-*` / `SVG-EMPTY-*` / `SVG-ESCAPE-*` / `SVG-PROP-*`

Tests live in `crates/pdf-render/tests/svg.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `SVG-BASIC-001` | rect + glyph + image → `<svg>` root, viewBox, paths | PRD §7 | green |
| `SVG-BASIC-002` | `matrix` scales the viewport | PRD §7 | green |
| `SVG-BASIC-003` | stroke attributes emitted | PRD §7 | green |
| `SVG-BASIC-004` | even-odd fill rule | PRD §7 | green |
| `SVG-BASIC-005` | clip emits a `<clipPath>` | PRD §7 | green |
| `SVG-BASIC-006` | shading → gradient | PRD §7 | green |
| `SVG-EMPTY-001` | blank page → well-formed empty `<svg>` | PRD §7 | green |
| `SVG-EMPTY-002` | page with no `/Contents` | PRD §7 | green |
| `SVG-ESCAPE-001` | no unescaped XML specials | PRD §7 | green |
| `SVG-ESCAPE-002` | text-fallback path escaped | PRD §7 | green |
| `SVG-PROP-001` | arbitrary content → well-formed SVG | PRD §8.1 | green |
| `SVG-PROP-002` | `/Rotate` swaps the viewport | PRD §7 | green |

### M7 — `pdf-api` facade wiring — `TABLES-API-*` / `OCG-API-*` / `SVG-API-*`

Tests live in `crates/pdf-api/tests/m7_facade.rs`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `TABLES-API-001` | `page_find_tables` assembles textpage+words+device-drawings → grid + `extract` | PRD §9.1 | green |
| `TABLES-API-002` | facade `Table.to_markdown` / `to_html` shape | PRD §9.1 | green |
| `TABLES-API-003` | `strategy_from_str` maps lines/lines_strict/text (lenient) | PRD §9.1 | green |
| `OCG-API-001` | `Document` add → save → reopen → `get_ocgs` / `layer_ui_configs` round-trip | PRD §9.1 | green |
| `OCG-API-002` | `set_layer` / `set_layer_state` toggle reflected in `ocg_state` | PRD §9.1 | green |
| `SVG-API-001` | `page_get_svg_image` → well-formed SVG string | PRD §9.1 | green |

### M7 — Python `find_tables` / OCG / `get_svg_image` + `fitz` parity — `PYTABLE-*` / `PYOCG-*` / `PYSVG-*` / `PYFITZ-M7-*`

Tests live in `python/tests/test_m7.py`.

| ID | feature | spec ref | status |
|---|---|---|---|
| `PYTABLE-001` | `page.find_tables()` → `TableFinder` (`len`/`.tables`), `bbox` is a `Rect` | PRD §9.5 | green |
| `PYTABLE-002` | `Table.extract()` returns the cell-string grid | PRD §9.5 | green |
| `PYTABLE-003` | `Table.to_markdown()` pipe-delimited shape | PRD §9.5 | green |
| `PYTABLE-004` | `Table.to_html()` has `<table>`/`<td>` (oxide_pdf extra) | PRD §9.5 | green |
| `PYTABLE-005` | merged-header table → `to_html` carries `colspan` | PRD §9.5 | green |
| `PYTABLE-006` | `TableFinder` is iterable + indexable | PRD §9.5 | green |
| `PYTABLE-007` | `strategy="text"` returns a `TableFinder` (never raises) | PRD §9.5 | green |
| `PYOCG-001` | `add_ocg` → save → reopen → appears in `get_ocgs`/`layer_ui_configs` | PRD §9.5 | green |
| `PYOCG-002` | `set_layer(off=[xref])` reflected in `ocg_state`/`get_layer` | PRD §9.5 | green |
| `PYOCG-003` | `add_ocg(on=False)` lands OFF | PRD §9.5 | green |
| `PYSVG-001` | `page.get_svg_image()` → string starting `<?xml`/`<svg`, well-formed | PRD §9.5 | green |
| `PYSVG-002` | `get_svg_image(matrix=…)` accepted | PRD §9.5 | green |
| `PYFITZ-M7-001` | `fitz` `find_tables`/`findTables` parity; `fitz.TableFinder`/`fitz.Table` | PRD §9.5 | green |
| `PYFITZ-M7-002` | `fitz` `get_svg_image`/`getSVGimage` parity | PRD §9.5 | green |
| `PYFITZ-M7-003` | `fitz` `addOCG`/`getOCGs`/`layerUIConfigs`/`setLayer` aliases | PRD §9.5 | green |
| `PYFITZ-M7-004` | `pymupdf.open(...).find_tables().tables[0].to_markdown()` works | PRD §9.5 | green |
