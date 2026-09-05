//! `GLYPHGEO-*` — the per-glyph geometry contract (PRD §8.6.1).
//!
//! The interpreter publishes the full text rendering matrix `Trm`, the raw `Tm`
//! and CTM it was built from, and the untransformed glyph cell, so downstream
//! consumers never have to reverse-engineer a font size out of a bbox or repair
//! a rotated run themselves.
//!
//! The expected numbers for the pure-matrix cases (001/002/003/005/006/007) were
//! cross-checked against PyMuPDF 1.28.2's `rawdict` span `size`, which is
//! MuPDF's `fz_matrix_expansion` = `sqrt(|det|)` of the render matrix.

mod common;

use common::*;
use pdf_core::geom::{Matrix, Quad, Rect};
use pdf_text::model::WritingDir;
use pdf_text::serialize::{defaults, to_dict, to_json, to_text, to_xml};
use pdf_text::{
    rendered_font_size, textpage_from_glyphs, ContentInterpreter, DictBlock, DictSpan,
    InterpretResult, PositionedGlyph, TextPage,
};

const EPS: f64 = 1e-9;

/// A font where every WinAnsi code is 500/1000 wide, with explicit vertical
/// metrics so the cell is deterministic (ascent 0.8, descent −0.2).
fn font_w500() -> pdf_core::Object {
    let widths: Vec<i64> = (0..95).map(|_| 500).collect();
    winansi_type1_with_metrics("Helvetica", 32, &widths, 800, -200)
}

/// Runs `content` against [`font_w500`] as resource `F1`.
fn run(content: &[u8]) -> InterpretResult {
    run_with_font(font_w500(), content)
}

/// Asserts a matrix equals `(a, b, c, d, e, f)` component-wise.
#[track_caller]
fn assert_matrix(m: &Matrix, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
    approx(m.a, a, EPS);
    approx(m.b, b, EPS);
    approx(m.c, c, EPS);
    approx(m.d, d, EPS);
    approx(m.e, e, EPS);
    approx(m.f, f, EPS);
}

/// Asserts only the linear part `(a, b, c, d)` — the translation is the pen
/// position, asserted separately where it matters.
#[track_caller]
fn assert_linear(m: &Matrix, a: f64, b: f64, c: f64, d: f64) {
    approx(m.a, a, EPS);
    approx(m.b, b, EPS);
    approx(m.c, c, EPS);
    approx(m.d, d, EPS);
}

/// The two geometry invariants every emitted glyph must satisfy:
/// `(0,0)·render_matrix == origin` and `cell.quad()·render_matrix ⊃= bbox`.
#[track_caller]
fn assert_invariants(g: &PositionedGlyph) {
    let m = &g.render_matrix;
    approx(m.e, g.origin.x, 1e-9);
    approx(m.f, g.origin.y, 1e-9);
    let r = g.cell.quad().transform(m).rect().normalize();
    let b = g.bbox.normalize();
    approx(r.x0, b.x0, 1e-9);
    approx(r.y0, b.y0, 1e-9);
    approx(r.x1, b.x1, 1e-9);
    approx(r.y1, b.y1, 1e-9);
}

// === GLYPHGEO-001: the identity case =====================================

