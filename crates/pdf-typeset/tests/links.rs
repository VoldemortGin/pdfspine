//! TS-11 run-level hyperlink acceptance (PRD §10): a run carrying
//! `RunStyle::link` produces `/Link` annotations over its real laid-out
//! rectangles. Read back through the repo's own `get_links`: URI + rect
//! landing, same-URI merge on one line, one rectangle per line across breaks,
//! and no annotation without a link.

mod common;

use common::*;
use pdf_typeset::{Block, FixedPages, PageGeom, ParaProps, Run, RunStyle};

const URL: &str = "https://example.com/a";

/// A Liberation-Sans style at `size` pt linking to `uri`.
fn linked(size: f64, uri: &str) -> RunStyle {
    let mut s = style(size);
    s.link = Some(uri.to_string());
    s
}

/// Lays `runs` out as one left-aligned paragraph on a `pw`×`ph` / `margin`
/// page and returns the emitted bytes.
fn export_runs(runs: Vec<Run>, pw: f64, ph: f64, margin: f64) -> Vec<u8> {
    let mut engine = ts();
    let blocks = vec![Block::Paragraph(ParaProps::new(), runs)];
    let pages = engine.layout_flow(&blocks, &mut FixedPages::new(PageGeom::new(pw, ph, margin)));
    engine.emit(&pages).expect("emit").pdf
}

/// The single URI of a link (panics on any other kind).
fn uri(link: &pdf_api::Link) -> &str {
    match &link.kind {
        pdf_api::LinkKind::Uri(u) => u,
        other => panic!("expected a URI link, got {other:?}"),
    }
}

#[test]
fn single_run_link_lands_on_the_word() {
    let ph = 500.0;
    let margin = 50.0;
    let pdf = export_runs(vec![Run::new("Linked", linked(12.0, URL))], 400.0, ph, margin);
    let doc = open(&pdf);
    let links = doc.get_links(0);
    assert_eq!(links.len(), 1, "one link for one linked run");
    assert_eq!(uri(&links[0]), URL);

    // Rect landing (±1 pt): x0 at the left margin; the box covers the word's
    // read-back ink bbox (top-left coords via the page-height flip).
    let r = links[0].from;
    assert_near(r.x0, margin, 1.0, "link starts at the text left edge");
    let w = words(&pdf, 0);
    assert_eq!(w.len(), 1);
    let (word_top, word_bot) = (ph - r.y1, ph - r.y0);
    assert!(
        r.x0 <= w[0].0 + 1.0 && r.x1 >= w[0].2 - 1.0,
        "link x-range {:?} must span the word ink [{}, {}]",
        (r.x0, r.x1),
        w[0].0,
        w[0].2
    );
    assert!(
        word_top <= w[0].1 + 1.0 && word_bot >= w[0].3 - 1.0,
        "link must cover the word vertically ({word_top}..{word_bot} vs {}..{})",
        w[0].1,
        w[0].3
    );
}

#[test]
fn adjacent_same_uri_runs_merge_into_one_rect() {
    let pdf = export_runs(
        vec![Run::new("Two ", linked(12.0, URL)), Run::new("words", linked(12.0, URL))],
        400.0,
        500.0,
        50.0,
    );
    let links = open(&pdf).get_links(0);
    assert_eq!(links.len(), 1, "same-URI neighbours merge to one rect");
    assert_eq!(uri(&links[0]), URL);
    // The merged rect spans both words' ink extent.
    let w = words(&pdf, 0);
    let right = w.iter().map(|t| t.2).fold(f64::MIN, f64::max);
    assert!(links[0].from.x1 >= right - 1.0, "merged rect reaches the last word");
}

#[test]
fn different_uris_do_not_merge() {
    let pdf = export_runs(
        vec![
            Run::new("aaa", linked(12.0, "https://a.test/1")),
            Run::new("bbb", linked(12.0, "https://b.test/2")),
        ],
        400.0,
        500.0,
        50.0,
    );
    let mut links = open(&pdf).get_links(0);
    assert_eq!(links.len(), 2, "distinct URIs stay separate");
    links.sort_by(|a, b| a.from.x0.total_cmp(&b.from.x0));
    assert_eq!(uri(&links[0]), "https://a.test/1");
    assert_eq!(uri(&links[1]), "https://b.test/2");
    assert!(links[0].from.x1 <= links[1].from.x0 + 1.0, "rects are side by side");
}

#[test]
fn link_broken_across_lines_yields_one_rect_per_line() {
    // A hard break splits the linked run into two lines ⇒ two annotations.
    let pdf = export_runs(vec![Run::new("first\nsecond", linked(12.0, URL))], 400.0, 500.0, 50.0);
    let links = open(&pdf).get_links(0);
    assert_eq!(links.len(), 2, "one rect per line");
    for l in &links {
        assert_eq!(uri(l), URL);
    }
    // The first line sits above the second (larger PDF y, y grows upward).
    let mut ys: Vec<f64> = links.iter().map(|l| l.from.y0).collect();
    ys.sort_by(f64::total_cmp);
    assert!(ys[1] > ys[0] + 1.0, "the two line rects are vertically separated");
}

#[test]
fn unlinked_run_produces_no_annotation() {
    let pdf = export_runs(vec![Run::new("plain text", style(12.0))], 400.0, 500.0, 50.0);
    assert!(open(&pdf).get_links(0).is_empty(), "no link, no annotation");
}
