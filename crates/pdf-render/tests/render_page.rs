//! `RENDER-PAGE-*` / `DISPLAYLIST-*` — full-page render integration tests (M6d).
//!
//! Each test builds a **self-contained** single-page PDF in raw bytes (classic
//! xref), opens it as a `DocumentStore`, and renders via the public
//! `pdf_render::render_page` / `DisplayList`. The embedded TrueType program is the
//! authored box font reused from `render_text.rs` (license-clean, self-contained).

use std::sync::Arc;

use pdf_core::geom::{IRect, Matrix};
use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_render::{render_page, DisplayList, RenderOptions};

mod synth;

// ============================================================================
// Minimal classic-xref PDF builder.
// ============================================================================

/// A classic-xref PDF assembler from `(obj_num, body_bytes)` entries; obj 1 is
/// the catalog. Mirrors the raw-bytes builders used across the workspace tests.
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
/// (object 3, per the builder convention below).
fn open_page(bytes: Vec<u8>) -> (Arc<DocumentStore>, Page) {
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open pdf");
    let arc = Arc::new(doc);
    let page = Page::new(arc.clone(), 0, ObjRef::new(3, 0));
    (arc, page)
}

// ============================================================================
// Page assemblers (catalog 1, pages 2, page 3, then content/resources).
// ============================================================================

const MEDIA: &str = "[0 0 200 200]";

/// A page whose content is `content`, with a `/Resources` dict literal `res`.
fn page_pdf(content: &[u8], res: &str, rotate: i32) -> Vec<u8> {
    page_pdf_extra(content, res, rotate, Vec::new())
}

