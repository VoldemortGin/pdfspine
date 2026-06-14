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

## M1+ (placeholder)

Catalogs for `LEXER-*`, `XREF-*`, `OBJSTM-*`, `FLATE-*`, `REPAIR-*`, `CRYPT-*`,
`LIMITS-DEFAULT-*` (M1), `WORDS-*` / text formats (M2), etc. are enumerated at
the start of each milestone per PRD §10.1.1 before implementation begins.
