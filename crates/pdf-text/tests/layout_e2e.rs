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

/// Equivalent 10pt glyphs may use Tf10/Tm1 or Tf1/Tm10. The column-gutter
/// splitter must use their device size, otherwise a 2pt occupancy sliver cuts
/// the normal word space in a full-width running header.
#[test]
fn layout_e2e_015_scaled_font_keeps_full_width_header_intact() {
    let extract = |tf: i32, scale: f64| {
        let mut content = format!("BT /F1 {tf} Tf ");
        for (x, y, text) in [
            (150.0, 750, "HEADER COLUMN"),
            (216.62, 750, "/"),
            (
                223.24,
                750,
                "VOLUME AND DATE WITH A FULL WIDTH RUNNING HEADER",
            ),
        ] {
            content.push_str(&format!("{scale} 0 0 {scale} {x} {y} Tm ({text}) Tj "));
        }
        // A slightly larger, remote page number shares the header baseline.
        // Its size must not set the scale of the whole running-header text.
        let number_scale = scale * 1.1;
        content.push_str(&format!(
            "{number_scale} 0 0 {number_scale} 560 750 Tm (527) Tj "
        ));
        for y in [690, 680, 670] {
            content.push_str(&format!("{scale} 0 0 {scale} 218 {y} Tm (X) Tj "));
        }
        for x in [40, 225, 410] {
            for row in 0..20 {
                let y = 630 - row * 14;
                content.push_str(&format!(
                    "{scale} 0 0 {scale} {x} {y} Tm (ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHI) Tj "
                ));
            }
        }
        content.push_str("ET");
        let widths = vec![500; 95];
        let font = winansi_type1("Helvetica", 32, &widths);
        let (doc, _) = PageDoc::new()
            .font("F1", font)
            .content(content.as_bytes())
            .open();
        let page = page_handle(doc);
        build_textpage(page.document(), &page, &Limits::unbounded_decode())
    };
    let ordinary = line_texts(&extract(10, 1.0));
    let scaled = line_texts(&extract(1, 10.0));
    assert!(
        ordinary
            .iter()
            .any(|s| s == "HEADER COLUMN / VOLUME AND DATE WITH A FULL WIDTH RUNNING HEADER"),
        "ordinary {ordinary:?}"
    );
    assert_eq!(
        scaled, ordinary,
        "equivalent font transforms changed gutter splits"
    );
    assert_eq!(
        line_texts(&extract(100, 0.1)),
        ordinary,
        "shrinking a large Tf changed gutter splits"
    );
}

/// Real 4pt text still needs a narrow 3pt column gap: it is below the ordinary
/// independent-run threshold, so the scale-aware gutter detector must retain it.
#[test]
fn layout_e2e_016_small_text_retains_true_narrow_column_gutter() {
    let mut content = String::from("BT /F1 4 Tf ");
    let suffix = "A".repeat(33);
    for (side, x) in [("L", 40), ("R", 113)] {
        for row in 0..8 {
            let y = 700 - row * 6;
            content.push_str(&format!("1 0 0 1 {x} {y} Tm ({side}{row}{suffix}) Tj "));
        }
    }
    content.push_str("ET");
    let font = winansi_type1("Helvetica", 32, &[500; 95]);
    let (doc, _) = PageDoc::new()
        .font("F1", font)
        .content(content.as_bytes())
        .open();
    let page = page_handle(doc);
    let lines = line_texts(&build_textpage(
        page.document(),
        &page,
        &Limits::unbounded_decode(),
    ));
    assert_eq!(lines.len(), 16, "narrow true columns merged: {lines:?}");
    for (i, line) in lines.iter().enumerate() {
        let side = if i < 8 { "L" } else { "R" };
        assert_eq!(line, &format!("{side}{}{suffix}", i % 8));
    }
}

/// A neighboring column can seed a cluster halfway between two tight body
/// baselines. The small-gap header guard must not suppress cross-baseline cuts
/// or weave an independently raised marker into the following ordinary word.
#[test]
fn layout_e2e_017_header_guard_retains_mixed_baselines_and_markers() {
    let mut content = String::from("BT /F1 1 Tf ");
    for row in 0..16 {
        let y = 700.0 - f64::from(row) * 12.0;
        content.push_str(&format!("10 0 0 10 40 {y} Tm (LEFT COLUMN BODY TEXT) Tj "));
    }
    content.push_str("10 0 0 10 40 675.5 Tm (BRIDGING NEIGHBOR) Tj ");
    content.push_str("8 0 0 8 222 680 Tm (Deputy Assistant Secretary) Tj ");
    content.push_str("8 0 0 8 222 671 Tm (Negotiations performing duties) Tj ");
    content.push_str("10 0 0 10 222 640 Tm (The Act) Tj ");
    content.push_str("6 0 0 6 258.5 644 Tm (12) Tj ");
    content.push_str("10 0 0 10 222 628 Tm (consistent with the rules) Tj ET");
    let font = winansi_type1("Helvetica", 32, &[500; 95]);
    let (doc, _) = PageDoc::new()
        .font("F1", font)
        .content(content.as_bytes())
        .open();
    let page = page_handle(doc);
    let lines = line_texts(&build_textpage(
        page.document(),
        &page,
        &Limits::unbounded_decode(),
    ));
    for expected in [
        "Deputy Assistant Secretary",
        "Negotiations performing duties",
        "consistent with the rules",
    ] {
        assert!(lines.iter().any(|line| line == expected), "{lines:?}");
    }
    assert_eq!(lines.join(" ").matches("12").count(), 1);
}

