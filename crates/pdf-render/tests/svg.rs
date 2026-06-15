//! `SVG-*` — page → standalone SVG document export (M7).
//!
//! Each test builds a **self-contained** single-page PDF in raw bytes (classic
//! xref), opens it as a `DocumentStore`, and exports via the public
//! `pdf_render::get_svg_image`. The embedded TrueType program is the authored
//! box font reused from `render_page.rs` (license-clean, self-contained).
//!
//! Well-formedness is checked with a small dependency-free XML scanner
//! ([`xml_well_formed`]) that validates tag nesting + attribute-quote balance —
//! enough to prove the serializer never emits broken markup.

use std::sync::Arc;

use pdf_core::geom::Matrix;
use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_render::{get_svg_image, SvgOptions};

mod synth;

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

fn page_pdf(content: &[u8], res: &str, rotate: i32) -> Vec<u8> {
    page_pdf_extra(content, res, rotate, Vec::new())
}

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

/// An embedded TrueType simple font (resource `/F1`); objects 10/11/12. Maps
/// `'A'` → the box glyph.
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

/// A 1x1 solid green RGB image XObject (object `num`).
fn green_image_objs(num: u32) -> Vec<(u32, Vec<u8>)> {
    let data = [0u8, 255, 0];
    vec![(
        num,
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
             /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &data,
        ),
    )]
}

// ============================================================================
// A tiny, dependency-free XML well-formedness scanner.
// ============================================================================

/// Validates that `s` is well-formed XML at the tag/attribute level:
/// - every `<tag …>` opens a matching `</tag>` (self-closing `/>` excepted),
/// - tags nest (a LIFO stack with matching names),
/// - the stack is empty at the end (all tags closed),
/// - attribute values use balanced double quotes,
/// - no stray `<`/`>` inside text or attribute values,
/// - the special chars `&`/`<` in text appear only as entities.
///
/// This is intentionally permissive (it is not a full XML parser) but rejects
/// the broken-markup mistakes the serializer could plausibly make.
fn xml_well_formed(s: &str) -> Result<(), String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'<' {
            // Comments / declarations / processing instructions.
            if s[i..].starts_with("<!--") {
                let end = s[i..].find("-->").ok_or("unterminated comment")?;
                i += end + 3;
                continue;
            }
            if s[i..].starts_with("<?") {
                let end = s[i..].find("?>").ok_or("unterminated PI")?;
                i += end + 2;
                continue;
            }
            if s[i..].starts_with("<!") {
                let end = s[i..].find('>').ok_or("unterminated declaration")?;
                i += end + 1;
                continue;
            }
            // Find the end of the tag, respecting quoted attribute values.
            let mut j = i + 1;
            let mut in_quote = false;
            while j < bytes.len() {
                let cj = bytes[j];
                if cj == b'"' {
                    in_quote = !in_quote;
                } else if cj == b'>' && !in_quote {
                    break;
                } else if cj == b'<' && !in_quote {
                    return Err(format!("nested '<' inside a tag at byte {j}"));
                }
                j += 1;
            }
            if j >= bytes.len() {
                return Err("unterminated tag".into());
            }
            if in_quote {
                return Err("unbalanced attribute quotes".into());
            }
            let inner = &s[i + 1..j]; // tag body without the angle brackets.
            if let Some(name) = inner.strip_prefix('/') {
                // Closing tag.
                let name = name.trim();
                match stack.pop() {
                    Some(top) if top == name => {}
                    Some(top) => {
                        return Err(format!("mismatched close </{name}> for <{top}>"));
                    }
                    None => return Err(format!("close </{name}> with empty stack")),
                }
            } else if inner.ends_with('/') {
                // Self-closing tag: no push.
            } else {
                // Open tag: push its element name (first whitespace-delimited token).
                let name = inner
                    .split(|ch: char| ch.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    return Err("empty tag name".into());
                }
                stack.push(name);
            }
            i = j + 1;
        } else if c == b'>' {
            return Err(format!("stray '>' at byte {i}"));
        } else if c == b'&' {
            // Must be a recognized entity reference (terminated by ';').
            let rest = &s[i..];
            let end = rest.find(';').ok_or("unterminated entity")?;
            let ent = &rest[1..end];
            let ok = matches!(ent, "amp" | "lt" | "gt" | "quot" | "apos") || ent.starts_with('#');
            if !ok {
                return Err(format!("unknown entity &{ent};"));
            }
            i += end + 1;
        } else {
            i += 1;
        }
    }
    if !stack.is_empty() {
        return Err(format!("unclosed tags: {stack:?}"));
    }
    Ok(())
}

