//! `RENDER-TYPE3-*` — full-page render integration tests for Type3 fonts.
//!
//! Each test builds a **self-contained** single-page PDF (classic xref) whose
//! page shows text in a `/Subtype /Type3` font. A Type3 glyph is a mini content
//! stream (`/CharProcs`) drawn in glyph space, mapped to text space by the
//! font's `/FontMatrix`, then by the text-rendering matrix (`Tfs`, `Tm`, CTM,
//! page transform). The tests assert the glyph's shape lands at the expected
//! device pixels (and that empty regions stay white), exercise `/FontMatrix`
//! scaling, the d0 (colored) vs d1 (uncolored) metric operators, and that a
//! missing `/CharProcs` entry degrades to "draw nothing" without panicking.

use std::sync::Arc;

use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_image::pixmap::Pixmap;
use pdf_render::{render_page, RenderOptions};

// ============================================================================
// Minimal classic-xref PDF builder (mirrors render_page.rs).
// ============================================================================

struct Pdf {
    objects: Vec<(u32, Vec<u8>)>,
}

impl Pdf {
    fn new() -> Self {
        Pdf {
            objects: Vec::new(),
        }
    }

    fn obj(mut self, num: u32, body: impl AsRef<[u8]>) -> Self {
        self.objects.push((num, body.as_ref().to_vec()));
        self
    }

    fn build(mut self) -> Vec<u8> {
        self.objects.sort_by_key(|(n, _)| *n);
        let max = self.objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = vec![0usize; (max + 1) as usize];
        for (num, body) in &self.objects {
            offsets[*num as usize] = out.len();
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_off = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", max + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for n in 1..=max {
            out.extend_from_slice(format!("{:010} 00000 n \n", offsets[n as usize]).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                max + 1,
                xref_off
            )
            .as_bytes(),
        );
        out
    }
}

/// A stream object body: `<< dict >>\nstream\n…\nendstream`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(format!("<< {} /Length {} >>\nstream\n", dict, data.len()).as_bytes());
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

/// Opens raw PDF bytes and returns `(Arc<DocumentStore>, Page)` for page 0
/// (object 3, per the builder convention).
fn open_page(bytes: Vec<u8>) -> (Arc<DocumentStore>, Page) {
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open pdf");
    let arc = Arc::new(doc);
    let page = Page::new(arc.clone(), 0, ObjRef::new(3, 0));
    (arc, page)
}

const MEDIA: &str = "[0 0 200 200]";

/// Assembles a one-page PDF: page content `content`, `/Resources` literal `res`,
/// plus the extra indirect objects (the Type3 font + CharProc streams).
fn page_pdf_extra(content: &[u8], res: &str, extra: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox {MEDIA} \
                 /Resources {res} /Contents 4 0 R >>"
            )
            .into_bytes(),
        )
        .obj(4, stream("", content));
    for (num, body) in extra {
        pdf = pdf.obj(num, body);
    }
    pdf.build()
}

// ============================================================================
// Pixel helpers.
// ============================================================================

fn px(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let v = pm.pixel(x, y).expect("pixel in range");
    (v[0], v[1], v[2])
}

/// Count of pixels strictly darker than mid-gray on all channels.
fn dark_count(pm: &Pixmap) -> usize {
    let n = pm.n as usize;
    pm.samples()
        .chunks_exact(n)
        .filter(|c| c[0] < 128 && c[1] < 128 && c[2] < 128)
        .count()
}

/// Count of pixels matching `(r, g, b)` exactly.
fn color_count(pm: &Pixmap, rgb: (u8, u8, u8)) -> usize {
    let n = pm.n as usize;
    pm.samples()
        .chunks_exact(n)
        .filter(|c| (c[0], c[1], c[2]) == rgb)
        .count()
}

fn render(doc: &DocumentStore, page: &Page) -> Pixmap {
    render_page(doc, page, &RenderOptions::default()).expect("render_page ok")
}

// ============================================================================
// Type3 font object assemblers.
// ============================================================================