/// As [`page_pdf`] but with extra indirect objects appended (e.g. font/image).
fn page_pdf_extra(content: &[u8], res: &str, rotate: i32, extra: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let rot = if rotate != 0 {
        format!(" /Rotate {rotate}")
    } else {
        String::new()
    };
    let mut pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox {MEDIA}{rot} \
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

/// The RGB(A) sample at device pixel `(x, y)` of a Pixmap (panics out of range).
fn px(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let v = pm.pixel(x, y).expect("pixel in range");
    (v[0], v[1], v[2])
}

/// Whether any pixel differs from opaque white (a non-blank page).
fn non_blank(pm: &Pixmap) -> bool {
    let n = pm.n as usize;
    pm.samples()
        .chunks_exact(n)
        .any(|c| c[0] != 255 || c[1] != 255 || c[2] != 255)
}

fn render(doc: &DocumentStore, page: &Page, opts: &RenderOptions) -> Pixmap {
    render_page(doc, page, opts).expect("render_page ok")
}

// ============================================================================
// RENDER-PAGE-001: a text page renders to a non-blank pixmap of the right size.
// ============================================================================

/// An embedded TrueType simple font (resource `/F1`) using objects 10 (font),
/// 11 (descriptor), 12 (FontFile2). Maps `'A'` → the box glyph.
fn embedded_font_objs() -> Vec<(u32, Vec<u8>)> {
    let ttf = synth::ttf();
    vec![
        (
            10,
            b"<< /Type /Font /Subtype /TrueType /BaseFont /BoxFont \
              /FirstChar 65 /LastChar 65 /Widths [1000] \
              /FontDescriptor 11 0 R /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ),
        (
            11,
            b"<< /Type /FontDescriptor /FontName /BoxFont /Flags 4 \
              /FontBBox [100 0 900 700] /ItalicAngle 0 /Ascent 800 /Descent -200 \
              /CapHeight 700 /StemV 80 /FontFile2 12 0 R >>"
                .to_vec(),
        ),
        (12, stream("/Length1 0", &ttf)),
    ]
}

#[test]
fn render_page_001_text_page_non_blank_sized() {
    // Big text so it covers pixels; place it within the 200x200 box.
    let content = b"BT /F1 100 Tf 20 60 Td (A) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> >>",
        0,
        embedded_font_objs(),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    // CropBox is 200x200, scale 1 → 200x200 output.
    assert_eq!((pm.width, pm.height), (200, 200));
    assert_eq!(pm.colorspace, Colorspace::Rgb);
    assert!(non_blank(&pm), "text page must not be blank");
}

// ============================================================================
// RENDER-PAGE-STD14-FALLBACK (P1-1): a NON-embedded standard-14 font (Helvetica,
// no /FontFile*) now renders real glyphs via the bundled Liberation substitute
// — body text is no longer blank. Before P1-1, resolve_font_program returned
// None for such a font and draw_text early-returned (blank page).
// ============================================================================

/// A non-embedded `/Type1` `/Helvetica` simple font (resource `/F1`): objects
/// 10 (font) + 11 (descriptor), with NO `/FontFile*` program.
fn non_embedded_helvetica_objs() -> Vec<(u32, Vec<u8>)> {
    vec![
        (
            10,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
              /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ),
        (
            11,
            b"<< /Type /FontDescriptor /FontName /Helvetica /Flags 32 \
              /FontBBox [-166 -225 1000 931] /ItalicAngle 0 /Ascent 718 \
              /Descent -207 /CapHeight 718 /StemV 88 >>"
                .to_vec(),
        ),
    ]
}

#[test]
fn render_page_std14_non_embedded_helvetica_not_blank() {
    // Large text covering many pixels; printable ASCII resolves through the
    // Liberation Sans cmap by Unicode.
    let content = b"BT /F1 100 Tf 20 80 Td (Hello) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> >>",
        0,
        non_embedded_helvetica_objs(),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!((pm.width, pm.height), (200, 200));
    // The substitute outlines must paint *some* ink: pre-P1-1 this page was
    // entirely blank (no embedded program → no glyphs drawn).
    assert!(
        non_blank(&pm),
        "non-embedded Helvetica body text must render glyphs (Liberation fallback)"
    );
    // Count painted (non-white) pixels: a real glyph run covers a meaningful
    // area, not a stray pixel.
    let n = pm.n as usize;
    let inked = pm
        .samples()
        .chunks_exact(n)
        .filter(|c| c[0] != 255 || c[1] != 255 || c[2] != 255)
        .count();
    assert!(
        inked > 100,
        "expected substantial glyph coverage, got {inked}"
    );
}

// ============================================================================
// RENDER-PAGE-TYPE1 (P4-2): an embedded Adobe Type1 (`/FontFile`) simple font
// now rasterizes real glyph outlines through the first-party `type1` outliner.
// Pre-P4-2 `resolve_font_program` skipped `/FontFile` (Type1 PFB not parseable
// by ttf-parser) → the page was blank for Type1-only text. This proves it inks.
// ============================================================================

/// An embedded `/Type1` simple font (resource `/F1`) backed by a synthetic
/// Type1 `/FontFile` program (objects 10 font, 11 descriptor, 12 FontFile).
/// Maps `'A'` (WinAnsi) → the box glyph named `A`.
fn embedded_type1_objs() -> Vec<(u32, Vec<u8>)> {
    let t1 = synth::type1();
    let len = t1.len();
    vec![
        (
            10,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /BoxT1 \
              /FirstChar 65 /LastChar 65 /Widths [700] \
              /FontDescriptor 11 0 R /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ),
        (
            11,
            b"<< /Type /FontDescriptor /FontName /BoxT1 /Flags 4 \
              /FontBBox [50 0 650 700] /ItalicAngle 0 /Ascent 700 /Descent 0 \
              /CapHeight 700 /StemV 80 /FontFile 12 0 R >>"
                .to_vec(),
        ),
        // Flat (uncompressed) Type1 program. /Length1 = cleartext, /Length2 =
        // binary, /Length3 = trailer; the renderer hands the raw bytes to the
        // type1 outliner regardless, so exact segment lengths are not critical.
        (
            12,
            stream(&format!("/Length1 {len} /Length2 0 /Length3 0"), &t1),
        ),
    ]
}

#[test]
fn render_page_type1_embedded_glyph_pixels() {
    // Black 'A' at size 120, origin user (20,40): the box glyph fills the cell.
    let content = b"BT /F1 120 Tf 20 40 Td (A) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> >>",
        0,
        embedded_type1_objs(),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!((pm.width, pm.height), (200, 200));
    // The embedded Type1 box glyph must paint many dark pixels — pre-P4-2 this
    // page was entirely blank (FontFile skipped → no outline).
    let n = pm.n as usize;
    let dark = pm
        .samples()
        .chunks_exact(n)
        .filter(|c| c[0] < 128 && c[1] < 128 && c[2] < 128)
        .count();
    assert!(
        dark > 200,
        "embedded Type1 glyph must paint many dark pixels (got {dark})"
    );
}

// ============================================================================
// RENDER-PAGE-002: a filled rect paints its fill color; white elsewhere.
// ============================================================================

#[test]
fn render_page_002_filled_rect_color() {
    // Red rect from (50,50) to (150,150) in user space (y-up).
    let content = b"1 0 0 rg 50 50 100 100 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let pm = render(&doc, &page, &RenderOptions::default());
    // Device y-flip: user (100,100) → device (100, 100) (center of the rect).
    assert_eq!(px(&pm, 100, 100), (255, 0, 0), "rect interior is red");
    // A corner well outside the rect stays white.
    assert_eq!(
        px(&pm, 10, 10),
        (255, 255, 255),
        "outside the rect is white"
    );
}

// ============================================================================
// RENDER-PAGE-003: z-order — a later fill paints over an earlier one.
// ============================================================================

#[test]
fn render_page_003_zorder_later_over_earlier() {
    // Red rect, then a green rect overlapping it: the overlap is green.
    let content = b"1 0 0 rg 40 40 120 120 re f \
                    0 1 0 rg 40 40 120 120 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!(
        px(&pm, 100, 100),
        (0, 255, 0),
        "later green paints over red"
    );
}

// ============================================================================
// RENDER-PAGE-004: embedded TTF glyph pixels appear where the text sits.
// ============================================================================

#[test]
fn render_page_004_text_glyph_pixels() {
    // Black 'A' at size 120, origin user (20,40). Box glyph covers ~ the cell.
    let content = b"BT /F1 120 Tf 20 40 Td (A) Tj ET";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> >>",
        0,
        embedded_font_objs(),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    // The box glyph fills a large region; count dark pixels.
    let n = pm.n as usize;
    let dark = pm
        .samples()
        .chunks_exact(n)
        .filter(|c| c[0] < 128 && c[1] < 128 && c[2] < 128)
        .count();
    assert!(
        dark > 200,
        "embedded glyph must paint many dark pixels (got {dark})"
    );
}

// ============================================================================
// RENDER-PAGE-005: text + vector + image composed; each region present.
// ============================================================================

/// A 2x2 raw RGB image XObject (object `num`): top-left red, others blue.
fn rgb_image_objs(num: u32) -> Vec<(u32, Vec<u8>)> {
    // 2x2, row-major RGB: (R)(B)/(B)(B).
    let data = [
        255u8, 0, 0, 0, 0, 255, // row 0
        0, 0, 255, 0, 0, 255, // row 1
    ];
    vec![(
        num,
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
             /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &data,
        ),
    )]
}