#[test]
fn glyphgeo_001_identity_trm() {
    // `Td` sets Tm = translate(100, 700); `Tf 12` puts the whole scale in params.
    let res = run(b"BT /F1 12 Tf 100 700 Td (A) Tj ET");
    assert_eq!(res.glyphs.len(), 1);
    let g = &res.glyphs[0];
    assert_matrix(&g.render_matrix, 12.0, 0.0, 0.0, 12.0, 100.0, 700.0);
    assert_matrix(&g.text_matrix, 1.0, 0.0, 0.0, 1.0, 100.0, 700.0);
    assert_matrix(&g.ctm, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    approx(rendered_font_size(&g.render_matrix), 12.0, EPS);
    approx(g.size, 12.0, EPS);
    // Cell = [0, descent .. advance, ascent] in the unit space params scales:
    // advance 500/1000, ascent 800/1000, descent −200/1000.
    approx(g.cell.x0, 0.0, EPS);
    approx(g.cell.y0, -0.2, EPS);
    approx(g.cell.x1, 0.5, EPS);
    approx(g.cell.y1, 0.8, EPS);
    assert_invariants(g);
}

// === GLYPHGEO-002: scale in `Tm`, not in `Tf` ============================

#[test]
fn glyphgeo_002_scale_in_text_matrix() {
    // `Tf 1` + `Tm 12 0 0 12`: fitz reports size 12; our `size` stays the
    // declared operand 1, which is exactly why `rendered_font_size` exists.
    let res = run(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj ET");
    assert_eq!(res.glyphs.len(), 1);
    let g = &res.glyphs[0];
    assert_linear(&g.render_matrix, 12.0, 0.0, 0.0, 12.0);
    approx(rendered_font_size(&g.render_matrix), 12.0, EPS);
    approx(g.size, 1.0, EPS); // declared `Tf` operand — deliberately unchanged
    assert_matrix(&g.text_matrix, 12.0, 0.0, 0.0, 12.0, 100.0, 700.0);
    assert_invariants(g);
}

// === GLYPHGEO-003: pure rotation =========================================

#[test]
fn glyphgeo_003_rotation_preserves_rendered_size() {
    // `Tm 0 12 -12 0` is a 90° rotation at scale 12: |det| = 144 → size 12.
    let res = run(b"BT /F1 1 Tf 0 12 -12 0 100 700 Tm (A) Tj ET");
    let g = &res.glyphs[0];
    assert_linear(&g.render_matrix, 0.0, 12.0, -12.0, 0.0);
    approx(rendered_font_size(&g.render_matrix), 12.0, EPS);
    // The advance direction turned with it: the text x-axis is now +y.
    approx(g.advance_dir.0, 0.0, 1e-12);
    approx(g.advance_dir.1, 1.0, 1e-12);
    assert_invariants(g);
}

// === GLYPHGEO-004: skew → a real parallelogram ===========================

#[test]
fn glyphgeo_004_skew_yields_parallelogram_quad() {
    // `Tm 12 0 6 12`: |det| = |12·12 − 0·6| = 144 → rendered size 12, but the
    // glyph quad is sheared, so the axis-aligned bbox over-covers it.
    let res = run(b"BT /F1 1 Tf 12 0 6 12 100 700 Tm (A) Tj ET");
    let g = &res.glyphs[0];
    assert_linear(&g.render_matrix, 12.0, 0.0, 6.0, 12.0);
    approx(rendered_font_size(&g.render_matrix), 12.0, EPS);

    let q: Quad = g.cell.quad().transform(&g.render_matrix);
    // Opposite edges are equal vectors — a parallelogram, not four loose points.
    approx(q.ur.x - q.ul.x, q.lr.x - q.ll.x, 1e-9);
    approx(q.ur.y - q.ul.y, q.lr.y - q.ll.y, 1e-9);
    approx(q.ll.x - q.ul.x, q.lr.x - q.ur.x, 1e-9);
    approx(q.ll.y - q.ul.y, q.lr.y - q.ur.y, 1e-9);
    // ...and it is NOT axis-aligned: the left edge leans by the shear.
    assert!(
        (q.ll.x - q.ul.x).abs() > 1.0,
        "skewed cell must not be axis-aligned; got ul={:?} ll={:?}",
        q.ul,
        q.ll
    );
    assert_invariants(g);
}

// === GLYPHGEO-005: horizontal scaling `Tz` ===============================

#[test]
fn glyphgeo_005_tz_halves_the_x_axis() {
    // `Tf 12` + `50 Tz`: params = [12·0.5, 0, 0, 12] → |det| = 72.
    let res = run(b"BT /F1 12 Tf 50 Tz 100 700 Td (A) Tj ET");
    let g = &res.glyphs[0];
    assert_linear(&g.render_matrix, 6.0, 0.0, 0.0, 12.0);
    approx(rendered_font_size(&g.render_matrix), 72.0_f64.sqrt(), 1e-9);
    approx(
        rendered_font_size(&g.render_matrix),
        8.485_281_374_238_57,
        1e-9,
    );
    approx(g.size, 12.0, EPS); // `Tz` never touches the declared operand
    assert_invariants(g);
}

// === GLYPHGEO-006: anisotropic scaling ===================================

#[test]
fn glyphgeo_006_anisotropic_gives_geometric_mean() {
    // `Tm 20 0 0 10`: |det| = 200 → the geometric mean of the two axes.
    let res = run(b"BT /F1 1 Tf 20 0 0 10 100 700 Tm (A) Tj ET");
    let g = &res.glyphs[0];
    assert_linear(&g.render_matrix, 20.0, 0.0, 0.0, 10.0);
    approx(rendered_font_size(&g.render_matrix), 200.0_f64.sqrt(), 1e-9);
    approx(
        rendered_font_size(&g.render_matrix),
        14.142_135_623_730_951,
        1e-9,
    );
    assert_invariants(g);
}

// === GLYPHGEO-007: scale in the CTM ======================================

#[test]
fn glyphgeo_007_ctm_scale_doubles_rendered_size() {
    // `cm 2` before `BT`: the CTM carries the scale, `Tf 12` the rest → 24.
    let res = run(b"2 0 0 2 0 0 cm BT /F1 12 Tf 50 350 Td (A) Tj ET");
    let g = &res.glyphs[0];
    approx(rendered_font_size(&g.render_matrix), 24.0, EPS);
    assert_matrix(&g.ctm, 2.0, 0.0, 0.0, 2.0, 0.0, 0.0);
    // `Tm` is a pure translation — the scale is not folded back into it.
    assert_matrix(&g.text_matrix, 1.0, 0.0, 0.0, 1.0, 50.0, 350.0);
    // ...and the composed Trm lands the pen at (100, 700) in user space.
    assert_matrix(&g.render_matrix, 24.0, 0.0, 0.0, 24.0, 100.0, 700.0);
    approx(g.size, 12.0, EPS);
    assert_invariants(g);
}

// === GLYPHGEO-008: vertical writing ======================================

#[test]
fn glyphgeo_008_vertical_writing_cell_and_quad() {
    // Identity-V, /DW 1000 (w0 = 1.0), /DW2 [880 −1000] → vx = w0/2 = 0.5,
    // vy = 0.88. The cell spans x ∈ [−0.5, 0.5], y ∈ [desc − 0.88, asc − 0.88].
    let tounicode: &[u8] = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
        1 begincodespacerange <0000> <FFFF> endcodespacerange \
        1 beginbfchar <0001> <4E2D> endbfchar endcmap end end";
    let (doc, page) = build_vertical_doc(tounicode, b"BT /F1 24 Tf 300 700 Td <0001> Tj ET");
    let res = ContentInterpreter::new(&doc).run_page(&page);
    assert_eq!(res.glyphs.len(), 1);
    let g = &res.glyphs[0];
    assert_eq!(g.writing_dir, WritingDir::Vertical);
    assert_matrix(&g.render_matrix, 24.0, 0.0, 0.0, 24.0, 300.0, 700.0);
    approx(rendered_font_size(&g.render_matrix), 24.0, EPS);
    // The cell is displaced by −v, which `bbox` alone cannot tell you.
    approx(g.cell.x0, -0.5, EPS);
    approx(g.cell.x1, 0.5, EPS);
    // Latin-text fallback metrics: ascent 0.8, descent −0.2 (no /FontDescriptor).
    approx(g.cell.y0, -0.2 - 0.88, EPS);
    approx(g.cell.y1, 0.8 - 0.88, EPS);
    assert_invariants(g);
    // The cell's left edge lands at pen.x − 0.5·24 = 288 (matches INTERP-021).
    approx(g.bbox.normalize().x0, 288.0, 1e-6);
}

// === GLYPHGEO-009: the invariants hold across every fixture ==============

#[test]
fn glyphgeo_009_invariants_hold_everywhere() {
    let contents: &[&[u8]] = &[
        b"BT /F1 12 Tf 100 700 Td (Hello) Tj ET",
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hello) Tj ET",
        b"BT /F1 1 Tf 0 12 -12 0 100 700 Tm (Hello) Tj ET",
        b"BT /F1 1 Tf 12 0 6 12 100 700 Tm (Hello) Tj ET",
        b"BT /F1 12 Tf 50 Tz 100 700 Td (Hello) Tj ET",
        b"BT /F1 1 Tf 20 0 0 10 100 700 Tm (Hello) Tj ET",
        b"2 0 0 2 0 0 cm BT /F1 12 Tf 50 350 Td (Hello) Tj ET",
        b"BT /F1 12 Tf 3 Tc 2 Tw 5 Ts 100 700 Td (a b c) Tj ET",
        b"BT /F1 12 Tf 100 700 Td [(A) -500 (B)] TJ ET",
        // Degenerate: a singular Tm must not panic and must report size 0.
        b"BT /F1 12 Tf 0 0 0 0 100 700 Tm (A) Tj ET",
    ];
    for content in contents {
        let res = run(content);
        for g in &res.glyphs {
            assert_invariants(g);
            let s = rendered_font_size(&g.render_matrix);
            assert!(
                s.is_finite() && s >= 0.0,
                "rendered size must be finite ≥ 0"
            );
            // Every published matrix component stays finite, even under the
            // singular-`Tm` fixture (PRD §8.6.2: never emit NaN/Inf).
            for m in [&g.render_matrix, &g.text_matrix, &g.ctm] {
                for v in [m.a, m.b, m.c, m.d, m.e, m.f] {
                    assert!(v.is_finite(), "matrix component must stay finite");
                }
            }
        }
    }
}

// =========================================================================
// GLYPHGEO-010..016 — publishing the geometry through the serializers
// (`dict` / `rawdict` / `json` / `rawjson` / `xml`).
// =========================================================================

/// The page transform of the 612×792, unrotated test page: PDF user space
/// (y up) → PyMuPDF device space (y down).
fn page_p() -> Matrix {
    Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, 792.0)
}

