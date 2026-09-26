//! `SPANVIS-*` — a span is one *visual* run, not just one text state.
//!
//! `build_line` used to open a new span only on a change of font, declared size,
//! colour or style flags. None of those describe how a glyph is actually
//! painted, so a producer that emits `Tf 1` and puts the scale in `Tm` — the
//! LaTeX/PMC idiom — handed downstream a single span covering runs at different
//! scales, rotations, shears or baselines, published with the matrix of its
//! first glyph alone. Once the line is sorted along its reading axis those runs
//! also *interleave*, so the span text came out shuffled between them.
//!
//! These tests pin both directions: the runs that must now split, and the
//! ordinary text that must still not.

mod common;

use common::*;
use pdf_core::geom::Rect;
use pdf_text::serialize::{defaults, to_dict};
use pdf_text::{textpage_from_glyphs, DictBlock, DictSpan, InterpretResult, TextPage};

const EPS: f64 = 1e-9;

/// A font where every WinAnsi code is 500/1000 wide, with explicit vertical
/// metrics so the cell is deterministic (ascent 0.8, descent −0.2). At 12 pt
/// every glyph advances exactly 6 pt, which is what the fixture x-positions
/// below are computed from.
fn font_w500() -> pdf_core::Object {
    let widths: Vec<i64> = (0..95).map(|_| 500).collect();
    winansi_type1_with_metrics("Helvetica", 32, &widths, 800, -200)
}

fn run(content: &[u8]) -> InterpretResult {
    run_with_font(font_w500(), content)
}

/// Lays `content` out into a device-space [`TextPage`] on a 612×792 page.
fn page_of(content: &[u8]) -> TextPage {
    let res = run(content);
    textpage_from_glyphs(&res.glyphs, &[], Rect::new(0.0, 0.0, 612.0, 792.0), 0)
}

/// The spans of the first line of the first text block, as `rawdict` (so every
/// span carries its chars).
fn line_spans(content: &[u8]) -> Vec<DictSpan> {
    let tp = page_of(content);
    let d = to_dict(&tp, true, defaults::RAWDICT);
    let b = match d.blocks.first().expect("at least one block") {
        DictBlock::Text(b) => b,
        DictBlock::Image(_) => panic!("expected a text block"),
    };
    assert_eq!(b.lines.len(), 1, "fixture is meant to lay out as one line");
    b.lines[0].spans.clone()
}

/// A span's text, taken from its chars (rawdict moves the text there).
fn span_text(s: &DictSpan) -> String {
    s.chars.iter().map(|c| c.c.as_str()).collect()
}

/// The `(text, rendered_size)` of every span on the line.
fn shape(content: &[u8]) -> Vec<(String, f64)> {
    line_spans(content)
        .iter()
        .map(|s| (span_text(s), s.rendered_size))
        .collect()
}

#[track_caller]
fn assert_shape(got: &[(String, f64)], want: &[(&str, f64)]) {
    let got_text: Vec<&str> = got.iter().map(|(t, _)| t.as_str()).collect();
    let want_text: Vec<&str> = want.iter().map(|(t, _)| *t).collect();
    assert_eq!(got_text, want_text, "span texts (got {got:?})");
    for (g, w) in got.iter().zip(want) {
        approx(g.1, w.1, 1e-6);
    }
}

// === the runs that must split ===========================================

// === SPANVIS-001: two `Tm` scales, one declared size =====================

