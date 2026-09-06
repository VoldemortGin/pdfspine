//! `RENDER-PAGE-*` — additional full-page render coverage (M6d).
//!
//! These extend `render_page.rs` into the replay paths its suite does not reach:
//! the `sh` shading operator (axial / radial, several colorspaces, `/Extend`, and
//! the deferred / no-op branches), stencil `/ImageMask` painting with `/Decode`
//! inversion, `/SMask` soft-mask resolution, alpha-suppressed images, dashed and
//! hairline strokes, and a non-substitutable non-embedded font. Every test builds
//! a self-contained classic-xref PDF and asserts on rendered pixel values.

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

/// A stream object body: `<< dict /Length N >>\nstream\n…\nendstream`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(format!("<< {} /Length {} >>\nstream\n", dict, data.len()).as_bytes());
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

fn open_page(bytes: Vec<u8>) -> (Arc<DocumentStore>, Page) {
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open pdf");
    let arc = Arc::new(doc);
    let page = Page::new(arc.clone(), 0, ObjRef::new(3, 0));
    (arc, page)
}

const MEDIA: &str = "[0 0 200 200]";

/// A single-page PDF whose content is `content`, resources `res`, plus `extra`
/// indirect objects (obj 1 catalog, 2 pages, 3 page, 4 content).
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

fn render(doc: &DocumentStore, page: &Page) -> Pixmap {
    render_page(doc, page, &RenderOptions::default()).expect("render_page ok")
}

fn px(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let v = pm.pixel(x, y).expect("pixel in range");
    (v[0], v[1], v[2])
}

fn non_blank(pm: &Pixmap) -> bool {
    let n = pm.n as usize;
    pm.samples()
        .chunks_exact(n)
        .any(|c| c[0] != 255 || c[1] != 255 || c[2] != 255)
}

fn is_white(c: (u8, u8, u8)) -> bool {
    c == (255, 255, 255)
}

// ============================================================================
// RENDER-PAGE-SHADE-* : the `sh` shading operator (draw_shading_op path).
// ============================================================================