/// Lays `content` out into a device-space [`TextPage`] on the standard test page.
fn page_of(content: &[u8]) -> TextPage {
    let res = run(content);
    textpage_from_glyphs(&res.glyphs, &[], Rect::new(0.0, 0.0, 612.0, 792.0), 0)
}

/// The first span of the first text block, as `dict` (`raw == false`) or
/// `rawdict` (`raw == true`).
fn first_span(content: &[u8], raw: bool) -> DictSpan {
    let tp = page_of(content);
    let flags = if raw {
        defaults::RAWDICT
    } else {
        defaults::DICT
    };
    let d = to_dict(&tp, raw, flags);
    match d.blocks.first().expect("at least one block") {
        DictBlock::Text(b) => b.lines[0].spans[0].clone(),
        DictBlock::Image(_) => panic!("expected a text block"),
    }
}

/// Asserts a published 6-tuple equals `(a, b, c, d, e, f)`.
#[track_caller]
fn assert_tuple6(t: (f64, f64, f64, f64, f64, f64), want: (f64, f64, f64, f64, f64, f64)) {
    approx(t.0, want.0, 1e-9);
    approx(t.1, want.1, 1e-9);
    approx(t.2, want.2, 1e-9);
    approx(t.3, want.3, 1e-9);
    approx(t.4, want.4, 1e-9);
    approx(t.5, want.5, 1e-9);
}