#[test]
fn xml_checker_accepts_good_and_rejects_bad() {
    assert!(xml_well_formed("<a><b/></a>").is_ok());
    assert!(xml_well_formed(r#"<a x="1"><b y="z &amp; w"/></a>"#).is_ok());
    assert!(
        xml_well_formed("<a><b></a></b>").is_err(),
        "mismatched nesting"
    );
    assert!(xml_well_formed("<a>").is_err(), "unclosed");
    assert!(xml_well_formed("<a>&bogus;</a>").is_err(), "bad entity");
    assert!(
        xml_well_formed(r#"<a x="oops>"#).is_err(),
        "unbalanced quote"
    );
}

// ============================================================================
// SVG-BASIC-001: rect + glyph + image → well-formed SVG with expected pieces.
// ============================================================================

#[test]
fn svg_basic_001_rect_glyph_image() {
    // green rect bottom-left; image top-right; black text middle.
    let content = b"0 1 0 rg 10 10 60 60 re f \
                    q 80 0 0 80 110 110 cm /Im0 Do Q \
                    0 0 0 rg BT /F1 60 Tf 20 100 Td (A) Tj ET";
    let mut extra = embedded_font_objs();
    extra.extend(green_image_objs(20));
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> /XObject << /Im0 20 0 R >> >>",
        0,
        extra,
    );
    let (doc, page) = open_page(pdf);
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");

    // Parseable / well-formed.
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed svg: {e}\n{svg}"));

    // Standalone svg root with namespace + a viewBox matching the 200x200 page.
    assert!(svg.contains("<svg"), "has an <svg> root");
    assert!(
        svg.contains("xmlns=\"http://www.w3.org/2000/svg\""),
        "declares the SVG namespace"
    );
    assert!(
        svg.contains("viewBox=\"0 0 200 200\""),
        "viewBox matches page size, got:\n{svg}"
    );

    // The rect → a <path> filled green (#00ff00).
    assert!(svg.contains("<path"), "vector op → a <path>");
    assert!(
        svg.contains("#00ff00") || svg.to_lowercase().contains("rgb(0,255,0)"),
        "rect fill is green, got:\n{svg}"
    );

    // The glyph → a <path> (outline) or a <text> fallback.
    assert!(
        svg.contains("<text") || svg.matches("<path").count() >= 2,
        "glyph emitted as an outline <path> or a <text> fallback"
    );

    // The image → an <image href="data:image/png;base64,...">.
    assert!(
        svg.contains("<image") && svg.contains("data:image/png;base64,"),
        "image op → an inline data-URI <image>, got:\n{svg}"
    );
}

// ============================================================================
// SVG-BASIC-002: viewBox / width / height scale with the matrix.
// ============================================================================

#[test]
fn svg_basic_002_matrix_scales_viewport() {
    let (doc, page) = open_page(page_pdf(b"1 0 0 rg 0 0 200 200 re f", "<< >>", 0));
    let opts = SvgOptions {
        matrix: Matrix::scale(2.0, 2.0),
    };
    let svg = get_svg_image(&doc, &page, &opts).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    // 200pt page × 2 → 400 viewport.
    assert!(
        svg.contains("width=\"400\"") && svg.contains("height=\"400\""),
        "matrix 2x → 400x400 viewport, got:\n{svg}"
    );
    assert!(svg.contains("viewBox=\"0 0 400 400\""), "viewBox scaled");
}

// ============================================================================
// SVG-BASIC-003: a stroked vector emits stroke attributes.
// ============================================================================

#[test]
fn svg_basic_003_stroke_attrs() {
    let content = b"1 1 0 RG 5 w 20 20 m 180 180 l S";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    assert!(svg.contains("stroke="), "stroked op carries a stroke color");
    assert!(
        svg.contains("stroke-width="),
        "stroked op carries a stroke width"
    );
    // Yellow stroke.
    assert!(
        svg.contains("#ffff00") || svg.to_lowercase().contains("rgb(255,255,0)"),
        "stroke is yellow, got:\n{svg}"
    );
}

// ============================================================================
// SVG-BASIC-004: even-odd fill rule surfaces as fill-rule="evenodd".
// ============================================================================

#[test]
fn svg_basic_004_even_odd_fill_rule() {
    let content = b"1 0 0 rg 0 0 200 200 re 50 50 100 100 re f*";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    assert!(
        svg.contains("fill-rule=\"evenodd\""),
        "f* → evenodd fill rule, got:\n{svg}"
    );
}

// ============================================================================
// SVG-BASIC-005: a W n clip emits a <clipPath> + clip-path reference.
// ============================================================================

#[test]
fn svg_basic_005_clip_emits_clippath() {
    let content = b"0 0 100 200 re W n \
                    1 0 0 rg 0 0 200 200 re f";
    let (doc, page) = open_page(page_pdf(content, "<< >>", 0));
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    assert!(svg.contains("<clipPath"), "clip → a <clipPath> def");
    assert!(
        svg.contains("clip-path=\"url(#"),
        "clipped content references the clipPath, got:\n{svg}"
    );
}

// ============================================================================
// SVG-BASIC-006: a shading (sh) emits a gradient def.
// ============================================================================

#[test]
fn svg_basic_006_shading_gradient() {
    // An axial shading filling the page via the sh operator.
    let shading = "<< /ShadingType 2 /ColorSpace /DeviceRGB \
        /Coords [0 0 200 0] /Extend [true true] \
        /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>";
    let content = b"0 0 200 200 re W n /Sh0 sh";
    let res = format!("<< /Shading << /Sh0 {shading} >> >>");
    let (doc, page) = open_page(page_pdf(content, &res, 0));
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    assert!(
        svg.contains("<linearGradient") || svg.contains("<radialGradient"),
        "shading → a gradient def, got:\n{svg}"
    );
}

// ============================================================================
// SVG-EMPTY-001: a blank page → a valid empty <svg>.
// ============================================================================

#[test]
fn svg_empty_001_blank_page() {
    let (doc, page) = open_page(page_pdf(b"", "<< >>", 0));
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed empty svg: {e}\n{svg}"));
    assert!(svg.contains("<svg"), "still a valid svg root");
    assert!(svg.contains("viewBox=\"0 0 200 200\""), "sized to the page");
    assert!(
        !svg.contains("<path") && !svg.contains("<image") && !svg.contains("<text"),
        "no drawcalls for a blank page, got:\n{svg}"
    );
}

// ============================================================================
// SVG-EMPTY-002: a page with no /Contents (None dict path) → valid empty svg.
// ============================================================================

#[test]
fn svg_empty_002_no_contents() {
    let pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 90] >>")
        .build();
    let (doc, page) = open_page(pdf);
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    assert!(
        svg.contains("viewBox=\"0 0 120 90\""),
        "sized to its MediaBox, got:\n{svg}"
    );
}

// ============================================================================
// SVG-ESCAPE-001: XML special chars are escaped (here via a glyph mapping that
// would otherwise inject markup). We exercise the escaper through a <text>
// fallback path is hard to force, so escape the resource name into an attribute
// is not user-controlled; instead assert the serializer never leaks a raw
// special char by feeding content that produces an image whose stream is benign
// but verifying the document carries no unescaped lone '&'.
// ============================================================================

#[test]
fn svg_escape_001_no_unescaped_specials() {
    // A normal text + rect page; the produced SVG must contain no bare '&' that
    // is not part of an entity (the well-formedness scanner already enforces
    // this, but assert explicitly on the raw bytes for clarity).
    let content = b"0 0 0 rg BT /F1 40 Tf 20 100 Td (A) Tj ET 1 0 0 rg 0 0 20 20 re f";
    let pdf = page_pdf_extra(
        content,
        "<< /Font << /F1 10 0 R >> >>",
        0,
        embedded_font_objs(),
    );
    let (doc, page) = open_page(pdf);
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    // Every '&' begins a valid entity.
    for (idx, _) in svg.match_indices('&') {
        let rest = &svg[idx..];
        let end = rest.find(';').expect("entity terminated");
        let ent = &rest[1..end];
        assert!(
            matches!(ent, "amp" | "lt" | "gt" | "quot" | "apos") || ent.starts_with('#'),
            "every '&' is an entity, found &{ent};"
        );
    }
}

// ============================================================================
// SVG-ESCAPE-002: the <text> fallback escapes its content (non-embedded font).
// ============================================================================

#[test]
fn svg_escape_002_text_fallback_escaped() {
    // A non-embedded font with a string holding XML specials: the glyph-outline
    // path is unavailable (no FontFile*), so the serializer may fall back to
    // <text>; if it does, the content must be escaped. If it skips text
    // entirely (also a valid choice), the doc is simply well-formed.
    let content = b"BT /F1 20 Tf 10 100 Td (a<b&c>d\"e) Tj ET";
    let font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                 /Encoding /WinAnsiEncoding >>"
        .to_vec();
    let pdf = page_pdf_extra(content, "<< /Font << /F1 10 0 R >> >>", 0, vec![(10, font)]);
    let (doc, page) = open_page(pdf);
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}\n{svg}"));
    // The raw injected sequence must never appear verbatim.
    assert!(
        !svg.contains("a<b&c>d"),
        "raw special chars must be escaped, got:\n{svg}"
    );
}

