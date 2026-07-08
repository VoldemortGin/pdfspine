//! C-9 tab-stop acceptance (PRD §10): a `\t` advances the pen to the next tab
//! stop (Word's `defaultTabStop`, 36 pt by default; overridable via
//! [`Typesetter::set_tab_interval`]). Post-tab word x0 lands on the stop ± 1 pt.

mod common;

use common::*;
use pdf_typeset::{Block, FixedPages, PageGeom, ParaProps, Run};

/// The read-back x0 (relative to the left margin) of the first word whose text
/// contains `needle`, laid out as paragraph `"A\tB"` at 12 pt. `interval` sets
/// the tab stop (`None` keeps the 36 pt default).
fn post_tab_rel_x0(interval: Option<f64>, needle: char) -> f64 {
    let mut engine = ts();
    if let Some(pt) = interval {
        engine.set_tab_interval(pt);
    }
    let blocks = vec![Block::Paragraph(
        ParaProps::new(),
        vec![Run::new("A\tB", style(12.0))],
    )];
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let pages = engine.layout_flow(&blocks, &mut FixedPages::new(geom));
    let result = engine.emit(&pages).expect("emit");
    let ws = words(&result.pdf, 0);
    let w = ws
        .iter()
        .find(|w| w.4.contains(needle))
        .unwrap_or_else(|| panic!("word {needle:?} missing; got {ws:?}"));
    w.0 - 50.0 // x0 minus the 50 pt left margin (tab stops measure from line start)
}

#[test]
fn tab_advances_word_to_next_default_stop() {
    let rel = post_tab_rel_x0(None, 'B');
    let nearest = (rel / 36.0).round() * 36.0;
    assert!(
        (rel - nearest).abs() <= 1.0,
        "post-tab word x0 (rel {rel}) is not on a 36 pt tab stop"
    );
    assert!(
        rel >= 36.0 - 1.0,
        "a tab must advance at least one 36 pt stop past 'A' (rel {rel})"
    );
}

#[test]
fn custom_tab_interval_moves_the_stop() {
    let rel = post_tab_rel_x0(Some(72.0), 'B');
    let nearest = (rel / 72.0).round() * 72.0;
    assert!(
        (rel - nearest).abs() <= 1.0,
        "post-tab word x0 (rel {rel}) is not on a 72 pt tab stop"
    );
    assert!(
        rel >= 72.0 - 1.0,
        "a 72 pt interval must advance the stop to 72 pt (rel {rel})"
    );
}

#[test]
fn zero_or_negative_interval_keeps_the_default() {
    // 0 and negative intervals are ignored by set_tab_interval → 36 pt default.
    let rel = post_tab_rel_x0(Some(-10.0), 'B');
    let nearest = (rel / 36.0).round() * 36.0;
    assert!(
        (rel - nearest).abs() <= 1.0,
        "invalid interval must leave the 36 pt default (rel {rel})"
    );
}