#[test]
fn render_page_005_text_vector_image_composed() {
    // green rect bottom-left; image top-right; black text middle.
    let content = b"0 1 0 rg 10 10 60 60 re f \
                    q 80 0 0 80 110 110 cm /Im0 Do Q \
                    0 0 0 rg BT /F1 60 Tf 20 100 Td (A) Tj ET";
    let mut extra = embedded_font_objs();
    extra.extend(rgb_image_objs(20));
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> /XObject << /Im0 20 0 R >> >>",
        0,
        extra,
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());

    // green rect region (device y-flip: user (40,40) → device (40,160)).
    assert_eq!(px(&pm, 40, 160), (0, 255, 0), "green rect present");
    // image region: placed at user (110..190, 110..190) → device (110..190, 10..90).
    // The image is mostly blue; sample its center.
    let (r, g, b) = px(&pm, 150, 50);
    assert!(
        b > 150 && r < 100,
        "image (blue-ish) present, got ({r},{g},{b})"
    );
    // text dark pixels somewhere in the upper-left text band.
    let n = pm.n as usize;
    let dark = pm
        .samples()
        .chunks_exact(n)
        .filter(|c| c[0] < 80 && c[1] < 80 && c[2] < 80)
        .count();
    assert!(dark > 50, "text present (dark pixels = {dark})");
}

