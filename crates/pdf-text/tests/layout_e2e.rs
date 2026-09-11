//! M2c end-to-end tests: build a `TextPage` from a real self-built PDF page via
//! `build_textpage`, asserting the full block/line/span/word structure + text in
//! device space. Catalog IDs: `LAYOUT-E2E-*`.

mod common;

use std::sync::Arc;

use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::Limits;
use pdf_text::{build_textpage, words};

use common::{winansi_type1, PageDoc};

/// Wraps a fixture `(DocumentStore, _)` into a `Page` handle. The fixture always
/// emits the single page as object 3, generation 0 (see `tests/common`).
fn page_handle(doc: pdf_core::DocumentStore) -> Page {
    Page::new(Arc::new(doc), 0, ObjRef::new(3, 0))
}

#[test]
fn layout_e2e_001_two_lines_two_words_structure_and_text() {
    // A WinAnsi font with explicit widths so advances are deterministic. Codes
    // 'A'..='Z' and space; width 500 for letters, 250 for space (1000-unit).
    // FirstChar 32 (space). Widths cover 32..=90 ('Z').
    let mut widths = vec![250i64]; // space (32)
    for code in 33..=90 {
        // give space (would be 32 only) — punctuation/digits 33..=64 width 500
        let _ = code;
        widths.push(500);
    }
    let font = winansi_type1("Helvetica", 32, &widths);

    // Content: "AB CD" on line 1 (y=700), "EF GH" on line 2 (y=686, 14pt down).
    // 12pt text. Each letter advances 500/1000*12 = 6pt; space 250/1000*12=3pt.
    let content = b"BT /F1 12 Tf \
        1 0 0 1 100 700 Tm (AB CD) Tj \
        1 0 0 1 100 686 Tm (EF GH) Tj \
        ET";

    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());

    // One text block with two lines (lines 14pt apart → same paragraph block).
    let text_blocks: Vec<_> = tp
        .blocks
        .iter()
        .filter(|b| b.kind == pdf_text::BlockKind::Text)
        .collect();
    assert_eq!(text_blocks.len(), 1, "expected one paragraph block");
    let block = text_blocks[0];
    assert_eq!(block.lines.len(), 2, "expected two lines");

    // Line texts.
    let l0: String = block.lines[0]
        .spans
        .iter()
        .flat_map(|s| s.text.chars())
        .collect();
    let l1: String = block.lines[1]
        .spans
        .iter()
        .flat_map(|s| s.text.chars())
        .collect();
    assert_eq!(l0, "AB CD");
    assert_eq!(l1, "EF GH");

    // Device-space y-flip: line 1 (user y 700) is above line 2 (user y 686), so
    // its device y0 is smaller.
    assert!(block.lines[0].bbox.y0 < block.lines[1].bbox.y0);

    // Words: 2 per line, 4 total, with correct numbering.
    let ws = words(&tp);
    let triples: Vec<(usize, usize, usize, &str)> = ws
        .iter()
        .map(|w| (w.block_no, w.line_no, w.word_no, w.text.as_str()))
        .collect();
    assert_eq!(
        triples,
        vec![
            (0, 0, 0, "AB"),
            (0, 0, 1, "CD"),
            (0, 1, 0, "EF"),
            (0, 1, 1, "GH"),
        ]
    );
}

#[test]
fn layout_e2e_002_device_space_top_left_origin() {
    let widths: Vec<i64> = std::iter::once(250)
        .chain(std::iter::repeat_n(500, 58))
        .collect();
    let font = winansi_type1("Helvetica", 32, &widths);
    // A single word near the top of the page (user y 760 on a 792-high page).
    let content = b"BT /F1 12 Tf 1 0 0 1 72 760 Tm (Top) Tj ET";
    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());

    assert_eq!((tp.width, tp.height), (612.0, 792.0));
    let line = &tp.blocks[0].lines[0];
    // Near the top → small device y. Baseline device y ≈ 792 - 760 = 32.
    assert!(line.bbox.y0 < 100.0, "text near top should have small y");
    // x is preserved (x0 = 72).
    assert!((line.bbox.x0 - 72.0).abs() < 1.0);
    let text: String = line.spans.iter().flat_map(|s| s.text.chars()).collect();
    assert_eq!(text, "Top");
}