/// A Type3 font (resource `/T3`) drawing code 65 ('A') as the CharProc bytes
/// `glyph`. `font_matrix` is the `/FontMatrix` array literal (glyph→text space).
/// Objects: 10 (font), 11 (CharProcs dict), 12 (encoding dict), 13 (glyph proc).
fn type3_font_objs(font_matrix: &str, glyph: &[u8]) -> Vec<(u32, Vec<u8>)> {
    vec![
        (
            10,
            format!(
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
                 /FontMatrix {font_matrix} /CharProcs 11 0 R /Encoding 12 0 R \
                 /FirstChar 65 /LastChar 65 /Widths [1000] /Resources << >> >>"
            )
            .into_bytes(),
        ),
        (11, b"<< /Achar 13 0 R >>".to_vec()),
        (
            12,
            b"<< /Type /Encoding /Differences [65 /Achar] >>".to_vec(),
        ),
        (13, stream("", glyph)),
    ]
}

// ============================================================================
// RENDER-TYPE3-001: a d1 glyph fills its box region; elsewhere stays white.
// ============================================================================

#[test]
fn render_type3_001_glyph_fills_box_region() {
    // FontMatrix maps 1000 glyph units to 1 text unit; at Tf 100 the glyph cell
    // spans 100 user units. Glyph proc draws a filled box covering most of the
    // cell (glyph space 100..900 x 100..900).
    let glyph = b"1000 0 d0 100 100 800 800 re f";
    let content = b"BT /T3 100 Tf 50 50 Td (A) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.001 0 0 0.001 0 0]", glyph),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);

    // The box maps to user (50+10 .. 50+90) = (60..130) in x and y; device
    // y-flip puts it around the page center. The center of the cell is painted.
    assert!(dark_count(&pm) > 1000, "Type3 glyph must paint many pixels");
    // A far corner stays white.
    assert_eq!(px(&pm, 5, 5), (255, 255, 255), "far corner stays white");
}

// ============================================================================
// RENDER-TYPE3-002: /FontMatrix scaling changes the painted glyph size.
// ============================================================================

#[test]
fn render_type3_002_font_matrix_scales_glyph() {
    let glyph = b"1000 0 d0 0 0 1000 1000 re f";
    let content = b"BT /T3 100 Tf 20 20 Td (A) Tj ET";

    // Small font matrix (0.0005 → glyph cell ~50 user units).
    let small_pdf = page_pdf_extra(
        content,
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.0005 0 0 0.0005 0 0]", glyph),
    );
    let (doc_s, page_s) = open_page(small_pdf);
    let small = render(&doc_s, &page_s);

    // Larger font matrix (0.001 → glyph cell ~100 user units).
    let big_pdf = page_pdf_extra(
        content,
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.001 0 0 0.001 0 0]", glyph),
    );
    let (doc_b, page_b) = open_page(big_pdf);
    let big = render(&doc_b, &page_b);

    let n_small = dark_count(&small);
    let n_big = dark_count(&big);
    assert!(n_small > 0 && n_big > 0, "both render some pixels");
    // 2x the matrix scale → ~4x the area.
    assert!(
        n_big > n_small * 2,
        "larger FontMatrix must cover more pixels (small={n_small}, big={n_big})"
    );
}

// ============================================================================
// RENDER-TYPE3-003: an uncolored (d1) glyph uses the current text fill color.
// ============================================================================

#[test]
fn render_type3_003_d1_uses_current_fill_color() {
    // d1 marks the glyph "uncolored": the proc may NOT set color, and the
    // current fill color (set by `rg`) is used. Here the page sets red.
    let glyph = b"1000 0 0 0 1000 1000 d1 100 100 800 800 re f";
    let content = b"1 0 0 rg BT /T3 100 Tf 50 50 Td (A) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.001 0 0 0.001 0 0]", glyph),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    // The glyph must be painted in red (the current fill color), not black.
    assert!(
        color_count(&pm, (255, 0, 0)) > 1000,
        "d1 glyph painted in the current fill color (red)"
    );
    assert_eq!(
        dark_count(&pm),
        0,
        "no black pixels: d1 ignores any proc color"
    );
}

// ============================================================================
// RENDER-TYPE3-004: a colored (d0) glyph sets its own color in the proc.
// ============================================================================

#[test]
fn render_type3_004_d0_uses_proc_color() {
    // d0 marks the glyph "colored": the proc sets its own color (green here),
    // overriding the page fill color (red).
    let glyph = b"1000 0 d0 0 1 0 rg 100 100 800 800 re f";
    let content = b"1 0 0 rg BT /T3 100 Tf 50 50 Td (A) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.001 0 0 0.001 0 0]", glyph),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(
        color_count(&pm, (0, 255, 0)) > 1000,
        "d0 glyph painted in its own (green) color"
    );
    assert_eq!(
        color_count(&pm, (255, 0, 0)),
        0,
        "no red: the proc color overrides the page fill"
    );
}