// ============================================================================
// RENDER-PAGE-006: dpi / matrix scale changes output dimensions.
// ============================================================================

#[test]
fn render_page_006_scale_changes_dimensions() {
    let (doc, page) = open_page(page_pdf(b"1 0 0 rg 0 0 200 200 re f", "<< >>", 0));

    let pm1 = render(&doc, &page, &RenderOptions::default());
    assert_eq!((pm1.width, pm1.height), (200, 200));

    let opts_dpi = RenderOptions {
        dpi: Some(144),
        ..RenderOptions::default()
    };
    let pm2 = render(&doc, &page, &opts_dpi);
    assert_eq!((pm2.width, pm2.height), (400, 400), "144dpi → 2x");

    let opts_m = RenderOptions {
        matrix: Matrix::scale(3.0, 3.0),
        ..RenderOptions::default()
    };
    let pm3 = render(&doc, &page, &opts_m);
    assert_eq!((pm3.width, pm3.height), (600, 600), "matrix 3x");
}

// ============================================================================
// RENDER-PAGE-007: /Rotate 90 swaps width/height.
// ============================================================================

#[test]
fn render_page_007_rotate_swaps_dims() {
    // MediaBox is square here, so make it explicit via a wide content + rotate.
    // Use a non-square page to make the swap observable: override MediaBox.
    let pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] /Rotate 90 \
              /Resources << >> /Contents 4 0 R >>",
        )
        .obj(4, stream("", b"1 0 0 rg 0 0 100 200 re f"))
        .build();
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    // 100x200 page rotated 90° → displayed 200x100.
    assert_eq!((pm.width, pm.height), (200, 100), "rotate swaps dims");
    assert!(non_blank(&pm));
}

// ============================================================================
// RENDER-PAGE-008: a W n clip restricts a following fill.
// ============================================================================

#[test]
fn render_page_008_clip_restricts_fill() {
    // Clip to the left half (0..100), then fill the whole page red.
    let content = b"0 0 100 200 re W n \
                    1 0 0 rg 0 0 200 200 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!(px(&pm, 50, 100), (255, 0, 0), "inside clip is red");
    assert_eq!(
        px(&pm, 150, 100),
        (255, 255, 255),
        "outside clip stays white"
    );
}

// ============================================================================
// RENDER-PAGE-009: an image XObject paints its colors at its placement.
// ============================================================================

#[test]
fn render_page_009_image_paints_colors() {
    // 1x1 solid green image scaled to fill the page.
    let data = [0u8, 255, 0];
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8",
        &data,
    );
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        0,
        vec![(20, img)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!(px(&pm, 100, 100), (0, 255, 0), "image green fills the page");
}

// ============================================================================
// RENDER-PAGE-010: a vector page renders (no Unsupported); non-blank.
// ============================================================================

#[test]
fn render_page_010_vector_page_renders() {
    let content = b"0 0 1 rg 0 0 200 200 re f \
                    1 1 0 RG 5 w 20 20 m 180 180 l S";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let r = render_page(&doc, &page, &RenderOptions::default());
    assert!(r.is_ok(), "vector page must render, not raise Unsupported");
    assert!(non_blank(&r.unwrap()));
}

// ============================================================================
// RENDER-PAGE-011: alpha=true output is 4-channel with a transparent bg.
// ============================================================================

#[test]
fn render_page_011_alpha_channel() {
    let content = b"1 0 0 rg 50 50 100 100 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let opts = RenderOptions {
        alpha: true,
        ..RenderOptions::default()
    };
    let pm = render(&doc, &page, &opts);
    assert!(pm.alpha, "alpha output requested");
    assert_eq!(pm.n, 4);
    // A corner outside the rect is transparent (alpha 0).
    let corner = pm.pixel(5, 5).unwrap();
    assert_eq!(corner[3], 0, "background transparent under alpha");
    // The rect interior is opaque red.
    let inside = pm.pixel(100, 100).unwrap();
    assert_eq!((inside[0], inside[3]), (255, 255), "rect opaque red");
}