/// Repeated aligned rows establish a real narrow column even when its gap is
/// smaller than an ordinary 10pt word space and Tf scale lives in the matrix.
#[test]
fn layout_e2e_018_repeated_rows_keep_tiny_true_column_gutter() {
    for rows in [2, 8] {
        let mut content = String::from("BT /F1 1 Tf ");
        let suffix = "A".repeat(33);
        for (side, x) in [("L", 40), ("R", 218)] {
            for row in 0..rows {
                let y = 700 - row * 14;
                content.push_str(&format!("10 0 0 10 {x} {y} Tm ({side}{row}{suffix}) Tj "));
            }
        }
        // The existing page-gutter detector requires four baseline runs.
        // Two unrelated lower lines keep that precondition independent of the
        // number of aligned rows supporting the narrow column.
        content.push_str("10 0 0 10 40 400 Tm (X) Tj 10 0 0 10 40 380 Tm (X) Tj ET");
        let font = winansi_type1("Helvetica", 32, &[500; 95]);
        let (doc, _) = PageDoc::new()
            .font("F1", font)
            .content(content.as_bytes())
            .open();
        let page = page_handle(doc);
        let lines = line_texts(&build_textpage(
            page.document(),
            &page,
            &Limits::unbounded_decode(),
        ));
        let lines: Vec<_> = lines.into_iter().filter(|line| line != "X").collect();
        assert_eq!(
            lines.len(),
            rows * 2,
            "true narrow columns merged: {lines:?}"
        );
        for (i, line) in lines.iter().enumerate() {
            let side = if i < rows { "L" } else { "R" };
            assert_eq!(line, &format!("{side}{}{suffix}", i % rows));
        }
    }
}

/// Unequal-length table cells share a right-column start even when only one
/// row nearly touches it. Its left continuation must precede the right cell.
#[test]
fn layout_e2e_019_repeated_cell_start_keeps_long_cell_continuation() {
    let content = b"BT /F1 1 Tf \
        10 0 0 10 40 700 Tm (Institute of Physics and Power here) Tj \
        10 0 0 10 40 688 Tm (Atomic Energy) Tj \
        10 0 0 10 218 700 Tm (AlphaMed Inc.) Tj \
        10 0 0 10 40 660 Tm (Another institute) Tj \
        10 0 0 10 218 660 Tm (Another partner) Tj \
        10 0 0 10 40 400 Tm (X) Tj \
        10 0 0 10 40 380 Tm (X) Tj ET";
    let font = winansi_type1("Helvetica", 32, &[500; 95]);
    let (doc, _) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    let lines = line_texts(&build_textpage(
        page.document(),
        &page,
        &Limits::unbounded_decode(),
    ));
    let position = |text: &str| {
        lines
            .iter()
            .position(|line| line == text)
            .unwrap_or_else(|| panic!("missing {text}: {lines:?}"))
    };
    assert!(position("Institute of Physics and Power here") < position("Atomic Energy"));
    assert!(position("Atomic Energy") < position("AlphaMed Inc."));
}

