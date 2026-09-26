//! `RENDER-TRANSP-*` — transparency, soft masks, blend modes and clipping of
//! images / shadings (PDF 32000-1 §11). Every test builds a self-contained
//! classic-xref PDF and asserts on rendered pixel values (200 × 200 pt page at
//! 1 pt = 1 px; device y = 200 − PDF y).

use std::sync::Arc;

use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_image::pixmap::Pixmap;
use pdf_render::{get_svg_image, render_page, RenderOptions, SvgOptions};
use pdf_text::{interpret_page_render, BlendMode, ContentInterpreter, RenderOp};

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

/// A Form XObject stream; `group` adds `/Group << /S /Transparency … >>`.
fn form(bbox: &str, group: Option<&str>, res: &str, content: &[u8]) -> Vec<u8> {
    let group = group.map_or(String::new(), |g| {
        format!("/Group << /Type /Group /S /Transparency {g} >>")
    });
    stream(
        &format!("/Type /XObject /Subtype /Form /BBox {bbox} {group} /Resources {res}"),
        content,
    )
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

/// Asserts each channel of `got` is within `tol` of `want`.
#[track_caller]
fn assert_near(got: (u8, u8, u8), want: (u8, u8, u8), tol: u8) {
    let ok = got.0.abs_diff(want.0) <= tol
        && got.1.abs_diff(want.1) <= tol
        && got.2.abs_diff(want.2) <= tol;
    assert!(ok, "got {got:?}, want {want:?} ±{tol}");
}

const WHITE: (u8, u8, u8) = (255, 255, 255);
const RED: (u8, u8, u8) = (255, 0, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);
/// Red at alpha 0.5 over white.
const HALF_RED: (u8, u8, u8) = (255, 128, 128);

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

// ============================================================================
// RENDER-TRANSP-GROUP-* : transparency-group Form XObjects.
// ============================================================================

/// Two overlapping red squares (a 60 pt overlap around (100, 100)).
const OVERLAP: &[u8] = b"1 0 0 rg 20 20 120 120 re f 60 60 120 120 re f";

/// RENDER-TRANSP-GROUP-ALPHA: the group composites as a whole with the `ca` in
/// effect at `Do`; overlapping content inside does not double-darken.
#[test]
fn group_composites_as_a_whole_with_outer_alpha() {
    let pm = render(
        b"/GS0 gs /Fm0 Do",
        "<< /ExtGState << /GS0 << /ca 0.5 >> >> /XObject << /Fm0 30 0 R >> >>",
        vec![(30, form("[0 0 200 200]", Some(""), "<< >>", OVERLAP))],
    );
    assert_near(px(&pm, 100, 100), HALF_RED, 2);
    assert_near(px(&pm, 30, 170), HALF_RED, 2);
    assert_eq!(px(&pm, 5, 5), WHITE);
}

/// RENDER-TRANSP-GROUP-RESET: `gs` inside the group sets alpha relative to the
/// group (reset to 1 at its start); the group alpha still applies on top —
/// the USGS translucent-box case that used to render opaque.
#[test]
fn group_inner_alpha_does_not_override_group_alpha() {
    let pm = render(
        b"/GS0 gs /Fm0 Do",
        "<< /ExtGState << /GS0 << /ca 0.5 >> >> /XObject << /Fm0 30 0 R >> >>",
        vec![(
            30,
            form(
                "[0 0 200 200]",
                Some(""),
                "<< /ExtGState << /GSi << /ca 1 >> >> >>",
                b"/GSi gs 1 0 0 rg 0 0 200 200 re f",
            ),
        )],
    );
    assert_near(px(&pm, 100, 100), HALF_RED, 2);
}

/// RENDER-TRANSP-FORM-INHERIT: a plain (non-group) form keeps per-object alpha
/// inherited from its caller, so overlapping content does darken.
#[test]
fn non_group_form_inherits_per_object_alpha() {
    let pm = render(
        b"/GS0 gs /Fm0 Do",
        "<< /ExtGState << /GS0 << /ca 0.5 >> >> /XObject << /Fm0 30 0 R >> >>",
        vec![(30, form("[0 0 200 200]", None, "<< >>", OVERLAP))],
    );
    assert_near(px(&pm, 30, 170), HALF_RED, 2);
    assert_near(px(&pm, 100, 100), (255, 64, 64), 2);
}

/// RENDER-TRANSP-GROUP-BBOX: a group's content is clipped to its `/BBox`.
#[test]
fn group_content_is_clipped_to_bbox() {
    let pm = render(
        b"/Fm0 Do",
        "<< /XObject << /Fm0 30 0 R >> >>",
        vec![(
            30,
            form(
                "[0 0 100 200]",
                Some(""),
                "<< >>",
                b"1 0 0 rg 0 0 200 200 re f",
            ),
        )],
    );
    assert_eq!(px(&pm, 50, 100), RED);
    assert_eq!(px(&pm, 150, 100), WHITE);
}

/// RENDER-TRANSP-GROUP-ISOLATED: an isolated group blends its content against
/// transparency, so a Multiply inside it does not see the page backdrop; a
/// non-isolated opaque group paints straight onto the page.
#[test]
fn isolated_group_blends_against_transparency() {
    let content = b"1 1 0 rg 0 0 200 200 re f /Fm0 Do";
    let res = "<< /XObject << /Fm0 30 0 R >> >>";
    let inner = b"/GSm gs 0 1 1 rg 0 0 200 200 re f";
    let inner_res = "<< /ExtGState << /GSm << /BM /Multiply >> >> >>";
    let isolated = render(
        content,
        res,
        vec![(30, form("[0 0 200 200]", Some("/I true"), inner_res, inner))],
    );
    assert_eq!(px(&isolated, 100, 100), (0, 255, 255));
    let plain = render(
        content,
        res,
        vec![(30, form("[0 0 200 200]", Some(""), inner_res, inner))],
    );
    assert_eq!(px(&plain, 100, 100), (0, 255, 0));
}

/// RENDER-TRANSP-GROUP-UNBALANCED: a stray `Q` inside a group cannot pop the
/// caller's clip.
#[test]
fn unbalanced_restore_inside_group_keeps_outer_clip() {
    let pm = render(
        b"q 0 0 100 200 re W n /GS0 gs /Fm0 Do Q",
        "<< /ExtGState << /GS0 << /ca 1 >> >> /XObject << /Fm0 30 0 R >> >>",
        vec![(
            30,
            form(
                "[0 0 200 200]",
                Some("/I true"),
                "<< >>",
                b"Q Q 1 0 0 rg 0 0 200 200 re f",
            ),
        )],
    );
    assert_eq!(px(&pm, 50, 100), RED);
    assert_eq!(px(&pm, 150, 100), WHITE);
}

// ============================================================================
// RENDER-TRANSP-SMASK-* : ExtGState soft masks.
// ============================================================================

/// An ExtGState soft mask dict body (object 40 is the mask group form).
fn smask_gs(subtype: &str, extra: &str) -> String {
    format!("<< /SMask << /Type /Mask /S /{subtype} /G 40 0 R {extra} >> >>")
}

/// A DeviceGray mask group filling the left half with gray level `g`.
fn left_half_mask(g: &str) -> Vec<u8> {
    form(
        "[0 0 200 200]",
        Some("/CS /DeviceGray"),
        "<< >>",
        format!("{g} g 0 0 100 200 re f").as_bytes(),
    )
}

const BLUE_PAGE: &[u8] = b"0 0 1 rg 0 0 200 200 re f";

fn with_gs(prefix: &[u8]) -> Vec<u8> {
    let mut c = prefix.to_vec();
    c.push(b' ');
    c.extend_from_slice(BLUE_PAGE);
    c
}

/// RENDER-TRANSP-SMASK-LUMINOSITY: a luminosity mask passes paint where the
/// group is white; outside the group the (default black) backdrop masks out.
#[test]
fn luminosity_soft_mask_masks_later_paint() {
    let res = format!("<< /ExtGState << /GS1 {} >> >>", smask_gs("Luminosity", ""));
    let pm = render(&with_gs(b"/GS1 gs"), &res, vec![(40, left_half_mask("1"))]);
    assert_eq!(px(&pm, 50, 100), BLUE);
    assert_eq!(px(&pm, 150, 100), WHITE);
}

/// RENDER-TRANSP-SMASK-BC: the `/BC` backdrop colour sets the mask value where
/// the group paints nothing.
#[test]
fn luminosity_soft_mask_uses_backdrop_colour() {
    let res = format!(
        "<< /ExtGState << /GS1 {} >> >>",
        smask_gs("Luminosity", "/BC [1]")
    );
    let pm = render(&with_gs(b"/GS1 gs"), &res, vec![(40, left_half_mask("0"))]);
    assert_eq!(px(&pm, 50, 100), WHITE);
    assert_eq!(px(&pm, 150, 100), BLUE);
}

/// RENDER-TRANSP-SMASK-ALPHA: an alpha mask uses the group's coverage/opacity.
#[test]
fn alpha_soft_mask_uses_group_alpha() {
    let res = format!("<< /ExtGState << /GS1 {} >> >>", smask_gs("Alpha", ""));
    let mask = form(
        "[0 0 200 200]",
        Some(""),
        "<< /ExtGState << /GSa << /ca 0.5 >> >> >>",
        b"/GSa gs 0 0 100 200 re f",
    );
    let pm = render(&with_gs(b"/GS1 gs"), &res, vec![(40, mask)]);
    assert_near(px(&pm, 50, 100), (128, 128, 255), 2);
    assert_eq!(px(&pm, 150, 100), WHITE);
}

/// RENDER-TRANSP-SMASK-TR: a `/TR` transfer function remaps mask values.
#[test]
fn soft_mask_transfer_function_is_applied() {
    let res = format!(
        "<< /ExtGState << /GS1 {} >> >>",
        smask_gs(
            "Luminosity",
            "/TR << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>"
        )
    );
    let pm = render(&with_gs(b"/GS1 gs"), &res, vec![(40, left_half_mask("1"))]);
    assert_eq!(px(&pm, 50, 100), WHITE, "inverted: white group masks out");
    assert_eq!(px(&pm, 150, 100), BLUE, "inverted: black backdrop passes");
}

/// RENDER-TRANSP-SMASK-SCOPE: the soft mask is graphics state — `Q` restores
/// the previous (absent) mask and `/SMask /None` clears it.
#[test]
fn soft_mask_is_scoped_and_cleared() {
    let res = format!(
        "<< /ExtGState << /GS1 {} /GS2 << /SMask /None >> >> >>",
        smask_gs("Luminosity", "")
    );
    for prefix in [&b"q /GS1 gs Q"[..], b"/GS1 gs /GS2 gs"] {
        let pm = render(&with_gs(prefix), &res, vec![(40, left_half_mask("1"))]);
        assert_eq!(px(&pm, 150, 100), BLUE);
    }
}

/// RENDER-TRANSP-SMASK-GROUP: a soft mask in effect at a group `Do` masks the
/// composited group (the USGS feathered-shadow pattern).
#[test]
fn soft_mask_applies_to_group_composite() {
    let res = format!(
        "<< /ExtGState << /GS1 {} >> /XObject << /Fm0 30 0 R >> >>",
        smask_gs("Luminosity", "")
    );
    let pm = render(
        b"/GS1 gs /Fm0 Do",
        &res,
        vec![
            (40, left_half_mask("1")),
            (
                30,
                form(
                    "[0 0 200 200]",
                    Some(""),
                    "<< >>",
                    b"1 0 0 rg 0 0 200 200 re f",
                ),
            ),
        ],
    );
    assert_eq!(px(&pm, 50, 100), RED);
    assert_eq!(px(&pm, 150, 100), WHITE);
}

/// RENDER-TRANSP-SMASK-OPS: the mask group is recorded inside the soft-mask op
/// (not the page stream), and its text never enters the page's inventory.
#[test]
fn soft_mask_group_is_recorded_privately() {
    let res = format!(
        "<< /ExtGState << /GS1 {} >> /Font << /F1 50 0 R >> >>",
        smask_gs("Luminosity", "")
    );
    let mask = form(
        "[0 0 200 200]",
        Some(""),
        "<< /Font << /F1 50 0 R >> >>",
        b"1 g BT /F1 12 Tf 10 10 Td (MASK) Tj ET",
    );
    let font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec();
    let pdf = page_pdf(
        b"/GS1 gs BT /F1 12 Tf 10 100 Td (PAGE) Tj ET",
        &res,
        vec![(40, mask), (50, font)],
    );
    let (doc, page) = open_page(pdf);
    let dict = page.dict().expect("page dict");
    let ops = interpret_page_render(&doc, &dict);
    let masks: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            RenderOp::SoftMask(Some(m)) => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(masks.len(), 1);
    assert!(masks[0].luminosity);
    assert!(masks[0]
        .ops
        .iter()
        .any(|op| matches!(op, RenderOp::Text(_))));
    let page_texts = ops
        .iter()
        .filter(|op| matches!(op, RenderOp::Text(_)))
        .count();
    assert_eq!(page_texts, 1, "only the page's own text run");

    let recording = ContentInterpreter::new_recording(&doc).run_page_recorded(&dict);
    let text: String = recording
        .content
        .glyphs
        .iter()
        .map(|g| g.unicode.as_str())
        .collect();
    assert_eq!(text, "PAGE");
    assert_eq!(recording.text_ops.len(), 1);
    assert!(matches!(
        recording.ops.get(recording.text_ops[0].0),
        Some(RenderOp::Text(_))
    ));
}

// ============================================================================
// RENDER-TRANSP-BLEND / TEXT / SVG
// ============================================================================

/// RENDER-TRANSP-BLEND-MULTIPLY: `/BM /Multiply` multiplies with the backdrop,
/// and is restored by `Q`.
#[test]
fn multiply_blend_mode_is_applied_and_scoped() {
    let pm = render(
        b"1 1 0 rg 0 0 200 200 re f q /GSm gs 0 1 1 rg 50 50 100 100 re f Q \
          0 1 1 rg 0 0 20 20 re f",
        "<< /ExtGState << /GSm << /BM /Multiply >> >> >>",
        vec![],
    );
    assert_eq!(px(&pm, 100, 100), (0, 255, 0));
    assert_eq!(px(&pm, 10, 190), (0, 255, 255), "Q restores Normal");
    assert_eq!(px(&pm, 190, 10), (255, 255, 0));
}

/// RENDER-TRANSP-BLEND-ARRAY: `/BM` may be an array; the first known name wins.
#[test]
fn blend_mode_array_uses_first_known_name() {
    let pdf = page_pdf(
        b"/GSm gs",
        "<< /ExtGState << /GSm << /BM [/Bogus /Screen /Multiply] >> >> >>",
        vec![],
    );
    let (doc, page) = open_page(pdf);
    let ops = interpret_page_render(&doc, &page.dict().expect("page dict"));
    assert!(matches!(
        ops.as_slice(),
        [RenderOp::BlendMode(BlendMode::Screen)]
    ));
}

/// RENDER-TRANSP-TEXT-ALPHA: text honors the fill alpha `ca`.
#[test]
fn text_fill_honors_constant_alpha() {
    let pm = render(
        b"/GS0 gs BT /F1 180 Tf 60 30 Td (I) Tj ET",
        "<< /ExtGState << /GS0 << /ca 0.5 >> >> \
           /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>",
        vec![],
    );
    let darkest = pm
        .samples()
        .chunks_exact(pm.n as usize)
        .map(|c| c[0])
        .min()
        .expect("pixels");
    assert!(
        (125..=131).contains(&darkest),
        "half-alpha black text, got darkest {darkest}"
    );
}

/// RENDER-TRANSP-SVG-GROUP: SVG export wraps a translucent group in an
/// `opacity` group and keeps the group's content opaque.
#[test]
fn svg_wraps_translucent_group_in_opacity() {
    let pdf = page_pdf(
        b"/GS0 gs /Fm0 Do",
        "<< /ExtGState << /GS0 << /ca 0.5 >> >> /XObject << /Fm0 30 0 R >> >>",
        vec![(30, form("[0 0 200 200]", Some(""), "<< >>", OVERLAP))],
    );
    let (doc, page) = open_page(pdf);
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg");
    assert!(svg.contains("<g opacity=\"0.502\">"), "{svg}");
    assert!(!svg.contains("fill-opacity"), "{svg}");
    assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
}

/// Guards the recording shape the replay / SVG consumers rely on: a group is
/// bracketed by `BeginGroup` … `EndGroup` carrying the caller alpha and bbox.
#[test]
fn group_ops_carry_alpha_and_bbox() {
    let pdf = page_pdf(
        b"/GS0 gs /Fm0 Do",
        "<< /ExtGState << /GS0 << /ca 0.25 >> >> /XObject << /Fm0 30 0 R >> >>",
        vec![(
            30,
            form(
                "[0 0 100 50]",
                Some("/I true /K true"),
                "<< >>",
                b"0 0 10 10 re f",
            ),
        )],
    );
    let (doc, page) = open_page(pdf);
    let ops = interpret_page_render(&doc, &page.dict().expect("page dict"));
    let [RenderOp::BeginGroup(g), RenderOp::Fill { alpha, .. }, RenderOp::EndGroup] =
        ops.as_slice()
    else {
        panic!("unexpected ops {ops:?}");
    };
    assert_eq!(g.alpha, 64);
    assert!(g.isolated && g.knockout);
    let bbox = g.bbox.expect("bbox").rect();
    assert_eq!(
        (bbox.x0, bbox.y0, bbox.x1, bbox.y1),
        (0.0, 0.0, 100.0, 50.0)
    );
    assert_eq!(*alpha, 255, "group content starts from alpha 1");
}