// ============================================================================
// RENDER-PAGE-012: non-embedded font → no glyph pixels but page still renders.
// ============================================================================

#[test]
fn render_page_012_non_embedded_font_no_glyphs() {
    // A Helvetica simple font with NO FontFile* — text is laid out but the
    // outline pipeline has nothing to rasterize (documented gap).
    let content = b"1 0 0 rg 10 10 30 30 re f \
                    BT /F1 40 Tf 50 100 Td (Hello) Tj ET";
    let font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                 /Encoding /WinAnsiEncoding >>"
        .to_vec();
    let pdf = page_pdf_extra(content, "<< /Font << /F1 10 0 R >> >>", 0, vec![(10, font)]);
    let (doc, page) = open_page(pdf);
    let r = render_page(&doc, &page, &RenderOptions::default());
    assert!(r.is_ok(), "page with non-embedded font still renders");
    let pm = r.unwrap();
    // The red rect proves the page rendered; we don't assert glyph pixels.
    assert_eq!(px(&pm, 20, 175), (255, 0, 0), "rect rendered");
}

// ============================================================================
// RENDER-PAGE-CS-* (P3-3): Indexed / Separation / DeviceN colorspaces + /Decode.
//
// Each test builds a synthetic single-page PDF exercising one colorspace feature
// and asserts the *rendered* pixel colors. The expected colors are the ones real
// fitz produces for the same construct (palette lookup / tint transform run, dark
// spot ink ≠ white, /Decode honored), cross-checked against `.venv-oracle`.
// ============================================================================

// RENDER-PAGE-CS-SEP-VECTOR (sub-problem 1): a 1-component Separation fill whose
// tint transform maps tint 1.0 → a DARK CMYK ink must render DARK, not white.
// Pre-P3-3 `scn` mapped tint 1.0 → gray(1.0) = white (the reported bug).
#[test]
fn render_page_cs_separation_vector_dark_not_white() {
    // /Sep1: Separation over DeviceCMYK; tint transform t -> [0 0 0 t] (black ink).
    // Fill a rect at tint 1.0 → must be black (0,0,0), NOT white.
    let res = "<< /ColorSpace << /Sep1 20 0 R >> >>";
    let content = b"/Sep1 cs 1 scn 50 50 100 100 re f";
    let sep = b"[/Separation /Spot1 /DeviceCMYK 21 0 R]".to_vec();
    let func = b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] \
                 /C1 [0 0 0 1] /N 1 >>"
        .to_vec();
    let pdf = page_pdf_extra(content, res, 0, vec![(20, sep), (21, func)]);
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!(
        px(&pm, 100, 100),
        (0, 0, 0),
        "dark spot-color fill renders dark, not white"
    );
    // Outside the rect stays white.
    assert_eq!(px(&pm, 10, 10), (255, 255, 255));
}

// RENDER-PAGE-CS-SEP-VECTOR-RGB: a Separation over DeviceRGB whose tint 1.0 maps
// to a specific RGB (a colored spot ink), proving the alternate space is honored.
#[test]
fn render_page_cs_separation_vector_rgb_alt() {
    // tint t -> [0.2*t 0.4*t 0.8*t]; at tint 1.0 → (51,102,204).
    let res = "<< /ColorSpace << /Sep1 20 0 R >> >>";
    let content = b"/Sep1 cs 1 scn 50 50 100 100 re f";
    let sep = b"[/Separation /Blue /DeviceRGB 21 0 R]".to_vec();
    let func = b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0] \
                 /C1 [0.2 0.4 0.8] /N 1 >>"
        .to_vec();
    let pdf = page_pdf_extra(content, res, 0, vec![(20, sep), (21, func)]);
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    // 0.2*255=51, 0.4*255=102, 0.8*255=204.
    assert_eq!(
        px(&pm, 100, 100),
        (51, 102, 204),
        "tint transform → RGB alt"
    );
}