#[test]
fn spanvis_001_scale_change_splits_the_span() {
    // `Tf 1` for both runs, so the declared size the old rule keyed on is 1 for
    // every glyph; only the render matrix tells them apart. "AB" is 12 pt
    // (6 pt/glyph, x 100→112); "CD" starts exactly where it ends, at 24 pt.
    let got = shape(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (AB) Tj 24 0 0 24 112 700 Tm (CD) Tj ET");
    assert_shape(&got, &[("AB", 12.0), ("CD", 24.0)]);
}

// === SPANVIS-002: a rotation too small to break the line =================

#[test]
fn spanvis_002_small_rotation_within_calibrated_tolerance_stays_one_span() {
    // 2°: the writing directions still match (dot 0.99939 > 0.996, so the line
    // stays one line and criterion 2 stays silent), but the linear parts differ
    // by sin 2° = 0.0349 of the size, within the calibrated 5% tolerance.
    let got = shape(
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (AB) Tj \
          11.99269 0.41879 -0.41879 11.99269 112 700 Tm (CD) Tj ET",
    );
    assert_shape(&got, &[("ABCD", 12.0)]);
}

// === SPANVIS-003: shear =================================================

#[test]
fn spanvis_003_shear_splits_the_span() {
    // `Tm 12 0 6 12` has the same determinant — and so the same rendered size
    // and the same writing direction — as `Tm 12 0 0 12`. Only comparing the
    // full linear part catches it.
    let got = shape(b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (AB) Tj 12 0 6 12 112 700 Tm (CD) Tj ET");
    assert_shape(&got, &[("AB", 12.0), ("CD", 12.0)]);
}

// === SPANVIS-004: a baseline shift that sets no flag =====================

#[test]
fn spanvis_004_small_baseline_shift_within_calibrated_tolerance_stays_one_span() {
    // Down 1 pt at 12 pt = 0.083 of the size. A *downward* shift sets no
    // `SUPERSCRIPT` flag and remains within the calibrated 10% tolerance; 1 pt is inside the
    // line-clustering tolerance (0.5 × size), so it is still one line.
    let got = shape(b"BT /F1 12 Tf 100 700 Td (AB) Tj 12 -1 Td (CD) Tj ET");
    assert_shape(&got, &[("ABCD", 12.0)]);
}

// === SPANVIS-005: interleaved runs are no longer shuffled together =======

#[test]
fn spanvis_005_interleaved_runs_do_not_share_a_span() {
    // Painted A, B, X but laid out A(100) X(106) B(118): the 24 pt "X" lands
    // between the two 12 pt glyphs once the line is sorted along its reading
    // axis. The old rule produced one span reading "AXB" carrying A's matrix.
    let got = shape(
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj \
          12 0 0 12 118 700 Tm (B) Tj \
          24 0 0 24 106 700 Tm (X) Tj ET",
    );
    assert_shape(&got, &[("A", 12.0), ("X", 24.0), ("B", 12.0)]);
    // Every span is homogeneous: its published matrix describes all its chars.
    for s in line_spans(
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj \
          12 0 0 12 118 700 Tm (B) Tj \
          24 0 0 24 106 700 Tm (X) Tj ET",
    ) {
        for c in &s.chars {
            approx(c.rendered_size, s.rendered_size, EPS);
        }
    }
}

// === the text that must still not split =================================

// === SPANVIS-006: an ordinary line stays one span =======================

#[test]
fn spanvis_006_ordinary_line_stays_one_span() {
    let got = shape(b"BT /F1 12 Tf 100 700 Td (Hel) Tj (lo) Tj ET");
    assert_shape(&got, &[("Hello", 12.0)]);
}

// === SPANVIS-007: a multi-glyph superscript stays one span ==============

#[test]
fn spanvis_007_superscript_is_not_shattered() {
    // "Note" 12 pt at x 100→124, then an 8 pt "12" raised 4 pt starting where it
    // ends. The two runs part on `size`/`SUPERSCRIPT` as they always did; what
    // matters here is that the superscript's own two glyphs, which share a
    // matrix and a baseline, are *not* split from each other.
    let got = shape(b"BT /F1 12 Tf 100 700 Td (Note) Tj ET BT /F1 8 Tf 124 704 Td (12) Tj ET");
    assert_shape(&got, &[("Note", 12.0), ("12", 8.0)]);
}

// === SPANVIS-008: a re-issued `Tm` per glyph stays one span =============

#[test]
fn spanvis_008_per_glyph_text_matrix_stays_one_span() {
    // Only the translation moves — the pen position, which every glyph has its
    // own of by definition. Splitting on that would put every glyph in its own
    // span on the many PDFs that set `Tm` per glyph.
    let got = shape(
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj \
          12 0 0 12 106 700 Tm (B) Tj \
          12 0 0 12 112 700 Tm (C) Tj ET",
    );
    assert_shape(&got, &[("ABC", 12.0)]);
}

// === SPANVIS-009: producer rounding is tolerated ========================

#[test]
fn spanvis_009_rounding_noise_stays_one_span() {
    // A producer that re-emits `Tm` with rounded operands perturbs the linear
    // part by ~1e-5 of the size. The tolerances are not zero for this reason.
    let got = shape(
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj \
          11.9999 0 0 12.0001 106 700.0001 Tm (B) Tj ET",
    );
    assert_shape(&got, &[("AB", 12.0)]);
}

// === SPANVIS-010: the tolerances are relative, not absolute =============

#[test]
fn spanvis_010_tolerances_scale_with_the_font() {
    // The same absolute 1 pt baseline drop retained in a 12 pt run
    // (`SPANVIS-004`) is 0.01 of a 100 pt run — inside the tolerance, so a
    // display line keeps its single span. 100 pt advances 50 pt per glyph.
    let got = shape(
        b"BT /F1 1 Tf 100 0 0 100 100 600 Tm (A) Tj \
          100 0 0 100 150 599 Tm (B) Tj ET",
    );
    assert_shape(&got, &[("AB", 100.0)]);
}
