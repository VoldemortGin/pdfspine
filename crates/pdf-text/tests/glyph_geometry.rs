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
use pdf_core::geom::{Matrix, Quad};
use pdf_text::model::WritingDir;
use pdf_text::{rendered_font_size, ContentInterpreter, InterpretResult, PositionedGlyph};

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