// RENDER-PAGE-CS-INDEXED-IMAGE (sub-problem 2): an Indexed image must render the
// palette colors, not the raw indices. Pre-P3-3 it rendered index bytes as gray.
#[test]
fn render_page_cs_indexed_image_palette_lookup() {
    // 4x1 image, 8 bpc indices [0,0,1,1]; palette: 0=red(255,0,0), 1=green
    // (0,255,0). Two solid texels per color so bilinear sampling at x=50/x=150
    // stays inside a pure region. Indexed base DeviceRGB, hival 1, lookup=6 bytes.
    let data = [0u8, 0u8, 1u8, 1u8];
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
         /ColorSpace [/Indexed /DeviceRGB 1 <FF0000 00FF00>] /BitsPerComponent 8",
        &data,
    );
    // Map the 2x1 image across the page: left half = index 0, right half = index 1.
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        0,
        vec![(20, img)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    // Left column → palette entry 0 (red); right column → entry 1 (green).
    // Sample inside each solid 2-texel region (avoids the bilinear seam at the
    // exact midpoint). Left half → palette red; right half → palette green.
    assert_eq!(px(&pm, 50, 100), (255, 0, 0), "indexed left = palette red");
    assert_eq!(
        px(&pm, 150, 100),
        (0, 255, 0),
        "indexed right = palette green"
    );
}

// RENDER-PAGE-CS-SEP-IMAGE (sub-problem 3): a Separation image must run the tint
// transform per pixel (tint → alternate → RGB), not render the raw tint as gray.
#[test]
fn render_page_cs_separation_image_tint_transform() {
    // 4x1 image, 8 bpc tints [0,0,255,255] → normalized [0.0,0.0,1.0,1.0]. Tint
    // transform t -> [0 0 0 t] over DeviceCMYK: tint 0 → white, tint 1 → black.
    let data = [0u8, 0u8, 255u8, 255u8];
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
         /ColorSpace [/Separation /Spot1 /DeviceCMYK 21 0 R] /BitsPerComponent 8",
        &data,
    );
    let func = b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] \
                 /C1 [0 0 0 1] /N 1 >>"
        .to_vec();
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        0,
        vec![(20, img), (21, func)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!(px(&pm, 50, 100), (255, 255, 255), "tint 0 → white");
    assert_eq!(px(&pm, 150, 100), (0, 0, 0), "tint 1 → black ink");
}

// RENDER-PAGE-CS-DEVICEN-IMAGE (sub-problem 3): a 2-colorant DeviceN image runs
// the N-input tint transform per pixel.
#[test]
fn render_page_cs_devicen_image_tint_transform() {
    // 4x1 image, 2 components/pixel, 8 bpc. Texels 0-1 = (0,0) → black; texels
    // 2-3 = (255,255) → white, via a 2x2x3 sampled tint transform over DeviceRGB.
    let data = [0u8, 0u8, 0u8, 0u8, 255u8, 255u8, 255u8, 255u8];
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
         /ColorSpace [/DeviceN [/C1 /C2] /DeviceRGB 21 0 R] /BitsPerComponent 8",
        &data,
    );
    // Type-0 sampled: 2 inputs, size [2 2], 3 outputs, 8 bps.
    // table order (row-major over inputs): (0,0)=black,(0,1)=red,(1,0)=green,(1,1)=white
    let samples = [
        0u8, 0, 0, 255, 0, 0, // input0=0: (0,0),(0,1)
        0, 255, 0, 255, 255, 255, // input0=1: (1,0),(1,1)
    ];
    let func = stream(
        "/FunctionType 0 /Domain [0 1 0 1] /Range [0 1 0 1 0 1] \
         /Size [2 2] /BitsPerSample 8",
        &samples,
    );
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        0,
        vec![(20, img), (21, func)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    assert_eq!(px(&pm, 50, 100), (0, 0, 0), "DeviceN (0,0) → black");
    assert_eq!(px(&pm, 150, 100), (255, 255, 255), "DeviceN (1,1) → white");
}

