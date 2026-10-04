//! The per-page content work budget covers rendering: a Type 3 glyph whose
//! procedure shows ten more glyphs of the same font recurses 10× per level up
//! to the Type 3 depth cap (10⁸ procedure runs) unless the page's budget,
//! shared by the page content and every glyph procedure, cuts it off.

use std::sync::Arc;

use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_render::{render_page, DisplayList, RenderOptions};

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(format!("<< {} /Length {} >>\nstream\n", dict, data.len()).as_bytes());
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

fn build(objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let max = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize; (max + 1) as usize];
    for (num, body) in &objects {
        offsets[*num as usize] = out.len();
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for n in 1..=max {
        out.extend_from_slice(format!("{:010} 00000 n \n", offsets[n as usize]).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    out
}

/// One page showing `a` in Type 3 font `/T3`, whose `a` procedure fills a box
/// and — when `recursive` — shows `aaaaaaaaaa` in `/T3` again (the font's own
/// resources).
fn type3_page(limits: Limits, recursive: bool) -> (Arc<DocumentStore>, Page) {
    let charproc: &[u8] = if recursive {
        b"1000 0 0 0 1000 1000 d1 0 0 100 100 re f BT /T3 0.1 Tf (aaaaaaaaaa) Tj ET"
    } else {
        b"1000 0 0 0 1000 1000 d1 0 0 600 600 re f"
    };
    let bytes = build(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
              /Resources << /Font << /T3 5 0 R >> >> /Contents 4 0 R >>"
                .to_vec(),
        ),
        (4, stream("", b"BT /T3 100 Tf 20 20 Td (a) Tj ET")),
        (
            5,
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
              /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /a 6 0 R >> \
              /Encoding << /Type /Encoding /Differences [97 /a] >> \
              /FirstChar 97 /LastChar 97 /Widths [1000] \
              /Resources << /Font << /T3 5 0 R >> >> >>"
                .to_vec(),
        ),
        (6, stream("", charproc)),
    ]);
    let doc = Arc::new(DocumentStore::from_bytes(bytes, limits).expect("open"));
    let page = Page::new(doc.clone(), 0, ObjRef::new(3, 0));
    (doc, page)
}

#[test]
fn type3_procedure_fan_out_is_cut_off_by_the_page_budget() {
    let limits = Limits::default().with_max_page_content_ops(20_000);
    let (doc, page) = type3_page(limits, true);
    let started = std::time::Instant::now();
    let pm = render_page(&doc, &page, &RenderOptions::default()).expect("render");
    assert!(pm.width > 0);
    let dl = DisplayList::from_page(&doc, &page);
    let _ = dl
        .get_pixmap(&doc, &RenderOptions::default())
        .expect("display list");
    // 20 000 work units allow a few hundred procedure runs; the unbounded
    // recursion would be 10⁸.
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

#[test]
fn a_budget_that_covers_the_page_renders_it_unchanged() {
    // An ordinary Type 3 glyph: the default budget and an unlimited one
    // render the same pixels, and the glyph is actually drawn.
    let (full_doc, full_page) = type3_page(Limits::default(), false);
    let full = render_page(&full_doc, &full_page, &RenderOptions::default()).expect("render");
    let (doc, page) = type3_page(Limits::default().with_max_page_content_ops(u64::MAX), false);
    let same = render_page(&doc, &page, &RenderOptions::default()).expect("render");
    assert_eq!(full.samples(), same.samples());
    assert!(full.samples().iter().any(|&v| v < 128), "glyph drawn");
}
