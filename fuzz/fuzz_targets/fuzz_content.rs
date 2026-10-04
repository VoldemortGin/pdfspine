#![no_main]
//! Content-stream fuzz target: the input bytes become a page's (uncompressed)
//! content stream inside a minimal, otherwise valid PDF shell, then run through
//! the whole text/table pipeline — tokenize → interpret (extraction and render
//! recording) → TextPage → words → table detection (`Lines` and `Text`).
//! Must never panic, overflow the stack, OOM or hang: nesting, text-state /
//! CTM extremes, Form fan-out and ruling sets are all content-controlled.
//! Run with `cargo +nightly fuzz run fuzz_content -- -max_len=65536`.

use std::sync::Arc;

use libfuzzer_sys::fuzz_target;
use pdf_core::{DocumentStore, Limits, Page};
use pdf_text::tables::{drawings_to_device, find_tables, Strategy, TableOptions};

/// A one-page PDF whose page content stream is `content`; the page has one
/// Helvetica font (`/F1`) and one Form XObject (`/X0`) that re-runs the same
/// content, so `Do` recursion and font lookups are reachable.
fn shell(content: &[u8]) -> Vec<u8> {
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> /XObject << /X0 6 0 R >> >> /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream(b"", content),
        stream(
            b"/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> /XObject << /X0 6 0 R >> >>",
            content,
        ),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn stream(dict: &[u8], body: &[u8]) -> Vec<u8> {
    let mut s = b"<< ".to_vec();
    s.extend_from_slice(dict);
    s.extend_from_slice(format!(" /Length {} >>\nstream\n", body.len()).as_bytes());
    s.extend_from_slice(body);
    s.extend_from_slice(b"\nendstream");
    s
}

fuzz_target!(|data: &[u8]| {
    // A smaller per-page work budget than the default keeps each run fast;
    // the budget mechanism itself is what fan-out inputs exercise.
    let limits = Limits::default().with_max_page_content_ops(1_000_000);
    let Ok(doc) = DocumentStore::from_bytes(shell(data), limits) else {
        return;
    };
    let doc = Arc::new(doc);
    let Some(&page_ref) = pdf_core::pagetree::page_refs(&doc).first() else {
        return;
    };
    let page = Page::new(doc.clone(), 0, page_ref);
    let Some(page_dict) = page.dict() else {
        return;
    };
    let res = pdf_text::interpret_page(&doc, &page_dict);
    let _ = pdf_text::interpret_page_render(&doc, &page_dict);
    let tp = pdf_text::layout::build_textpage(&doc, &page, &limits);
    let _ = pdf_text::serialize::to_text(&tp, 0);
    let words = pdf_text::words(&tp);
    let transform = pdf_text::layout::page_transform(page.cropbox(), page.rotation());
    let drawings = drawings_to_device(&res.drawings, &transform);
    for strategy in [Strategy::Lines, Strategy::Text] {
        let found = find_tables(
            &tp,
            &words,
            &drawings,
            &TableOptions::with_strategy(strategy),
        );
        for table in &found.tables {
            let _ = table.extract(&words);
        }
    }
});
