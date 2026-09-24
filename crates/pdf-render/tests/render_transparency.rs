//! `RENDER-TRANSP-*` — transparency, soft masks, blend modes and clipping of
//! images / shadings (PDF 32000-1 §11). Every test builds a self-contained
//! classic-xref PDF and asserts on rendered pixel values (200 × 200 pt page at
//! 1 pt = 1 px; device y = 200 − PDF y).

use std::sync::Arc;

use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_image::pixmap::Pixmap;
use pdf_render::{render_page, RenderOptions};

// ============================================================================
// Minimal classic-xref PDF builder (mirrors render_page_extra.rs).
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

/// A single-page PDF (obj 1 catalog, 2 pages, 3 page, 4 content) with page
/// resources `res` plus `extra` indirect objects.
fn page_pdf(content: &[u8], res: &str, extra: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
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

fn open_page(bytes: Vec<u8>) -> (Arc<DocumentStore>, Page) {
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open pdf");
    let arc = Arc::new(doc);
    let page = Page::new(arc.clone(), 0, ObjRef::new(3, 0));
    (arc, page)
}

fn render(content: &[u8], res: &str, extra: Vec<(u32, Vec<u8>)>) -> Pixmap {
    let (doc, page) = open_page(page_pdf(content, res, extra));
    render_page(&doc, &page, &RenderOptions::default()).expect("render_page ok")
}

fn px(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let v = pm.pixel(x, y).expect("pixel in range");
    (v[0], v[1], v[2])
}

const WHITE: (u8, u8, u8) = (255, 255, 255);
const RED: (u8, u8, u8) = (255, 0, 0);

/// A 1×1 opaque red RGB image XObject.
fn red_image() -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8",
        &[255u8, 0, 0],
    )
}

// ============================================================================
// RENDER-TRANSP-CLIP-* : images and shadings honor the clip path.
// ============================================================================

/// RENDER-TRANSP-CLIP-IMAGE: an image paints only inside a rectangular clip.
#[test]
fn image_is_clipped_to_clip_path() {
    let pm = render(
        b"q 50 50 100 100 re W n 200 0 0 200 0 0 cm /Im0 Do Q",
        "<< /XObject << /Im0 20 0 R >> >>",
        vec![(20, red_image())],
    );
    assert_eq!(px(&pm, 100, 100), RED);
    assert_eq!(px(&pm, 10, 10), WHITE, "outside the clip stays white");
    assert_eq!(px(&pm, 190, 100), WHITE);
}

/// RENDER-TRANSP-CLIP-ROUNDED: a rounded-corner clip built with `v` curves
/// (the USGS photo frame) cuts the image corners.
#[test]
fn image_is_clipped_to_rounded_corner_path() {
    let pm = render(
        b"q 60 180 m 20 180 20 140 v 20 60 l 20 20 60 20 v 140 20 l 180 20 180 60 v \
          180 140 l 180 180 140 180 v h W n 200 0 0 200 0 0 cm /Im0 Do Q",
        "<< /XObject << /Im0 20 0 R >> >>",
        vec![(20, red_image())],
    );
    assert_eq!(px(&pm, 100, 100), RED);
    assert_eq!(px(&pm, 22, 100), RED, "straight edge is inside");
    assert_eq!(px(&pm, 22, 22), WHITE, "rounded corner is cut");
    assert_eq!(px(&pm, 177, 177), WHITE, "rounded corner is cut");
}

/// RENDER-TRANSP-CLIP-RECT-SNAP: an axis-aligned rectangular clip keeps every
/// partially covered pixel whole, so content placed exactly inside it (the
/// usual image frame) shows no anti-aliased seam; non-rectangular clips stay
/// anti-aliased.
#[test]
fn rectangular_clip_is_pixel_snapped_outward() {
    let pm = render(
        b"q 0 0 100.4 200 re W n 1 0 0 rg 0 0 100.4 200 re f Q",
        "<< >>",
        vec![],
    );
    let unclipped = render(b"1 0 0 rg 0 0 100.4 200 re f", "<< >>", vec![]);
    assert_eq!(px(&pm, 99, 100), RED);
    // Column 100 is partly covered by the fill; the clip must not attenuate
    // the fill's own anti-aliased edge a second time.
    assert_ne!(px(&pm, 100, 100), WHITE);
    assert_eq!(pm.samples(), unclipped.samples());
    let tri = render(
        b"q 0 0 m 100.4 0 l 100.4 200 l h W n 1 0 0 rg 0 0 200 200 re f Q",
        "<< >>",
        vec![],
    );
    let (_, g, _) = px(&tri, 24, 150);
    assert!(g > 0 && g < 255, "diagonal clip edge stays anti-aliased");
}

/// RENDER-TRANSP-CLIP-SHADING: the `sh` operator fills only the clip region.
#[test]
fn shading_is_clipped_to_clip_path() {
    let shading = b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
        /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [1 0 0] /N 1 >> \
        /Extend [true true] >>";
    let pm = render(
        b"q 0 0 100 200 re W n /Sh0 sh Q",
        "<< /Shading << /Sh0 20 0 R >> >>",
        vec![(20, shading.to_vec())],
    );
    assert_eq!(px(&pm, 50, 100), RED);
    assert_eq!(px(&pm, 150, 100), WHITE, "shading must not cover the page");
}