// RENDER-PAGE-CS-DECODE-IMAGE (sub-problem 4): a /Decode [1 0] on a DeviceGray
// image inverts the samples generally (not only inside DCT). A 0x00 sample with
// /Decode [1 0] decodes to white; 0xFF decodes to black.
#[test]
fn render_page_cs_decode_inverts_gray_image() {
    // 4x1 DeviceGray, 8 bpc: [0,0,255,255]. With /Decode [1 0]: 0→1.0 (white),
    // 255→0.0 (black) — the inverse of the default. Two solid texels per region.
    let data = [0u8, 0u8, 255u8, 255u8];
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
         /ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [1 0]",
        &data,
    );
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        0,
        vec![(20, img)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page, &RenderOptions::default());
    // Without /Decode, left would be black & right white; /Decode [1 0] swaps.
    assert_eq!(
        px(&pm, 50, 100),
        (255, 255, 255),
        "/Decode inverts: 0 → white"
    );
    assert_eq!(px(&pm, 150, 100), (0, 0, 0), "/Decode inverts: 255 → black");
}

// ============================================================================
// RENDER-PAGE-PROP-001: arbitrary content never panics; always a Pixmap.
// ============================================================================

#[test]
fn render_page_prop_001_arbitrary_content_no_panic() {
    let cases: &[&[u8]] = &[
        b"",
        b"q q q",
        b"garbage tokens 1 2 3 BT ET Tj",
        b"0 0 0 0 0 0 cm 1 0 0 rg 0 0 50 50 re f",
        b"BT /Missing 12 Tf (x) Tj ET",
        b"W n f S B sh /X Do",
        b"99999999 0 0 99999999 0 0 cm 0 0 1 1 re f",
    ];
    for c in cases {
        let (doc, page) = open_page(page_pdf(c, "<< >>", 0));
        let r = render_page(&doc, &page, &RenderOptions::default());
        assert!(r.is_ok(), "content {:?} must render without error", c);
    }
}

// ============================================================================
// DISPLAYLIST-001..004
// ============================================================================

#[test]
fn displaylist_001_records_ops() {
    let content = b"1 0 0 rg 50 50 100 100 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let dl = DisplayList::from_page(&doc, &page);
    assert!(!dl.is_empty(), "display list records the fill op");
}

#[test]
fn displaylist_002_replay_matches_render_page() {
    let content = b"0 0 1 rg 20 20 160 160 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let opts = RenderOptions::default();
    let direct = render(&doc, &page, &opts);
    let dl = DisplayList::from_page(&doc, &page);
    let replayed = dl.get_pixmap(&doc, &opts).expect("dl pixmap");
    assert_eq!(
        (direct.width, direct.height),
        (replayed.width, replayed.height)
    );
    assert_eq!(
        direct.samples(),
        replayed.samples(),
        "replay is pixel-equal"
    );
}

#[test]
fn displaylist_003_replay_at_two_scales() {
    let (doc, page) = open_page(page_pdf(b"1 0 0 rg 0 0 200 200 re f", "<< >>", 0));
    let dl = DisplayList::from_page(&doc, &page);
    let a = dl.get_pixmap(&doc, &RenderOptions::default()).unwrap();
    let b = dl
        .get_pixmap(
            &doc,
            &RenderOptions {
                dpi: Some(144),
                ..RenderOptions::default()
            },
        )
        .unwrap();
    assert_eq!((a.width, a.height), (200, 200));
    assert_eq!((b.width, b.height), (400, 400));
}

#[test]
fn displaylist_004_rect_is_cropbox() {
    let (doc, page) = open_page(page_pdf(b"", "<< >>", 0));
    let dl = DisplayList::from_page(&doc, &page);
    let r = dl.rect();
    assert_eq!((r.x0, r.y0, r.x1, r.y1), (0.0, 0.0, 200.0, 200.0));
}

// ============================================================================
// RENDER-PAGE clip-irect option.
// ============================================================================

#[test]
fn render_page_clip_irect_subrect() {
    // Fill the whole page red; render only the device sub-rect [50,50,150,150].
    let (doc, page) = open_page(page_pdf(b"1 0 0 rg 0 0 200 200 re f", "<< >>", 0));
    let opts = RenderOptions {
        clip: Some(IRect::new(50, 50, 150, 150)),
        ..RenderOptions::default()
    };
    let pm = render(&doc, &page, &opts);
    assert_eq!((pm.width, pm.height), (100, 100), "clip sizes the target");
    assert_eq!(px(&pm, 10, 10), (255, 0, 0), "clip window is red");
}
