//! Document assembly: positioned draw ops → content streams → PDF bytes.
//!
//! The document is **built from scratch** (imagedoc-style seed + `pdf-core`
//! writer) rather than through `PageEditor`/`insert_text`, so each page gets
//! exactly one content stream and every font resource is registered once:
//!
//! - Base-14 faces become one shared `/Type1 … /WinAnsiEncoding` object each.
//! - The optional user / CJK-fallback TTFs are parsed **once**, their used
//!   glyphs accumulated across the whole document, and embedded **once** via
//!   [`pdf_edit::EmbeddedFont::write_type0`] (Type0 / Identity-H + ToUnicode) —
//!   never per text run.
//!
//! Output is deterministic: object allocation follows a fixed order, all maps
//! are ordered, and [`pdf_core::SaveOptions::default`] is the deterministic
//! table-xref baseline (no timestamps, content-hash `/ID`).

use std::collections::BTreeMap;

use pdf_core::error::Result;
use pdf_core::filters::flate;
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::{DocumentStore, Limits, SaveOptions, XrefStyle};

use crate::fonts::{winansi_byte, Face, FontSet};
use crate::images::PreparedImage;
use crate::layout::{Op, PageOps, Rgb};
use crate::Options;

/// The cubic-Bézier circle constant κ = 4/3·(√2 − 1) (same as `pdf-edit`).
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// Assembles the final PDF from the laid-out pages.
pub(crate) fn build_pdf(
    pages: &[PageOps],
    images: &[PreparedImage],
    fonts: &FontSet,
    opts: &Options,
) -> Result<Vec<u8>> {
    // Pass 1 — per-page face usage + whole-document used-glyph accumulation
    // for the embedded fonts (PRD §9: parse once, write_type0 once).
    let mut face_used = vec![[false; 7]; pages.len()];
    let mut used_user: BTreeMap<u16, char> = BTreeMap::new();
    let mut used_cjk: BTreeMap<u16, char> = BTreeMap::new();
    for (pi, page) in pages.iter().enumerate() {
        for op in &page.ops {
            if let Op::Text { face, text, .. } = op {
                face_used[pi][face.index()] = true;
                match face {
                    Face::User => {
                        if let Some(f) = &fonts.user {
                            for ch in text.chars() {
                                used_user.insert(f.gid(ch), ch);
                            }
                        }
                    }
                    Face::Cjk => {
                        if let Some(f) = &fonts.cjk {
                            for ch in text.chars() {
                                used_cjk.insert(f.gid(ch), ch);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Pass 2 — deterministic object creation.
    let doc = DocumentStore::from_bytes(empty_seed_pdf(), Limits::default())?;
    let pages_ref = ObjRef::new(2, 0);

    let mut font_refs: [Option<ObjRef>; 7] = [None; 7];
    let used_any = |face: Face| face_used.iter().any(|u| u[face.index()]);
    if used_any(Face::User) {
        if let Some(f) = &fonts.user {
            font_refs[Face::User.index()] = Some(f.font.write_type0(&doc, &used_user)?);
        }
    }
    if used_any(Face::Cjk) {
        if let Some(f) = &fonts.cjk {
            font_refs[Face::Cjk.index()] = Some(f.font.write_type0(&doc, &used_cjk)?);
        }
    }
    for face in Face::ALL {
        if let Some(std_name) = face.std_name() {
            if used_any(face) {
                font_refs[face.index()] = Some(doc.add_object(base14_font_object(std_name))?);
            }
        }
    }

    let mut kids: Vec<Object> = Vec::with_capacity(pages.len());
    for (pi, page) in pages.iter().enumerate() {
        // Image XObjects, in first-use order on the page.
        let mut xobjs: Vec<(usize, ObjRef)> = Vec::new();
        for op in &page.ops {
            if let Op::Image { id, .. } = op {
                if let Some(img) = images.get(*id) {
                    xobjs.push((*id, embed_image(&doc, img)?));
                }
            }
        }

        let content = emit_content(page, fonts, opts);
        let content_ref = doc.add_object(Object::Stream(StreamObj::new_encoded(
            Dict::from_iter([(Name::new("Length"), Object::Integer(content.len() as i64))]),
            content,
        )))?;

        let mut resources = Dict::new();
        let mut font_dict = Dict::new();
        for face in Face::ALL {
            if face_used[pi][face.index()] {
                if let Some(r) = font_refs[face.index()] {
                    font_dict.insert(Name::new(face.res_name()), Object::Reference(r));
                }
            }
        }
        if !font_dict.is_empty() {
            resources.insert(Name::new("Font"), Object::Dictionary(font_dict));
        }
        if !xobjs.is_empty() {
            let mut xdict = Dict::new();
            for (id, r) in &xobjs {
                xdict.insert(Name::new(format!("Im{id}")), Object::Reference(*r));
            }
            resources.insert(Name::new("XObject"), Object::Dictionary(xdict));
        }

        let mut leaf = Dict::new();
        leaf.insert(Name::new("Type"), Object::Name(Name::new("Page")));
        leaf.insert(Name::new("Parent"), Object::Reference(pages_ref));
        leaf.insert(
            Name::new("MediaBox"),
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(opts.page_width),
                Object::Real(opts.page_height),
            ]),
        );
        leaf.insert(Name::new("Contents"), Object::Reference(content_ref));
        leaf.insert(Name::new("Resources"), Object::Dictionary(resources));
        kids.push(Object::Reference(doc.add_object(Object::Dictionary(leaf))?));
    }

    let mut pages_dict = Dict::new();
    pages_dict.insert(Name::new("Type"), Object::Name(Name::new("Pages")));
    pages_dict.insert(Name::new("Count"), Object::Integer(kids.len() as i64));
    pages_dict.insert(Name::new("Kids"), Object::Array(kids));
    doc.update_object(pages_ref, Object::Dictionary(pages_dict))?;

    let opts = SaveOptions::default().with_xref_style(XrefStyle::Table);
    doc.save_to_vec(&opts)
}

/// A minimal, openable zero-page seed PDF (catalog + empty page tree), mirroring
/// the proven `pdf-image` seed. Object 1 = catalog, object 2 = `/Pages`.
fn empty_seed_pdf() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = [0usize; 2];
    offsets[0] = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets[1] = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
    let xref_pos = out.len();
    out.extend_from_slice(b"xref\n0 3\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    out.extend_from_slice(format!("{:010} 00000 n \n", offsets[0]).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", offsets[1]).as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{xref_pos}\n").as_bytes());
    out.extend_from_slice(b"%%EOF");
    out
}

/// A Base-14 `/Type1` font resource (no embedding, WinAnsi single-byte codes).
fn base14_font_object(std_name: &str) -> Object {
    let mut d = Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Type1")));
    d.insert(Name::new("BaseFont"), Object::Name(Name::new(std_name)));
    d.insert(
        Name::new("Encoding"),
        Object::Name(Name::new("WinAnsiEncoding")),
    );
    Object::Dictionary(d)
}

/// Embeds one prepared image as an `/XObject /Image` and returns its reference.
fn embed_image(doc: &DocumentStore, img: &PreparedImage) -> Result<ObjRef> {
    let mut d = Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Image")));
    d.insert(Name::new("BitsPerComponent"), Object::Integer(8));
    match img {
        PreparedImage::Jpeg {
            width,
            height,
            components,
            data,
        } => {
            d.insert(Name::new("Width"), Object::Integer(i64::from(*width)));
            d.insert(Name::new("Height"), Object::Integer(i64::from(*height)));
            let cs = match components {
                1 => "DeviceGray",
                4 => "DeviceCMYK",
                _ => "DeviceRGB",
            };
            d.insert(Name::new("ColorSpace"), Object::Name(Name::new(cs)));
            if *components == 4 {
                // Adobe CMYK JPEGs are stored inverted; flip via /Decode.
                d.insert(
                    Name::new("Decode"),
                    Object::Array(
                        [1, 0, 1, 0, 1, 0, 1, 0]
                            .iter()
                            .map(|v| Object::Integer(*v))
                            .collect(),
                    ),
                );
            }
            d.insert(Name::new("Filter"), Object::Name(Name::new("DCTDecode")));
            d.insert(Name::new("Length"), Object::Integer(data.len() as i64));
            doc.add_object(Object::Stream(StreamObj::new_encoded(d, data.clone())))
        }
        PreparedImage::Raw {
            width,
            height,
            gray,
            data,
        } => {
            d.insert(Name::new("Width"), Object::Integer(i64::from(*width)));
            d.insert(Name::new("Height"), Object::Integer(i64::from(*height)));
            let cs = if *gray { "DeviceGray" } else { "DeviceRGB" };
            d.insert(Name::new("ColorSpace"), Object::Name(Name::new(cs)));
            let compressed = flate::encode(data);
            d.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
            d.insert(
                Name::new("Length"),
                Object::Integer(compressed.len() as i64),
            );
            doc.add_object(Object::Stream(StreamObj::new_encoded(d, compressed)))
        }
    }
}

/// Serializes one page's ops into a content stream (top-left → PDF y-up flip).
fn emit_content(page: &PageOps, fonts: &FontSet, opts: &Options) -> Vec<u8> {
    let ph = opts.page_height;
    let mut out: Vec<u8> = Vec::new();
    for op in &page.ops {
        match op {
            Op::Text {
                face,
                size,
                color,
                x,
                baseline,
                text,
            } => {
                out.extend_from_slice(b"BT\n");
                write_line(&mut out, &format!("/{} {} Tf", face.res_name(), fmt(*size)));
                write_line(&mut out, &fill_color(*color));
                write_line(
                    &mut out,
                    &format!("1 0 0 1 {} {} Tm", fmt(*x), fmt(ph - *baseline)),
                );
                let mut show = Vec::new();
                if face.is_embedded() {
                    let emb = if *face == Face::User {
                        fonts.user.as_ref()
                    } else {
                        fonts.cjk.as_ref()
                    };
                    show.push(b'<');
                    if let Some(f) = emb {
                        for ch in text.chars() {
                            show.extend_from_slice(format!("{:04X}", f.gid(ch)).as_bytes());
                        }
                    }
                    show.push(b'>');
                } else {
                    show.push(b'(');
                    for ch in text.chars() {
                        let b = winansi_byte(ch).unwrap_or(b'?');
                        match b {
                            b'\\' => show.extend_from_slice(b"\\\\"),
                            b'(' => show.extend_from_slice(b"\\("),
                            b')' => show.extend_from_slice(b"\\)"),
                            _ => show.push(b),
                        }
                    }
                    show.push(b')');
                }
                out.extend_from_slice(&show);
                out.extend_from_slice(b" Tj\nET\n");
            }
            Op::FillRect { x, y, w, h, color } => {
                write_line(&mut out, &fill_color(*color));
                write_line(
                    &mut out,
                    &format!(
                        "{} {} {} {} re f",
                        fmt(*x),
                        fmt(ph - *y - *h),
                        fmt(*w),
                        fmt(*h)
                    ),
                );
            }
            Op::StrokeRect {
                x,
                y,
                w,
                h,
                color,
                line_width,
            } => {
                write_line(&mut out, &stroke_color(*color));
                write_line(&mut out, &format!("{} w", fmt(*line_width)));
                write_line(
                    &mut out,
                    &format!(
                        "{} {} {} {} re S",
                        fmt(*x),
                        fmt(ph - *y - *h),
                        fmt(*w),
                        fmt(*h)
                    ),
                );
            }
            Op::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
            } => {
                write_line(&mut out, &stroke_color(*color));
                write_line(&mut out, &format!("{} w", fmt(*width)));
                write_line(
                    &mut out,
                    &format!(
                        "{} {} m {} {} l S",
                        fmt(*x1),
                        fmt(ph - *y1),
                        fmt(*x2),
                        fmt(ph - *y2)
                    ),
                );
            }
            Op::FillCircle { cx, cy, r, color } => {
                emit_circle(&mut out, *cx, ph - *cy, *r, *color);
            }
            Op::Image { id, x, y, w, h } => {
                write_line(&mut out, "q");
                write_line(
                    &mut out,
                    &format!(
                        "{} 0 0 {} {} {} cm",
                        fmt(*w),
                        fmt(*h),
                        fmt(*x),
                        fmt(ph - *y - *h)
                    ),
                );
                write_line(&mut out, &format!("/Im{id} Do"));
                write_line(&mut out, "Q");
            }
        }
    }
    out
}

/// Appends `line` + `\n` to `out` (a tiny helper so call sites stay short).
fn write_line(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
}

/// A filled circle at user-space center `(cx, cy)` as four cubic Béziers.
fn emit_circle(out: &mut Vec<u8>, cx: f64, cy: f64, r: f64, color: Rgb) {
    let o = r * KAPPA;
    write_line(out, &fill_color(color));
    write_line(out, &format!("{} {} m", fmt(cx + r), fmt(cy)));
    write_line(
        out,
        &format!(
            "{} {} {} {} {} {} c",
            fmt(cx + r),
            fmt(cy + o),
            fmt(cx + o),
            fmt(cy + r),
            fmt(cx),
            fmt(cy + r)
        ),
    );
    write_line(
        out,
        &format!(
            "{} {} {} {} {} {} c",
            fmt(cx - o),
            fmt(cy + r),
            fmt(cx - r),
            fmt(cy + o),
            fmt(cx - r),
            fmt(cy)
        ),
    );
    write_line(
        out,
        &format!(
            "{} {} {} {} {} {} c",
            fmt(cx - r),
            fmt(cy - o),
            fmt(cx - o),
            fmt(cy - r),
            fmt(cx),
            fmt(cy - r)
        ),
    );
    write_line(
        out,
        &format!(
            "{} {} {} {} {} {} c",
            fmt(cx + o),
            fmt(cy - r),
            fmt(cx + r),
            fmt(cy - o),
            fmt(cx + r),
            fmt(cy)
        ),
    );
    write_line(out, "f");
}

fn fill_color(c: Rgb) -> String {
    format!("{} {} {} rg", fmt_c(c.0), fmt_c(c.1), fmt_c(c.2))
}

fn stroke_color(c: Rgb) -> String {
    format!("{} {} {} RG", fmt_c(c.0), fmt_c(c.1), fmt_c(c.2))
}

fn fmt_c(v: f64) -> String {
    fmt(v.clamp(0.0, 1.0))
}

/// Formats a scalar for a content operator: integers without a decimal point,
/// otherwise ≤ 4 fractional digits with trailing zeros trimmed (same convention
/// as `pdf-edit`). Non-finite values degrade to `0`.
fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}