/// Helvetica AFM advances for codes 32..=126 (1000-unit glyph space), so a
/// full-width `Tj` string lays its glyph cells edge to edge exactly as a real
/// Core-14 page does.
const HELV: [i64; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32..47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 48..63
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 64..79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 80..95
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 96..111
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // 112..126
];

/// Every line's text (spans concatenated), in emitted block/line order.
fn line_texts(tp: &pdf_text::TextPage) -> Vec<String> {
    tp.blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .map(|l| l.spans.iter().flat_map(|s| s.text.chars()).collect())
        .collect()
}

/// Builds a page from `content` with Helvetica (full AFM widths) as `/F1`.
fn helvetica_page(content: &[u8]) -> pdf_text::TextPage {
    let font = winansi_type1("Helvetica", 32, &HELV);
    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    build_textpage(page.document(), &page, &Limits::unbounded_decode())
}

/// LAYOUT-E2E-003: a single `Tj` title whose glyph cells touch edge to edge is
/// never broken into several lines by the page's column-gutter detector.
///
/// Reproduces the fintabnet `SLB_2015_page_72` header: a financial statement
/// whose label column (x 40..150) and five numeric columns (x ≥ 300) leave a
/// wide glyph-free band in every table row, so `detect_page_gutters` finds a
/// gutter there; the one-string title `(SCHLUMBERGER LIMITED AND SUBSIDIARIES) Tj`
/// runs straight across that band with zero gap between consecutive cells. A
/// gutter is a property of the *other* lines; it must not cut a run whose own
/// glyphs are contiguous (PyMuPDF keeps the title as one line).
#[test]
fn layout_e2e_003_touching_glyphs_cross_gutter_stay_one_line() {
    let mut content = String::from("BT /F1 10 Tf ");
    content.push_str("1 0 0 1 60 740 Tm (SCHLUMBERGER LIMITED AND SUBSIDIARIES) Tj ");
    for row in 0..12 {
        let y = 700.0 - 14.0 * row as f64;
        content.push_str(&format!("1 0 0 1 40 {y} Tm (Revenue and other income) Tj "));
        for x in [300, 360, 420, 480, 540] {
            content.push_str(&format!("1 0 0 1 {x} {y} Tm (12,345) Tj "));
        }
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    assert!(
        lines
            .iter()
            .any(|l| l == "SCHLUMBERGER LIMITED AND SUBSIDIARIES"),
        "title was cut at the table gutter: {lines:?}"
    );
    let ws: Vec<String> = words(&tp).iter().map(|w| w.text.clone()).collect();
    for w in ["SCHLUMBERGER", "LIMITED", "AND", "SUBSIDIARIES"] {
        assert!(ws.iter().any(|x| x == w), "word {w:?} missing from {ws:?}");
    }
    // The table rows themselves still split at their genuine inter-cell gaps.
    assert!(
        lines.iter().any(|l| l == "Revenue and other income"),
        "table label row lost: {lines:?}"
    );
}

/// LAYOUT-E2E-004: a `Tj` whose *space glyph* sits inside a detected gutter band
/// stays one line — the space cell fills the gap edge to edge, so there is no
/// along-axis whitespace at the gutter to cut on.
///
/// Reproduces the IRS `f1120` p1 pattern (`(Schedule C) Tj` → `Sche\ndule C` /
/// `Schedule \nC`): a two-column form whose label column (x 36..74) and value
/// columns (x ≥ 110) leave a band at x 71..110 (midpoint ≈ 90.5), and a section
/// title painted across that band as one string, its space cell spanning
/// x ≈ 89.4..91.6 — right over the gutter midpoint.
#[test]
fn layout_e2e_004_space_glyph_inside_gutter_stays_one_line() {
    let mut content = String::from("BT /F1 8 Tf ");
    content.push_str("1 0 0 1 56 740 Tm (Schedule C) Tj ");
    for row in 0..12 {
        let y = 700.0 - 12.0 * row as f64;
        content.push_str(&format!("1 0 0 1 36 {y} Tm (Dividends) Tj "));
        content.push_str(&format!("1 0 0 1 110 {y} Tm (1,234) Tj "));
        content.push_str(&format!("1 0 0 1 160 {y} Tm (5,678) Tj "));
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    assert!(
        lines.iter().any(|l| l == "Schedule C"),
        "form title was cut at the gutter: {lines:?}"
    );
}

/// LAYOUT-E2E-005 (reverse invariant of 003/004): a genuine two-column body —
/// each column its own `Tj` per baseline, the two columns sharing baselines and
/// separated by a real glyph-free gutter — still splits into per-column lines
/// and reads column-major (same expectation as LAYOUT-COLUMN-REGRESSION-001).
#[test]
fn layout_e2e_005_true_two_column_still_splits_at_gutter() {
    let left = ["Lone", "Ltwo", "Lthree", "Lfour", "Lfive", "Lsix"];
    let right = ["Rone", "Rtwo", "Rthree", "Rfour", "Rfive", "Rsix"];
    let mut content = String::from("BT /F1 10 Tf ");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        let y = 740.0 - 20.0 * i as f64;
        content.push_str(&format!(
            "1 0 0 1 40 {y} Tm ({l} column body text runs to here) Tj "
        ));
        content.push_str(&format!(
            "1 0 0 1 320 {y} Tm ({r} column body text runs to here) Tj "
        ));
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    assert_eq!(
        lines.len(),
        12,
        "each column line must stay its own line: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|l| !(l.contains("Lone") && l.contains("Rone"))),
        "columns merged into one line: {lines:?}"
    );
    let pos = |w: &str| {
        lines
            .iter()
            .position(|l| l.starts_with(w))
            .unwrap_or(usize::MAX)
    };
    let last_left = left.iter().map(|w| pos(w)).max().unwrap();
    let first_right = right.iter().map(|w| pos(w)).min().unwrap();
    assert!(
        last_left < first_right,
        "expected column-major order (all L before all R): {lines:?}"
    );
}

/// LAYOUT-E2E-006 (D4): a govinfo Federal Register running header spanning the
/// full page width is never fragmented at a body column gutter. The columns
/// below leave a wide glyph-free band (midpoint ≈ 217); the header is painted in
/// two positioned pieces whose ordinary ~3pt word gap happens to straddle that
/// midpoint. That gap is real along-axis whitespace (a word break, not a space
/// glyph, and well below the independent-run threshold), so the old midpoint-only
/// rule cut the header into fragments. Requiring the run's own gap to cover most
/// of the band keeps it one line (3pt ≪ 0.8 × 206pt), matching PyMuPDF — while a
/// genuine two-column body row, whose whole gutter is empty between its glyphs,
/// still splits.
#[test]
fn layout_e2e_006_full_width_header_word_gap_at_gutter_stays_one_line() {
    let mut content = String::from("BT /F1 10 Tf ");
    content.push_str("1 0 0 1 40 740 Tm (FederalRegisterVolumeNinetyNumberII) Tj ");
    content.push_str("1 0 0 1 217.5 740 Tm (RulesAndRegulations) Tj ");
    for row in 0..12 {
        let y = 700.0 - 14.0 * row as f64;
        content.push_str(&format!("1 0 0 1 40 {y} Tm (LeftColumnBody) Tj "));
        content.push_str(&format!("1 0 0 1 320 {y} Tm (RightColumnBody) Tj "));
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    // The header stays one line: both pieces on it, never fragmented at the gutter.
    assert!(
        lines.iter().any(|l| {
            l.contains("FederalRegisterVolumeNinetyNumberII") && l.contains("RulesAndRegulations")
        }),
        "running header was fragmented at the column gutter: {lines:?}"
    );
    // Reverse invariant: a genuine two-column body row — its whole gutter empty
    // between its own glyphs — still splits into per-column lines.
    assert!(
        lines.iter().any(|l| l == "LeftColumnBody"),
        "left column line missing: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "RightColumnBody"),
        "right column line missing: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|l| !(l.contains("LeftColumnBody") && l.contains("RightColumnBody"))),
        "column body rows merged across the gutter: {lines:?}"
    );
}

/// A full-width title can prevent the page-level column cut. The fallback
/// horizontal sweep must not split the continuous body at its paragraph gap.
#[test]
fn layout_e2e_007_title_above_columns_with_shared_paragraph_gap() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in 0..5 {
        let y = 750 - row * 12;
        content.push_str(&format!(
            "1 0 0 1 90 {y} Tm (FULL WIDTH TITLE EXTENDING ACROSS BOTH BODY COLUMNS) Tj "
        ));
    }
    for (prefix, x) in [("LEFT", 40), ("RIGHT", 330)] {
        for row in 0..12 {
            let y = 650 - row * 12 - if row >= 6 { 30 } else { 0 };
            content.push_str(&format!(
                "1 0 0 1 {x} {y} Tm ({prefix} {row:02} continuous body paragraph text) Tj "
            ));
        }
    }
    content.push_str("ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    assert_eq!(lines.len(), 29);
    assert!(lines[..5]
        .iter()
        .all(|s| s == "FULL WIDTH TITLE EXTENDING ACROSS BOTH BODY COLUMNS"));
    let left_last = lines.iter().position(|s| s.starts_with("LEFT 11")).unwrap();
    let right_first = lines
        .iter()
        .position(|s| s.starts_with("RIGHT 00"))
        .unwrap();
    assert!(
        left_last < right_first,
        "body columns interleaved: {lines:?}"
    );
    assert_eq!(lines.iter().filter(|s| s.starts_with("LEFT")).count(), 12);
    assert_eq!(lines.iter().filter(|s| s.starts_with("RIGHT")).count(), 12);
}

/// A centered heading remains a boundary between independent column sections,
/// even when its text is shorter than half the page width.
#[test]
fn layout_e2e_008_short_heading_separates_column_sections() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in 0..5 {
        let y = 750 - row * 12;
        content.push_str(&format!(
            "1 0 0 1 90 {y} Tm (FULL WIDTH TITLE EXTENDING ACROSS BOTH BODY COLUMNS) Tj "
        ));
    }
    for (section, top) in [("UPPER", 650), ("LOWER", 440)] {
        for (side, x) in [("LEFT", 40), ("RIGHT", 330)] {
            for row in 0..6 {
                let y = top - row * 12;
                content.push_str(&format!(
                    "1 0 0 1 {x} {y} Tm ({section} {side} {row} body paragraph text) Tj "
                ));
            }
        }
    }
    content.push_str("1 0 0 1 255 530 Tm (SECTION TWO) Tj ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    let upper = lines
        .iter()
        .position(|s| s.starts_with("UPPER RIGHT 5"))
        .unwrap();
    let heading = lines.iter().position(|s| s == "SECTION TWO").unwrap();
    let lower = lines
        .iter()
        .position(|s| s.starts_with("LOWER LEFT 0"))
        .unwrap();
    assert!(
        upper < heading && heading < lower,
        "heading boundary lost: {lines:?}"
    );
    assert_eq!(lines.len(), 30);
}

/// A two-to-three-column transition must stay in top-to-bottom section order.
#[test]
fn layout_e2e_009_changed_column_structure_keeps_band_boundary() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in 0..5 {
        let y = 750 - row * 12;
        content.push_str(&format!(
            "1 0 0 1 90 {y} Tm (FULL WIDTH TITLE EXTENDING ACROSS BOTH BODY COLUMNS) Tj "
        ));
    }
    for (side, x) in [("LEFT", 40), ("RIGHT", 330)] {
        for row in 0..6 {
            let y = 650 - row * 12;
            content.push_str(&format!(
                "1 0 0 1 {x} {y} Tm (UPPER {side} {row} body paragraph text) Tj "
            ));
        }
    }
    for (side, x) in [("LEFT", 40), ("MIDDLE", 230), ("RIGHT", 420)] {
        for row in 0..6 {
            let y = 500 - row * 12;
            content.push_str(&format!(
                "1 0 0 1 {x} {y} Tm (LOWER {side} {row} column text) Tj "
            ));
        }
    }
    content.push_str("ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    let upper = lines
        .iter()
        .position(|s| s.starts_with("UPPER RIGHT 5"))
        .unwrap();
    let lower = lines
        .iter()
        .position(|s| s.starts_with("LOWER LEFT 0"))
        .unwrap();
    assert!(upper < lower, "column-count boundary lost: {lines:?}");
    assert_eq!(lines.len(), 35);
}

/// Sparse form values must not be merged across a later label/value row just
/// because both row bands admit geometric two-column cuts.
#[test]
fn layout_e2e_010_sparse_form_values_keep_their_row_band() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in 0..5 {
        let y = 750 - row * 12;
        content.push_str(&format!(
            "1 0 0 1 90 {y} Tm (FULL WIDTH TITLE EXTENDING ACROSS BOTH BODY COLUMNS) Tj "
        ));
    }
    for row in 0..6 {
        let y = 650 - row * 12;
        content.push_str(&format!(
            "1 0 0 1 40 {y} Tm (UPPER LABEL {row} description and contact details) Tj "
        ));
    }
    content.push_str("1 0 0 1 330 650 Tm (Name) Tj 1 0 0 1 330 626 Tm (Actual address of the lender for the customer) Tj ");
    for (side, x) in [("LABEL", 40), ("VALUE", 330)] {
        for row in 0..3 {
            let y = 520 - row * 12;
            content.push_str(&format!(
                "1 0 0 1 {x} {y} Tm (LOWER {side} {row} commercial register information) Tj "
            ));
        }
    }
    content.push_str("ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    let name = lines.iter().position(|s| s == "Name").unwrap();
    let next_label = lines
        .iter()
        .position(|s| s.starts_with("LOWER LABEL 0"))
        .unwrap();
    let address = lines
        .iter()
        .position(|s| s == "Actual address of the lender for the customer")
        .unwrap();
    assert!(
        name < address && address < next_label,
        "form value moved after next row: {lines:?}"
    );
    assert_eq!(lines.len(), 19);
}

/// Sparse values align with multiline labels, even when the value column is
/// painted first. Each label/value cell must finish before the next row.
#[test]
fn layout_e2e_011_label_value_cells_follow_rows_not_paint_order() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in 0..4 {
        let y = 700 - row * 80;
        content.push_str(&format!(
            "1 0 0 1 330 {y} Tm ([VALUE {row} short answer]) Tj "
        ));
    }
    for row in 0..4 {
        let y = 700 - row * 80;
        content.push_str(&format!(
            "1 0 0 1 40 {y} Tm (LABEL {row} with a long description) Tj "
        ));
        for offset in [12, 24, 36] {
            let yy = y - offset;
            content.push_str(&format!(
                "1 0 0 1 40 {yy} Tm (continuation of the label explanation) Tj "
            ));
        }
    }
    content.push_str("ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    for row in 0..4 {
        let label = lines
            .iter()
            .position(|s| s.starts_with(&format!("LABEL {row}")))
            .unwrap();
        let value = lines
            .iter()
            .position(|s| s == &format!("[VALUE {row} short answer]"))
            .unwrap();
        assert_eq!(value, label + 4, "cell text interleaved: {lines:?}");
        if row < 3 {
            let next = lines
                .iter()
                .position(|s| s.starts_with(&format!("LABEL {}", row + 1)))
                .unwrap();
            assert!(value < next, "value detached from label: {lines:?}");
        }
    }
    assert_eq!(lines.len(), 20);
}

/// The same sparse geometry without field placeholders can be ordinary prose.
#[test]
fn layout_e2e_012_sparse_prose_columns_remain_column_major() {
    for prefix in ["", "[1] ", "[] "] {
        let mut content = String::from("BT /F1 10 Tf ");
        for row in 0..4 {
            let y = 700 - row * 80;
            content.push_str(&format!(
                "1 0 0 1 330 {y} Tm ({prefix}RIGHT {row} short paragraph) Tj "
            ));
        }
        for row in 0..4 {
            let y = 700 - row * 80;
            content.push_str(&format!(
                "1 0 0 1 40 {y} Tm (LEFT {row} body paragraph begins here) Tj "
            ));
            for offset in [12, 24, 36] {
                let yy = y - offset;
                content.push_str(&format!(
                    "1 0 0 1 40 {yy} Tm (continuation of the prose paragraph) Tj "
                ));
            }
        }
        content.push_str("ET");
        let lines = line_texts(&helvetica_page(content.as_bytes()));
        let left = lines.iter().position(|s| s.starts_with("LEFT 3")).unwrap();
        let right = lines
            .iter()
            .position(|s| s.starts_with(&format!("{prefix}RIGHT 0")))
            .unwrap();
        assert!(
            left + 3 < right,
            "ordinary prose treated as a form: {lines:?}"
        );
        assert_eq!(lines.len(), 20);
    }
}

/// Repeated aligned value starts split tightly adjacent multiline labels into
/// separate cells even though the label column has no paragraph-sized gap.
#[test]
fn layout_e2e_013_adjacent_labels_use_the_value_row_boundaries() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in 0..4 {
        let y = 700 - row * 24;
        content.push_str(&format!(
            "1 0 0 1 330 {y} Tm ([VALUE {row} field instructions]) Tj "
        ));
    }
    for row in 0..4 {
        let y = 700 - row * 24;
        let yy = y - 12;
        content.push_str(&format!("1 0 0 1 40 {y} Tm (LABEL {row} with multiline description) Tj 1 0 0 1 40 {yy} Tm (continued label explanation) Tj "));
    }
    content.push_str("ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    for row in 0..4 {
        assert!(
            lines[row * 3].starts_with(&format!("LABEL {row}")),
            "{lines:?}"
        );
        assert_eq!(lines[row * 3 + 1], "continued label explanation");
        assert_eq!(
            lines[row * 3 + 2],
            format!("[VALUE {row} field instructions]")
        );
    }
    assert_eq!(lines.len(), 12);
}

/// Multiline values stay intact; an empty value and a plain Yes/No answer are
/// retained in their own rows once repeated placeholders establish the form.
#[test]
fn layout_e2e_014_multiline_and_empty_values_keep_cell_order() {
    let mut content = String::from("BT /F1 10 Tf ");
    for row in [0, 3] {
        let y = 700 - row * 70;
        let yy = y - 12;
        content.push_str(&format!("1 0 0 1 330 {y} Tm ([VALUE {row} detailed instructions) Tj 1 0 0 1 330 {yy} Tm (continued value instructions]) Tj "));
    }
    content.push_str("1 0 0 1 330 560 Tm (Yes/No) Tj ");
    for row in 0..4 {
        let y = 700 - row * 70;
        let yy = y - 12;
        content.push_str(&format!("1 0 0 1 40 {y} Tm (LABEL {row} with a long description) Tj 1 0 0 1 40 {yy} Tm (continued label explanation) Tj "));
    }
    content.push_str("ET");
    let lines = line_texts(&helvetica_page(content.as_bytes()));
    let mut expected = Vec::new();
    for row in 0..4 {
        expected.push(format!("LABEL {row} with a long description"));
        expected.push("continued label explanation".to_string());
        if row == 0 || row == 3 {
            expected.push(format!("[VALUE {row} detailed instructions"));
            expected.push("continued value instructions]".to_string());
        } else if row == 2 {
            expected.push("Yes/No".to_string());
        }
    }
    assert_eq!(lines, expected);
}