/// RENDER-PAGE-SHADE-AXIAL: an axial (type 2) DeviceRGB shading with `/Extend`
/// paints a red→blue horizontal gradient across the page.
#[test]
fn render_page_shade_axial_rgb_gradient() {
    let shading = b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
        /Extend [true true] \
        /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>"
        .to_vec();
    let content = b"/Sh1 sh";
    let pdf = page_pdf_extra(
        content,
        "<< /Shading << /Sh1 20 0 R >> >>",
        vec![(20, shading)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(non_blank(&pm), "axial shading paints the page");
    let (lr, _, lb) = px(&pm, 5, 100);
    let (rr, _, rb) = px(&pm, 195, 100);
    assert!(lr > lb, "left endpoint red-dominant, got r={lr} b={lb}");
    assert!(rb > rr, "right endpoint blue-dominant, got r={rr} b={rb}");
}

/// RENDER-PAGE-SHADE-RADIAL: a radial (type 3) DeviceCMYK shading paints a dark
/// center (K=1) fading out; the corner outside the outer circle is untouched.
#[test]
fn render_page_shade_radial_cmyk() {
    let shading = b"<< /ShadingType 3 /ColorSpace /DeviceCMYK /Coords [100 100 0 100 100 100] \
        /Function << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 1] /C1 [0 0 0 0] /N 1 >> >>"
        .to_vec();
    let content = b"/Sh1 sh";
    let pdf = page_pdf_extra(
        content,
        "<< /Shading << /Sh1 20 0 R >> >>",
        vec![(20, shading)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    let center = px(&pm, 100, 100);
    assert!(
        center.0 < 128 && center.1 < 128 && center.2 < 128,
        "radial center is dark (K=1 ink), got {center:?}"
    );
    assert!(
        is_white(px(&pm, 5, 5)),
        "corner outside the outer circle stays white"
    );
}

/// RENDER-PAGE-SHADE-CS-ARRAY: a shading whose `/ColorSpace` is an ARRAY
/// (`[/CalGray …]`) resolves via the array branch to a Gray ramp (black→white).
#[test]
fn render_page_shade_colorspace_array_gray() {
    let shading = b"<< /ShadingType 2 /ColorSpace [/CalGray << /WhitePoint [1 1 1] >>] \
        /Coords [0 0 200 0] /Extend [false false] \
        /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> >>"
        .to_vec();
    let content = b"/Sh1 sh";
    let pdf = page_pdf_extra(
        content,
        "<< /Shading << /Sh1 20 0 R >> >>",
        vec![(20, shading)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(non_blank(&pm), "gray shading paints the page");
    let left = px(&pm, 5, 100);
    assert!(
        left.0 < 128 && left.0 == left.1 && left.1 == left.2,
        "left endpoint is dark neutral gray, got {left:?}"
    );
}

/// RENDER-PAGE-SHADE-NOFUNC: a shading dict without a `/Function` is a clean
/// no-op (deferred), leaving the page blank.
#[test]
fn render_page_shade_missing_function_noop() {
    let shading = b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] >>".to_vec();
    let content = b"/Sh1 sh";
    let pdf = page_pdf_extra(
        content,
        "<< /Shading << /Sh1 20 0 R >> >>",
        vec![(20, shading)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(!non_blank(&pm), "no /Function → nothing painted");
}

/// RENDER-PAGE-SHADE-UNSUPPORTED: a shading type the renderer defers (type 1) with
/// a valid function is a clean no-op (not an error), leaving the page blank.
#[test]
fn render_page_shade_unsupported_type_noop() {
    let shading = b"<< /ShadingType 1 /ColorSpace /DeviceRGB \
        /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>"
        .to_vec();
    let content = b"/Sh1 sh";
    let pdf = page_pdf_extra(
        content,
        "<< /Shading << /Sh1 20 0 R >> >>",
        vec![(20, shading)],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(!non_blank(&pm), "deferred shading type 1 paints nothing");
}

// ============================================================================
// RENDER-PAGE-IMG-* : stencil ImageMask, /Decode, /SMask, alpha suppression.
// ============================================================================

/// The single `(num, body)` entry for an 8×8 stencil `/ImageMask` XObject (all
/// bits 0). With the default `/Decode [0 1]` every bit-0 sample paints; `decode`
/// lets a test add `/Decode [1 0]` to invert that.
fn imagemask_pair(num: u32, decode: &str) -> (u32, Vec<u8>) {
    let data = vec![0u8; 8]; // 8 rows × 1 byte, all zero.
    let body = stream(
        &format!("/Type /XObject /Subtype /Image /Width 8 /Height 8 /ImageMask true /BitsPerComponent 1{decode}"),
        &data,
    );
    (num, body)
}

/// RENDER-PAGE-IMG-MASK: an `/ImageMask` paints the current fill color where the
/// stencil bit is 0 (the default `/Decode [0 1]`).
#[test]
fn render_page_imagemask_paints_fill_color() {
    let content = b"1 0 0 rg q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        vec![imagemask_pair(20, "")],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert_eq!(
        px(&pm, 100, 100),
        (255, 0, 0),
        "stencil paints the red fill"
    );
}

/// RENDER-PAGE-IMG-MASK-DECODE: `/Decode [1 0]` inverts the stencil — the same
/// all-zero bits now paint nowhere, leaving the page blank.
#[test]
fn render_page_imagemask_decode_inverted_blank() {
    let content = b"1 0 0 rg q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        vec![imagemask_pair(20, " /Decode [1 0]")],
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(
        !non_blank(&pm),
        "/Decode [1 0] inverts: all-zero bits paint nothing"
    );
}

/// A 1×1 solid-red RGB image (obj 20) with a 1×1 gray `/SMask` (obj 21) of value
/// `alpha`. `alpha = 0` → fully transparent; `alpha = 255` → fully opaque.
fn image_with_smask(alpha: u8) -> Vec<(u32, Vec<u8>)> {
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /SMask 21 0 R",
        &[255u8, 0, 0],
    );
    let smask = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8",
        &[alpha],
    );
    vec![(20, img), (21, smask)]
}

/// RENDER-PAGE-IMG-SMASK-OPAQUE: an `/SMask` of 255 keeps the image opaque → red.
#[test]
fn render_page_image_smask_opaque() {
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        image_with_smask(255),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert_eq!(px(&pm, 100, 100), (255, 0, 0), "opaque SMask → red shows");
}

/// RENDER-PAGE-IMG-SMASK-TRANSPARENT: an `/SMask` of 0 makes the image fully
/// transparent → the white background shows through.
#[test]
fn render_page_image_smask_transparent() {
    let content = b"q 200 0 0 200 0 0 cm /Im0 Do Q";
    let pdf = page_pdf_extra(
        content,
        "<< /XObject << /Im0 20 0 R >> >>",
        image_with_smask(0),
    );
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(
        is_white(px(&pm, 100, 100)),
        "SMask 0 → image transparent, page white"
    );
}

/// RENDER-PAGE-IMG-ALPHA0: an image drawn under a `ca 0` ExtGState is suppressed
/// entirely (alpha 0 short-circuit), leaving the page blank.
#[test]
fn render_page_image_alpha_zero_suppressed() {
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8",
        &[255u8, 0, 0],
    );
    let content = b"q /GS0 gs 200 0 0 200 0 0 cm /Im0 Do Q";
    let res = "<< /XObject << /Im0 20 0 R >> /ExtGState << /GS0 << /ca 0 >> >> >>";
    let pdf = page_pdf_extra(content, res, vec![(20, img)]);
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    assert!(!non_blank(&pm), "ca 0 → image not drawn");
}

// ============================================================================
// RENDER-PAGE-STROKE-* : dash-array parsing, hairline width.
// ============================================================================

/// Counts non-white (inked) pixels in a pixmap.
fn inked_pixels(pm: &Pixmap) -> usize {
    let n = pm.n as usize;
    pm.samples()
        .chunks_exact(n)
        .filter(|c| c[0] != 255 || c[1] != 255 || c[2] != 255)
        .count()
}

/// RENDER-PAGE-STROKE-DASH: a dashed stroke (`[12 8] 3 d`) paints a broken line —
/// exercises the dash-array + phase parse. The line inks, but with gaps, so it
/// covers fewer pixels than the same solid line would.
#[test]
fn render_page_dashed_stroke_inks_with_gaps() {
    let dashed = b"q 6 w [12 8] 3 d 1 0 0 RG 10 100 m 190 100 l S Q";
    let (doc, page) = open_page(page_pdf_extra(dashed, "<< >>", Vec::new()));
    let inked_dashed = inked_pixels(&render(&doc, &page));
    let solid = b"q 6 w 1 0 0 RG 10 100 m 190 100 l S Q";
    let (doc2, page2) = open_page(page_pdf_extra(solid, "<< >>", Vec::new()));
    let inked_solid = inked_pixels(&render(&doc2, &page2));
    assert!(inked_dashed > 0, "dashed line inks some segments");
    assert!(
        inked_dashed < inked_solid,
        "dashed ({inked_dashed}) covers fewer pixels than solid ({inked_solid})"
    );
}

/// RENDER-PAGE-STROKE-DASH-ZERO: an all-zero dash array (`[0 0] 0 d`) is treated
/// as solid (no gaps), so it inks like an undashed stroke.
#[test]
fn render_page_zero_dash_is_solid() {
    let content = b"q 6 w [0 0] 0 d 1 0 0 RG 10 100 m 190 100 l S Q";
    let (doc, page) = open_page(page_pdf_extra(content, "<< >>", Vec::new()));
    let pm = render(&doc, &page);
    // A solid red line crosses x at y≈100: sample its middle.
    assert_eq!(
        px(&pm, 100, 100),
        (255, 0, 0),
        "zero-dash line is solid red"
    );
}

/// RENDER-PAGE-STROKE-HAIRLINE: a zero-width stroke (`0 w`) renders as a
/// 1-device-pixel hairline (never invisible).
#[test]
fn render_page_hairline_stroke_inks() {
    let content = b"q 0 w 0 0 0 RG 10 100 m 190 100 l S Q";
    let (doc, page) = open_page(page_pdf_extra(content, "<< >>", Vec::new()));
    let pm = render(&doc, &page);
    assert!(
        inked_pixels(&pm) > 0,
        "zero-width hairline still inks the line"
    );
}

// ============================================================================
// RENDER-PAGE-FONT-NOSUB: a non-embedded font with no standard-14 substitute.
// ============================================================================

/// RENDER-PAGE-FONT-NOSUB: a non-embedded `/Type1` font whose `/BaseFont` matches
/// no standard-14 family (and has no `/FontFile*`) yields no substitute program —
/// the text draws nothing, but the page still renders (with its rect).
#[test]
fn render_page_non_substitutable_font_renders_blank_text() {
    let content = b"1 0 0 rg 10 10 40 40 re f \
                    BT /F1 40 Tf 40 100 Td (Hello) Tj ET";
    let font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Bogus-Nonstd \
                 /Encoding /WinAnsiEncoding >>"
        .to_vec();
    let pdf = page_pdf_extra(content, "<< /Font << /F1 10 0 R >> >>", vec![(10, font)]);
    let (doc, page) = open_page(pdf);
    let pm = render(&doc, &page);
    // The red rect proves the page rendered; the unsubstitutable text is blank.
    assert_eq!(px(&pm, 20, 175), (255, 0, 0), "rect rendered");
}
