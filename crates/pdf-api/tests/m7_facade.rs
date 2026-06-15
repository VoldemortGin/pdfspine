//! M7 facade wiring (`TABLES-API-*` / `OCG-API-*` / `SVG-API-*`) — the
//! `pdf-api` ergonomic entries assemble the merged M7 Rust surfaces:
//! `page_find_tables` (textpage + device words + device drawings),
//! `Document` OCG read/write round-trip, and `page_get_svg_image`. All fixtures
//! are self-generated in-test (PRD §10).

use pdf_api::{Document, Matrix, Strategy, TableOptions};

/// A complete classic-xref PDF from `(num, body)` object pairs + trailer keys.
fn build_pdf(objects: &[(u32, &[u8])], root: u32, extra_trailer: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut max_num = 0u32;
    let mut offsets: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (num, body) in objects {
        offsets.insert(*num, out.len());
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
        max_num = max_num.max(*num);
    }
    let size = max_num + 1;
    let startxref = out.len();
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..size {
        match offsets.get(&num) {
            Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(
        format!("<< /Size {size} /Root {root} 0 R {extra_trailer} >>\n").as_bytes(),
    );
    out.extend_from_slice(format!("startxref\n{startxref}\n%%EOF\n").as_bytes());
    out
}

/// A one-page PDF whose content stream draws a 2-row × 3-col ruled grid with a
/// label in each cell (PDF user space, y-up).
fn ruled_table_pdf() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    for y in [700, 670, 640] {
        c.push_str(&format!("100 {y} m 400 {y} l S\n"));
    }
    for x in [100, 200, 300, 400] {
        c.push_str(&format!("{x} 640 m {x} 700 l S\n"));
    }
    c.push_str("BT /F1 10 Tf\n");
    for (x, y, t) in [
        (110, 685, "A1"),
        (210, 685, "B1"),
        (310, 685, "C1"),
        (110, 655, "A2"),
        (210, 655, "B2"),
        (310, 655, "C2"),
    ] {
        c.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj\n"));
    }
    c.push_str("ET\n");
    let content = c.into_bytes();
    let stream = {
        let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        s.extend_from_slice(&content);
        s.extend_from_slice(b"\nendstream");
        s
    };
    build_pdf(
        &[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>"),
            (3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
            (
                4,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
            ),
            (5, &stream),
        ],
        1,
        "",
    )
}

/// A minimal blank one-page PDF (for OCG add/save/reopen + SVG).
fn blank_pdf() -> Vec<u8> {
    build_pdf(
        &[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
            ),
        ],
        1,
        "",
    )
}

#[test]
fn tables_api_001_find_tables_detects_grid() {
    let doc = Document::open_bytes(ruled_table_pdf()).expect("open");
    let page = doc.load_page(0).expect("page");
    let finder = pdf_api::page_find_tables(&page, &TableOptions::default());
    assert_eq!(finder.len(), 1, "one ruled table detected");
    let table = &finder.tables[0];
    assert_eq!(table.row_count(), 2);
    assert_eq!(table.col_count(), 3);

    let grid = table.extract();
    assert_eq!(grid.len(), 2);
    assert_eq!(grid[0][0].as_deref(), Some("A1"));
    assert_eq!(grid[1][2].as_deref(), Some("C2"));
}

#[test]
fn tables_api_002_markdown_and_html_shape() {
    let doc = Document::open_bytes(ruled_table_pdf()).expect("open");
    let page = doc.load_page(0).expect("page");
    let finder = pdf_api::page_find_tables(&page, &TableOptions::default());
    let table = &finder.tables[0];

    let md = table.to_markdown();
    assert!(md.contains("A1"), "markdown carries cell text: {md}");
    assert!(md.contains('|'), "markdown is pipe-delimited");

    let html = table.to_html();
    assert!(html.contains("<table"), "html has a <table>");
    assert!(
        html.contains("<td") || html.contains("<th"),
        "html has cells"
    );
    assert!(html.contains("A1"));
}

#[test]
fn tables_api_003_strategy_from_str() {
    assert_eq!(pdf_api::strategy_from_str("lines"), Strategy::Lines);
    assert_eq!(
        pdf_api::strategy_from_str("lines_strict"),
        Strategy::LinesStrict
    );
    assert_eq!(pdf_api::strategy_from_str("text"), Strategy::Text);
    assert_eq!(pdf_api::strategy_from_str("LINES"), Strategy::Lines);
    assert_eq!(pdf_api::strategy_from_str("bogus"), Strategy::Lines);
}

#[test]
fn ocg_api_001_add_save_reopen_roundtrip() {
    let doc = Document::open_bytes(blank_pdf()).expect("open");
    assert!(doc.get_ocgs().is_empty(), "no layers initially");

    let xref = doc.add_ocg("Layer1", true, &[], None).expect("add_ocg");

    let bytes = doc
        .save_to_bytes(&pdf_core::SaveOptions::default().with_garbage(1))
        .expect("save");
    let re = Document::open_bytes(bytes).expect("reopen");

    let ocgs = re.get_ocgs();
    assert_eq!(ocgs.len(), 1, "layer present after reopen");
    let info = ocgs.get(&xref).expect("ocg by xref");
    assert_eq!(info.name, "Layer1");
    assert!(info.on);
    assert!(re.ocg_state(xref));

    let configs = re.layer_ui_configs();
    assert!(configs.iter().any(|c| c.text == "Layer1"));
}

#[test]
fn ocg_api_002_set_layer_off() {
    let doc = Document::open_bytes(blank_pdf()).expect("open");
    let xref = doc.add_ocg("Layer1", true, &[], None).expect("add_ocg");
    assert!(doc.ocg_state(xref));

    doc.set_layer(&[], &[xref]).expect("set_layer off");
    assert!(!doc.ocg_state(xref), "layer turned OFF");

    doc.set_layer_state(xref, true).expect("set_layer_state on");
    assert!(doc.ocg_state(xref), "layer turned back ON");
}

#[test]
fn svg_api_001_get_svg_image_wellformed() {
    let doc = Document::open_bytes(blank_pdf()).expect("open");
    let page = doc.load_page(0).expect("page");
    let svg = pdf_api::page_get_svg_image(&page, Matrix::IDENTITY).expect("svg");
    assert!(
        svg.starts_with("<?xml") || svg.starts_with("<svg"),
        "starts with xml/svg prologue: {:?}",
        &svg[..svg.len().min(40)]
    );
    assert!(svg.contains("<svg"), "has an <svg> root");
    assert!(svg.contains("</svg>"), "closed <svg>");
}
