# `pdf-render` architecture (M6 rendering, PRD §8.11)

Page → `pdf_image::Pixmap` rasterizer. **All PDF intelligence is first-party**
(content interpretation, CTM, colorspaces, glyph-outline extraction); the
[`tiny-skia`](https://crates.io/crates/tiny-skia) dependency is used **only** as
the leaf anti-aliased fill/stroke/clip/blend primitive that paints already-built
paths into a pixel buffer.

`#![forbid(unsafe_code)]` applies to all first-party code. The rasterizer
dependency may use `unsafe` internally (SIMD) — that is a dependency, not
first-party code.

## Render stack (chosen for M6)

| Concern         | Choice                                   | License      | Why |
|-----------------|------------------------------------------|--------------|-----|
| 2D rasterizer   | `tiny-skia` 0.11 (`std`,`simd`; no `png-format`) | BSD-3-Clause | Mature pure-Rust Skia raster port: AA fill/stroke/clip/blend/gradients. Dropping `png-format` removes the `png`→`flate2`/`miniz_oxide` subtree (we encode PNG via `pdf_image::Pixmap`). Picked over `raqote` (less maintained) / `zeno` (rasterizer-only, no pixmap/clip/blend). |
| Glyph outlines  | reuse `ttf-parser` `OutlineBuilder`      | MIT/Apache-2.0 | Outlines feed straight into a tiny-skia `PathBuilder`; no extra glyph-raster crate (`swash`/`fontdue`/`ab_glyph`) needed — minimizes deps per policy. |
| Color (ICC)     | deferred (`moxcms` not added)            | —            | Naive CMYK→RGB is acceptable for the scaffold + first pass (PRD §8.11). Add `moxcms` (Apache-2.0) later if/when ICC profiles are needed. |

The graph added is BSD/MIT/Apache/Zlib leaves only (`tiny-skia-path`,
`arrayref`, `arrayvec`, `bytemuck` [already shared with `image`], `cfg-if`,
`log`, `strict-num`) — `cargo deny check licenses` clean.

## Crate dependencies (DAG, PRD §9.1)

`pdf-core` (objects/geom + Error), `pdf-fonts` (glyph outlines/metrics),
`pdf-text` (content interpreter → positioned glyphs + draw paths + image
placements), `pdf-image` (`Pixmap` + image codecs), `thiserror`, `tiny-skia`.

## Module ownership (frozen — parallel M6a/b/c/d fill disjoint files)

| Module        | Owner | Responsibility |
|---------------|-------|----------------|
| `canvas.rs`   | M6a   | `Canvas` raster target wrapping the tiny-skia pixmap; blank ctor; `into_pixmap`. |
| `vector.rs`   | M6a   | `fill_path` / `stroke_path` / `set_clip` for geometry paths + `Paint`/`StrokeStyle`. |
| `text.rs`     | M6b   | `draw_glyph` / `draw_text_run` (ttf-parser outlines → filled paths). |
| `image.rs`    | M6c   | `draw_image` (composite a decoded `Pixmap` under a CTM; +shading/pattern). |
| `render.rs`   | M6d   | `render_page` entry + `RenderOptions`; orchestrates the above; DisplayList; PyO3/fitz wiring. |
| `error.rs`    | shared| `Error` / `Result` + `From<pdf_core::Error>` / `From<pdf_image::Error>`. |
| `lib.rs`      | shared| module tree + re-exports. |

Implementers own their feature tests + catalog rows (not added by the scaffold).

## Frozen stub signatures (the contract)

```rust
// error.rs
pub enum Error { Unsupported(&'static str), InvalidArgument(&'static str),
                 LimitExceeded(&'static str), Core(pdf_core::Error), Image(pdf_image::Error) }
impl Error { pub fn kind(&self) -> &'static str; }
pub type Result<T> = std::result::Result<T, Error>;

// canvas.rs
pub struct Canvas { /* tiny-skia pixmap + base_transform + out colorspace/alpha */ }
impl Canvas {
    pub fn blank(width: u32, height: u32, base_transform: Matrix,
                 out_colorspace: Colorspace, out_alpha: bool) -> Result<Self>;
    pub fn width(&self) -> u32;
    pub fn height(&self) -> u32;
    pub fn base_transform(&self) -> Matrix;
    pub(crate) fn pixmap_mut(&mut self) -> &mut tiny_skia::Pixmap;
    pub(crate) fn pixmap(&self) -> &tiny_skia::Pixmap;
    pub fn into_pixmap(self) -> Result<Pixmap>;
}

// vector.rs
pub struct Paint { pub rgba: [u8; 4] }      impl Paint { pub fn from_rgb(rgb: u32) -> Self; }
pub struct StrokeStyle { pub width: f32 }   // grows: joins/caps/dashes
pub fn fill_path(canvas: &mut Canvas, path: &DrawPath, paint: Paint, ctm: Matrix, even_odd: bool) -> Result<()>;
pub fn stroke_path(canvas: &mut Canvas, path: &DrawPath, paint: Paint, style: &StrokeStyle, ctm: Matrix) -> Result<()>;
pub fn set_clip(canvas: &mut Canvas, items: &[PathItem], ctm: Matrix, even_odd: bool) -> Result<()>;

// text.rs
pub fn draw_glyph(canvas: &mut Canvas, glyph: &PositionedGlyph, paint: Paint, ctm: Matrix) -> Result<()>;
pub fn draw_text_run(canvas: &mut Canvas, glyphs: &[PositionedGlyph], paint: Paint, ctm: Matrix) -> Result<()>;

// image.rs
pub fn draw_image(canvas: &mut Canvas, image: &Pixmap, ctm: Matrix, alpha: u8) -> Result<()>;

// render.rs
pub struct RenderOptions { pub matrix: Matrix, pub dpi: Option<u32>,
                           pub colorspace: Colorspace, pub alpha: bool, pub clip: Option<IRect> }
impl Default for RenderOptions { /* identity matrix, RGB, no alpha, no clip */ }
pub fn render_page(doc: &DocumentStore, page: &Page, opts: &RenderOptions) -> Result<Pixmap>;
```

All stubs return `Err(Error::Unsupported(<name>))` (or `InvalidArgument`/
`LimitExceeded` for the geometry guards in `Canvas::blank`). No
`todo!()`/`unimplemented!()`/`panic!` — arbitrary input never panics (PRD §8.1).
The `#[non_exhaustive]` on `Error` lets implementers add variants without a
breaking change.