// ============================================================================
// SVG-PROP-001: arbitrary content never panics and always yields well-formed
// SVG.
// ============================================================================

#[test]
fn svg_prop_001_arbitrary_content_well_formed() {
    let cases: &[&[u8]] = &[
        b"",
        b"q q q",
        b"garbage tokens 1 2 3 BT ET Tj",
        b"0 0 0 0 0 0 cm 1 0 0 rg 0 0 50 50 re f",
        b"BT /Missing 12 Tf (x) Tj ET",
        b"W n f S B sh /X Do",
        b"99999999 0 0 99999999 0 0 cm 0 0 1 1 re f",
        b"1 0 0 rg 10 10 50 50 re f 0 1 0 RG 2 w 0 0 m 100 100 l S",
        b"0 0 100 100 re W n 0 0 1 rg 0 0 200 200 re f*",
    ];
    for c in cases {
        let (doc, page) = open_page(page_pdf(c, "<< >>", 0));
        let svg = get_svg_image(&doc, &page, &SvgOptions::default())
            .unwrap_or_else(|e| panic!("content {c:?} must export, got err {e:?}"));
        xml_well_formed(&svg)
            .unwrap_or_else(|e| panic!("content {c:?} → malformed svg: {e}\n{svg}"));
    }
}

// ============================================================================
// SVG-PROP-002: rotation produces a correctly-sized (swapped) viewport.
// ============================================================================

#[test]
fn svg_prop_002_rotate_swaps_viewport() {
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
    let svg = get_svg_image(&doc, &page, &SvgOptions::default()).expect("svg ok");
    xml_well_formed(&svg).unwrap_or_else(|e| panic!("malformed: {e}"));
    // 100x200 page rotated 90° → displayed 200x100.
    assert!(
        svg.contains("viewBox=\"0 0 200 100\""),
        "rotate swaps the viewport, got:\n{svg}"
    );
}
