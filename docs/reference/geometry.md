# Geometry

The geometry value types — `Point`, `Rect`, `IRect`, `Matrix`, `Quad` — mirror
PyMuPDF 1.24.x arithmetic **exactly** as a documented contract. They behave as
sequences (`r[0]`, `tuple(r)`, unpacking), so code that reads `page.rect` works
unchanged.

PyMuPDF's coordinate space is top-left origin, y-down. Distances are in points
(1/72 inch) unless a unit is given.

## Point

`Point(x, y)` — a 2-D point.

```python
p = oxide_pdf.Point(72, 100)
p.x, p.y                        # components
abs(p)                          # vector length (alias: p.norm())
p.unit, p.abs_unit              # unit vectors
p + (10, 0)                     # add a point/sequence/scalar
p - q                           # subtract
p * 2                           # scalar scale
p * m                           # transform by a Matrix
p.distance_to(other, "mm")     # distance to a Point or Rect; units px/in/cm/mm
p.transform(m)                  # transform in place, returns self
```

## Rect

`Rect(x0, y0, x1, y1)` — an axis-aligned rectangle; `(x0, y0)` is top-left.
Accepts `Rect()`, a 4-sequence, two point-likes, or 4 numbers.

### Dimensions & predicates

| Member | Description |
|---|---|
| `x0, y0, x1, y1` | Corner coordinates. |
| `width`, `height` | Clamped to ≥ 0. |
| `abs(r)` | Area (0 if empty/infinite). |
| `norm()` | Euclidean norm of the 4-tuple. |
| `get_area(unit="px")` | Area in `px`/`in`/`cm`/`mm`. |
| `is_empty`, `is_valid`, `is_infinite` | Predicates. |

### Corners & conversion

`top_left`/`tl`, `top_right`/`tr`, `bottom_left`/`bl`, `bottom_right`/`br`,
`quad`, `round()` / `irect` (→ `IRect`).

### Operations (mutate self, return self)

`include_point(p)`, `include_rect(r)`, `intersect(r)`, `transform(m)`,
`normalize()`, `morph(p, m)` (→ `Quad`), `torect(r)` (→ `Matrix`).

### Queries & operators

`contains(x)` / `x in r`, `intersects(x)`; operators `|` (union), `&`
(intersection), `+`, `-`, `*` (transform), `/` (inverse transform) all return
**new** objects.

## IRect

`IRect(x0, y0, x1, y1)` — an integer rectangle. Same surface as `Rect` (corners,
predicates, operations), with integer components; operations defer to `Rect` then
re-round. `rect` converts to a float `Rect`; `round()` / `irect` return self.

## Matrix

`Matrix(...)` — a 2-D affine matrix `[a b c d e f]` mapping `(x, y)` to
`(a*x + c*y + e, b*x + d*y + f)`.

Constructors:

```python
oxide_pdf.Matrix()              # identity
oxide_pdf.Matrix(degree)        # anti-clockwise rotation by `degree`
oxide_pdf.Matrix(sx, sy)        # scaling
oxide_pdf.Matrix(a, b, c, d, e, f)
oxide_pdf.Matrix(seq6)          # from a 6-sequence
```

| Member | Description |
|---|---|
| `a, b, c, d, e, f` | Components. |
| `concat(one, two)` | `self = one * two`; returns self. |
| `invert(src=None)` | Invert into self; `0` ok / `1` if degenerate. |
| `prerotate(theta)` | Pre-rotate (returns self). |
| `prescale(sx, sy)` | Pre-scale (returns self). |
| `preshear(h, v)` | Pre-shear (returns self). |
| `pretranslate(tx, ty)` | Pre-translate (returns self). |
| `is_rectilinear` | Whether axis-aligned. |
| `norm()` / `abs(m)` | Frobenius norm. |
| `~m` | Inverse (new Matrix). |
| `*`, `/`, `+`, `-` | Products / sums (scalars or matrices). |

`oxide_pdf.Matrix` also has the read-only singleton `Identity`
(`IdentityMatrix`).

## Quad

`Quad(ul, ur, ll, lr)` — a quadrilateral. Accepts `Quad()`, a 4-sequence of
points, or 4 points.

| Member | Description |
|---|---|
| `ul, ur, ll, lr` | Corner `Point`s. |
| `rect` | Enclosing `Rect`. |
| `width`, `height` | Edge extents. |
| `abs(q)` | Area (0 if empty). |
| `is_empty`, `is_infinite`, `is_convex`, `is_rectangular` | Predicates. |
| `transform(m)` | Transform in place; returns self. |
| `q * m` | Transform (new Quad). |
| `morph(p, m)` | Morph about `p` by `m` (new Quad). |

## Module helpers

`paper_sizes()`, `paper_size(name)`, `paper_rect(name)` (e.g. `"a4"`,
`"letter-l"`); the singletons / factories `Identity`, `EMPTY_RECT()`,
`EMPTY_IRECT()`, `EMPTY_QUAD()`, `INFINITE_RECT()`, `INFINITE_IRECT()`,
`INFINITE_QUAD()`; and the `*_like` type aliases `point_like`, `rect_like`,
`matrix_like`, `quad_like`.