// ============================================================================
// RENDER-TYPE3-005: a missing CharProc entry draws nothing, never panics.
// ============================================================================

#[test]
fn render_type3_005_missing_charproc_no_draw() {
    // The encoding maps code 65 → /Achar, but CharProcs has no /Achar entry.
    let extra = vec![
        (
            10,
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
              /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs 11 0 R \
              /Encoding 12 0 R /FirstChar 65 /LastChar 65 /Widths [1000] \
              /Resources << >> >>"
                .to_vec(),
        ),
        (11, b"<< >>".to_vec()), // empty CharProcs
        (
            12,
            b"<< /Type /Encoding /Differences [65 /Achar] >>".to_vec(),
        ),
    ];
    let content = b"BT /T3 100 Tf 50 50 Td (A) Tj ET";
    let pdf = page_pdf_extra(content, "<< /Font << /T3 10 0 R >> >>", extra);
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    // Nothing painted: the page stays blank white.
    assert_eq!(dark_count(&pm), 0, "missing CharProc paints nothing");
    assert_eq!(
        color_count(&pm, (255, 255, 255)),
        (pm.width * pm.height) as usize,
        "page is entirely white"
    );
}

// ============================================================================
// RENDER-TYPE3-006: the Td origin positions the Type3 glyph.
// ============================================================================

#[test]
fn render_type3_006_origin_positions_glyph() {
    let glyph = b"1000 0 d0 0 0 400 400 re f";
    // Two pages: glyph placed bottom-left vs top-right.
    let left_pdf = page_pdf_extra(
        b"BT /T3 100 Tf 10 10 Td (A) Tj ET",
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.001 0 0 0.001 0 0]", glyph),
    );
    let (doc_l, page_l) = open_page(left_pdf);
    let left = render(&doc_l, &page_l);

    let right_pdf = page_pdf_extra(
        b"BT /T3 100 Tf 150 150 Td (A) Tj ET",
        "<< /Font << /T3 10 0 R >> >>",
        type3_font_objs("[0.001 0 0 0.001 0 0]", glyph),
    );
    let (doc_r, page_r) = open_page(right_pdf);
    let right = render(&doc_r, &page_r);

    // Bottom-left placement: user (10..50, 10..50) → device bottom-left corner
    // (large device y). Top-right placement: user (150..190) → device top-right.
    // Sample a device pixel in the bottom-left quadrant: dark only for `left`.
    assert!(
        px(&left, 25, 175) != (255, 255, 255),
        "left glyph paints in the bottom-left device quadrant"
    );
    assert_eq!(
        px(&right, 25, 175),
        (255, 255, 255),
        "right-placed glyph leaves the bottom-left quadrant white"
    );
    // And the top-right quadrant is painted only for `right`.
    assert!(
        px(&right, 170, 30) != (255, 255, 255),
        "right glyph paints in the top-right device quadrant"
    );
}

// ============================================================================
// RENDER-TYPE3-007: a Type3 glyph proc that itself shows Type3 text terminates
// (recursion guard) and never panics.
// ============================================================================

#[test]
fn render_type3_007_recursive_proc_no_panic() {
    // The glyph proc shows the same Type3 font (self-reference). The recursion
    // guard must stop it; the render must complete without panicking.
    let glyph = b"1000 0 d0 BT /T3 50 Tf 0 0 Td (A) Tj ET 0 0 500 500 re f";
    let extra = vec![
        (
            10,
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
              /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs 11 0 R \
              /Encoding 12 0 R /FirstChar 65 /LastChar 65 /Widths [1000] \
              /Resources << /Font << /T3 10 0 R >> >> >>"
                .to_vec(),
        ),
        (11, b"<< /Achar 13 0 R >>".to_vec()),
        (
            12,
            b"<< /Type /Encoding /Differences [65 /Achar] >>".to_vec(),
        ),
        (13, stream("", glyph)),
    ];
    let content = b"BT /T3 100 Tf 50 50 Td (A) Tj ET";
    let pdf = page_pdf_extra(content, "<< /Font << /T3 10 0 R >> >>", extra);
    let (doc, page) = open_page(pdf);
    // Must not panic, and must paint the outer box at least.
    let pm = render(&doc, &page);
    assert!(dark_count(&pm) > 0, "recursive Type3 still paints the box");
}