/// The axis-aligned envelope of a published 8-tuple quad.
fn quad_rect(q: (f64, f64, f64, f64, f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let xs = [q.0, q.2, q.4, q.6];
    let ys = [q.1, q.3, q.5, q.7];
    (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

// === GLYPHGEO-010: the dict span publishes the full geometry ==============

#[test]
fn glyphgeo_010_dict_span_geometry_keys() {
    // `Tf 1` + `Tm 12 0 0 12 100 700`: the scale lives entirely in `Tm`, so the
    // declared and rendered sizes disagree — the whole point of the new keys.
    let span = first_span(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET", false);

    // Structured size is rendered; the remaining existing keys keep their meaning.
    approx(span.size, 12.0, EPS);
    approx(span.origin.0, 100.0, 1e-9);
    approx(span.origin.1, 92.0, 1e-9); // 792 − 700
    assert_eq!(span.text, "Hi");

    // The original `Tf` remains explicit; `size` and `rendered_size` both use
    // `sqrt(|det|)` of the render matrix (PyMuPDF's rawdict size semantics).
    approx(span.declared_size, 1.0, EPS);
    approx(span.rendered_size, 12.0, EPS);

    // Device-space render matrix: `Trm · page_transform`, y flipped.
    assert_tuple6(span.matrix, (12.0, 0.0, 0.0, -12.0, 100.0, 92.0));
    // ...and the raw user-space operands it was composed from.
    assert_tuple6(span.text_matrix, (12.0, 0.0, 0.0, 12.0, 100.0, 700.0));
    assert_tuple6(span.ctm, (1.0, 0.0, 0.0, 1.0, 0.0, 0.0));

    // Invariant 1: `(0,0)·matrix == origin`.
    approx(span.matrix.4, span.origin.0, 1e-9);
    approx(span.matrix.5, span.origin.1, 1e-9);
    // Invariant 2: the quad's bounding rect is the bbox.
    let r = quad_rect(span.quad);
    approx(r.0, span.bbox.0, 1e-9);
    approx(r.1, span.bbox.1, 1e-9);
    approx(r.2, span.bbox.2, 1e-9);
    approx(r.3, span.bbox.3, 1e-9);

    approx(span.dir.0, 1.0, 1e-12);
    approx(span.dir.1, 0.0, 1e-12);
    assert_eq!(span.seq, 0, "the first painted glyph starts the span");
    // dict mode still carries no chars.
    assert!(span.chars.is_empty());
}

// === GLYPHGEO-011: the rawdict char publishes the per-glyph geometry ======

#[test]
fn glyphgeo_011_rawdict_char_geometry_keys() {
    let span = first_span(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET", true);
    assert!(span.text.is_empty(), "rawdict moves the text into chars");
    assert_eq!(span.chars.len(), 2);

    let c0 = &span.chars[0];
    assert_eq!(c0.c, "H");
    // Same basis as `origin`/`bbox`, and the same matrix as the span's first glyph.
    assert_tuple6(c0.matrix, (12.0, 0.0, 0.0, -12.0, 100.0, 92.0));
    approx(c0.rendered_size, 12.0, EPS);
    assert_eq!(c0.seq, 0);
    assert!(!c0.synthetic);

    // The cell is `[0, −0.2 .. 0.5, 0.8]` (advance 500/1000, ascent 800,
    // descent −200); through the device matrix that is a y-flipped rectangle
    // whose `ul` is the *ascender* corner — the visual upper-left.
    let q = c0.quad;
    approx(q.0, 100.0, 1e-9); // ul.x
    approx(q.1, 82.4, 1e-9); // ul.y  = 92 − 12·0.8
    approx(q.2, 106.0, 1e-9); // ur.x = 100 + 12·0.5
    approx(q.3, 82.4, 1e-9);
    approx(q.4, 100.0, 1e-9); // ll.x
    approx(q.5, 94.4, 1e-9); // ll.y  = 92 + 12·0.2
    approx(q.6, 106.0, 1e-9);
    approx(q.7, 94.4, 1e-9);
    assert!(q.1 < q.5, "ul must be above ll in device space (y down)");

    // Invariants, per char: origin from the matrix, bbox from the quad.
    for c in &span.chars {
        approx(c.matrix.4, c.origin.0, 1e-9);
        approx(c.matrix.5, c.origin.1, 1e-9);
        let r = quad_rect(c.quad);
        approx(r.0, c.bbox.0, 1e-9);
        approx(r.1, c.bbox.1, 1e-9);
        approx(r.2, c.bbox.2, 1e-9);
        approx(r.3, c.bbox.3, 1e-9);
        approx(
            c.rendered_size,
            rendered_font_size(&Matrix::new(
                c.matrix.0, c.matrix.1, c.matrix.2, c.matrix.3, c.matrix.4, c.matrix.5,
            )),
            1e-12,
        );
    }
    assert_eq!(span.chars[1].seq, 1, "seq is the painting index");
}

// === GLYPHGEO-012: the composition invariant =============================

#[test]
fn glyphgeo_012_matrix_is_params_tm_ctm_page() {
    // `cm 2` puts the scale in the CTM, `Tf 12` in params, and `Td` leaves `Tm`
    // a pure translation — so all three factors are non-trivial and distinct.
    let span = first_span(b"2 0 0 2 0 0 cm BT /F1 12 Tf 50 350 Td (A) Tj ET", false);
    approx(span.declared_size, 12.0, EPS);
    approx(span.rendered_size, 24.0, EPS);

    // Invariant 3: matrix = params · text_matrix · ctm · page_transform, with
    // params = [Tfs·Th, 0, 0, Tfs, 0, Trise] (Th = 1, Trise = 0 here).
    let params = Matrix::new(span.declared_size, 0.0, 0.0, span.declared_size, 0.0, 0.0);
    let tm = Matrix::new(
        span.text_matrix.0,
        span.text_matrix.1,
        span.text_matrix.2,
        span.text_matrix.3,
        span.text_matrix.4,
        span.text_matrix.5,
    );
    let ctm = Matrix::new(
        span.ctm.0, span.ctm.1, span.ctm.2, span.ctm.3, span.ctm.4, span.ctm.5,
    );
    let want = Matrix::concat(
        &Matrix::concat(&Matrix::concat(&params, &tm), &ctm),
        &page_p(),
    );
    assert_tuple6(
        span.matrix,
        (want.a, want.b, want.c, want.d, want.e, want.f),
    );
    assert_tuple6(span.ctm, (2.0, 0.0, 0.0, 2.0, 0.0, 0.0));
    assert_tuple6(span.text_matrix, (1.0, 0.0, 0.0, 1.0, 50.0, 350.0));
}

// === GLYPHGEO-013: json / rawjson carry the same keys ====================

#[test]
fn glyphgeo_013_json_and_rawjson_geometry_keys() {
    let tp = page_of(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET");

    let j = to_json(&tp, false, defaults::JSON);
    for key in [
        "\"declared_size\":1",
        "\"rendered_size\":12",
        "\"matrix\":[12,0,0,-12,100,92]",
        "\"text_matrix\":[12,0,0,12,100,700]",
        "\"ctm\":[1,0,0,1,0,0]",
        "\"dir\":[1,0]",
        "\"seq\":0",
        "\"number\":0",
    ] {
        assert!(j.contains(key), "json missing {key}:\n{j}");
    }
    // dict-mode json still carries `text`, not `chars`.
    assert!(j.contains("\"text\":\"Hi\"") && !j.contains("\"chars\""));

    let rj = to_json(&tp, true, defaults::RAWJSON);
    for key in [
        "\"quad\":[100,82.4,106,82.4,100,94.4,106,94.4]",
        "\"rendered_size\":12",
        "\"synthetic\":false",
        "\"c\":\"H\"",
    ] {
        assert!(rj.contains(key), "rawjson missing {key}:\n{rj}");
    }
    assert!(rj.contains("\"chars\"") && !rj.contains("\"text\":\"Hi\""));
}

// === GLYPHGEO-014: seq is monotone, number matches the text order ========

#[test]
fn glyphgeo_014_seq_monotone_and_line_number_matches_text() {
    // Three lines painted top-to-bottom, so painting order and reading order
    // agree and `number` is directly checkable against `get_text("text")`.
    let content: &[u8] = b"BT /F1 12 Tf 100 700 Td (Alpha) Tj 0 -20 Td (Beta) Tj \
        0 -20 Td (Gamma) Tj ET";
    let tp = page_of(content);
    let d = to_dict(&tp, true, defaults::RAWDICT);

    let mut prev_seq = 0usize;
    let mut numbered: Vec<(usize, String)> = Vec::new();
    for block in &d.blocks {
        let DictBlock::Text(b) = block else { continue };
        for line in &b.lines {
            assert!(line.seq >= b.seq, "a block's seq is its smallest line seq");
            let mut text = String::new();
            for span in &line.spans {
                assert!(span.seq >= line.seq);
                for ch in &span.chars {
                    assert!(
                        ch.seq >= prev_seq,
                        "char seq must not go backwards: {} < {prev_seq}",
                        ch.seq
                    );
                    prev_seq = ch.seq;
                    text.push_str(&ch.c);
                }
            }
            numbered.push((line.number, text));
        }
    }

    // `number` is a dense 0..n range over the page's lines...
    let mut nums: Vec<usize> = numbered.iter().map(|(n, _)| *n).collect();
    nums.sort_unstable();
    assert_eq!(nums, (0..numbered.len()).collect::<Vec<_>>());

    // ...and ordering the lines by it reproduces `get_text("text")` line for line.
    numbered.sort_by_key(|(n, _)| *n);
    let want: Vec<String> = to_text(&tp, defaults::TEXT)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let got: Vec<String> = numbered.into_iter().map(|(_, t)| t).collect();
    assert_eq!(got, want);
}

// === GLYPHGEO-015: the xml `<char quad>` is the true parallelogram =======

#[test]
fn glyphgeo_015_xml_char_quad_is_a_parallelogram() {
    // `Tm 12 0 6 12` shears the cell: the four corners must stop being the
    // bbox corners and become a real (non-axis-aligned) parallelogram, which is
    // what PyMuPDF 1.28.2 emits for the same content.
    let tp = page_of(b"BT /F1 1 Tf 12 0 6 12 100 700 Tm (A) Tj ET");
    let xml = to_xml(&tp, defaults::XML);
    let at = xml.find("quad=\"").expect("a <char quad=…>") + 6;
    let end = at + xml[at..].find('"').expect("closing quote");
    let v: Vec<f64> = xml[at..end]
        .split_whitespace()
        .map(|t| t.parse::<f64>().expect("a number"))
        .collect();
    assert_eq!(v.len(), 8, "quad is 8 coordinates: {v:?}");
    let (ul, ur, ll, lr) = ((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), (v[6], v[7]));
    // Opposite edges are equal vectors → a parallelogram.
    approx(ur.0 - ul.0, lr.0 - ll.0, 1e-9);
    approx(ur.1 - ul.1, lr.1 - ll.1, 1e-9);
    approx(ll.0 - ul.0, lr.0 - ur.0, 1e-9);
    approx(ll.1 - ul.1, lr.1 - ur.1, 1e-9);
    // ...and it is NOT the axis-aligned bbox: the left edge leans by the shear
    // (Δx = 6·(ascent + |descent|) = 6·1.0 = 6).
    approx(ll.0 - ul.0, -6.0, 1e-9);
    assert!(ul.1 < ll.1, "ul is above ll in device space");
}

// === fixtures ============================================================

/// An Identity-V Type0 doc (`/DW 1000`, `/DW2 [880 −1000]`) running `content`.
fn build_vertical_doc(
    tounicode: &[u8],
    content: &[u8],
) -> (pdf_core::DocumentStore, pdf_core::Dict) {
    use pdf_core::Object;
    let mut pd = PageDoc::new();
    let tu_num = pd.add(raw_stream([], tounicode));
    let cidfont = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("Sub+Font")),
        (
            "CIDSystemInfo",
            Object::Dictionary(dict([
                (
                    "Registry",
                    Object::String(pdf_core::PdfString::literal("Adobe")),
                ),
                (
                    "Ordering",
                    Object::String(pdf_core::PdfString::literal("Identity")),
                ),
                ("Supplement", Object::Integer(0)),
            ])),
        ),
        ("DW", Object::Integer(1000)),
        (
            "DW2",
            Object::Array(vec![Object::Integer(880), Object::Integer(-1000)]),
        ),
    ]));
    let cid_num = pd.add(cidfont);
    let type0 = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("BaseFont", name_obj("Sub+Font")),
        ("Encoding", name_obj("Identity-V")),
        ("DescendantFonts", Object::Array(vec![rref(cid_num, 0)])),
        ("ToUnicode", rref(tu_num, 0)),
    ]));
    pd.font("F1", type0).content(content).open()
}