/// A header repair must not feed a wider line back into the three-column cut.
/// Footnotes and printing metadata retain their existing body block geometry
/// and sequence, even when the footnotes were painted before the body.
#[test]
fn layout_e2e_020_header_repair_preserves_three_column_body() {
    let mut content = String::from("BT /F1 1 Tf ");
    content.push_str("10 0 0 10 150 750 Tm (HEADER COLUMN) Tj ");
    content.push_str("10 0 0 10 216.62 750 Tm (/) Tj ");
    content
        .push_str("10 0 0 10 223.24 750 Tm (VOLUME AND DATE WITH A FULL WIDTH RUNNING HEADER) Tj ");
    for y in [690, 680, 670] {
        content.push_str(&format!("10 0 0 10 218 {y} Tm (X) Tj "));
    }
    for (side, x) in [("L", 40), ("M", 225), ("R", 410)] {
        content.push_str(&format!("8 0 0 8 {x} 100 Tm ({side} FOOTNOTE) Tj "));
    }
    let suffix = "A".repeat(32);
    for (side, x) in [("L", 40), ("M", 225), ("R", 410)] {
        for row in 0..20 {
            let y = 630 - row * 14;
            content.push_str(&format!(
                "10 0 0 10 {x} {y} Tm ({side}{row:02}{suffix}) Tj "
            ));
        }
    }
    content.push_str("6 0 0 6 25 28 Tm (VerDate metadata) Tj ");
    content.push_str("6 0 0 6 202 28 Tm (PO Frm Fmt Sfmt) Tj ");
    content.push_str("6 0 0 6 348 28 Tm (PRINT FILE) Tj ET");
    let font = winansi_type1("Helvetica", 32, &[500; 95]);
    let (doc, _) = PageDoc::new()
        .font("F1", font)
        .content(content.as_bytes())
        .open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());
    let body: Vec<_> = tp
        .blocks
        .iter()
        .filter(|b| b.bbox.y0 > 60.0)
        .map(|b| {
            (
                b.bbox,
                b.lines
                    .iter()
                    .map(|line| {
                        line.spans
                            .iter()
                            .map(|span| span.text.as_str())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let mut expected = vec![
        vec!["X".to_string(); 3],
        vec![
            "L FOOTNOTE".to_string(),
            "M FOOTNOTE".to_string(),
            "R FOOTNOTE".to_string(),
        ],
    ];
    for side in ["L", "M", "R"] {
        expected.push(
            (0..20)
                .map(|row| format!("{side}{row:02}{suffix}"))
                .collect(),
        );
    }
    expected.push(vec![
        "VerDate metadata".to_string(),
        "PO Frm Fmt Sfmt".to_string(),
        "PRINT FILE".to_string(),
    ]);
    assert_eq!(
        body.iter()
            .map(|(_, lines)| lines.clone())
            .collect::<Vec<_>>(),
        expected
    );
    let bounds = [
        (218.0, 94.0, 223.0, 124.0),
        (40.0, 685.6, 450.0, 693.6),
        (40.0, 154.0, 215.0, 430.0),
        (225.0, 154.0, 400.0, 430.0),
        (410.0, 154.0, 585.0, 430.0),
        (25.0, 759.2, 378.0, 765.2),
    ];
    for ((actual, _), expected) in body.iter().zip(bounds) {
        for (a, e) in [actual.x0, actual.y0, actual.x1, actual.y1]
            .into_iter()
            .zip([expected.0, expected.1, expected.2, expected.3])
        {
            assert!(
                (a - e).abs() < 1e-6,
                "body block bounds changed: {actual:?}"
            );
        }
    }
    assert!(line_texts(&tp)
        .iter()
        .any(|line| line == "HEADER COLUMN / VOLUME AND DATE WITH A FULL WIDTH RUNNING HEADER"));
}

/// A late header repair must not discard a nearby glyph that the existing
/// fragment reattacher absorbed after the replacement was planned. Reciprocal
/// Tf scale makes that legacy reattachment radius much larger than device size.
#[test]
fn layout_e2e_021_header_repair_keeps_reattached_source_glyphs() {
    let mut content = String::from("BT /F1 100 Tf ");
    content.push_str("0.1 0 0 0.1 150 750 Tm (HEADER) Tj 0.1 0 0 0.1 184 750 Tm (COLUMN) Tj ");
    content.push_str("0.1 0 0 0.1 216.62 750 Tm (/) Tj ");
    content.push_str(
        "0.1 0 0 0.1 223.24 750 Tm (VOLUME AND DATE WITH A FULL WIDTH RUNNING HEADER) Tj ",
    );
    content.push_str("0.1 0 0 0.1 179.5 730 Tm (#) Tj ");
    // Tf1 body retains the small occupancy-valley candidates.
    content.push_str("/F1 1 Tf ");
    for y in [690, 680, 670] {
        content.push_str(&format!("10 0 0 10 218 {y} Tm (X) Tj "));
    }
    for x in [40, 225, 410] {
        for row in 0..20 {
            let y = 630 - row * 14;
            content.push_str(&format!(
                "10 0 0 10 {x} {y} Tm (ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHI) Tj "
            ));
        }
    }
    content.push_str("ET");
    let font = winansi_type1("Helvetica", 32, &[500; 95]);
    let (doc, _) = PageDoc::new()
        .font("F1", font)
        .content(content.as_bytes())
        .open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());
    let text = line_texts(&tp).join(" ");
    assert_eq!(
        text.matches('#').count(),
        1,
        "reattached glyph lost: {text}"
    );
}
