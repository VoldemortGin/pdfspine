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
