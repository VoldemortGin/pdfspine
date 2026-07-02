//! Document assembly (TS-4, PRD §10 scope (f)): positioned draw ops → content
//! streams → PDF bytes, copy-adapted from the proven `pdf-markdown` assembly
//! and generalized to N registered faces plus the shape / alpha / clip /
//! transform op vocabulary.
//!
//! - Every used face embeds **once per document** via
//!   [`pdf_edit::EmbeddedFont::write_type0`] (Type0 / Identity-H, usage-based
//!   glyph subset, always-written ToUnicode — the read-back gate), with
//!   whole-document glyph usage accumulated in pass 1.
//! - Constant-alpha fills/strokes share deduplicated `/ExtGState` objects
//!   (`ca`/`CA`), one per distinct alpha pair per document.
//! - [`Op::Group`] becomes `q [cm] [clip W n] … Q`; the group transform is
//!   authored in top-left coordinates and conjugated with the page flip at
//!   emission.
//!
//! Output is deterministic: object allocation follows a fixed order, all maps
//! are ordered, and [`pdf_core::SaveOptions::default`] is the deterministic
//! table-xref baseline (no timestamps, content-hash `/ID`). Content streams
//! are written uncompressed so operator-level assertions can grep the bytes.

use std::collections::{BTreeMap, BTreeSet};

use pdf_core::error::{Error, Result};
use pdf_core::filters::flate;
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::{DocumentStore, Limits, SaveOptions, XrefStyle};
use pdf_image::imagedoc::{image_profile, open_image_document, ImageFormat};
use pdf_image::pixmap::Colorspace;

use crate::faces::FaceRegistry;
use crate::ops::{Fill, LineCap, LineJoin, Op, PageOps, PathSeg, Stroke};
use crate::{Matrix, Rgb};

/// The cubic-Bézier circle constant κ = 4/3·(√2 − 1) (same as `pdf-edit`).
const KAPPA: f64 = 0.552_284_749_830_793_4;

// --- prepared images -----------------------------------------------------------

/// A decoded, embed-ready image (the pdf-markdown preparation, generalized):
/// JPEG bytes pass through as `/DCTDecode`; every other supported raster
/// (PNG/BMP/GIF/WEBP/TIFF) decodes to 8-bit Gray/RGB samples (`/FlateDecode`,
/// alpha composited over white).
pub(crate) enum PreparedImage {
    /// Verbatim JPEG bytes.
    Jpeg {
        width: u32,
        height: u32,
        /// 1 = Gray, 3 = RGB/YCbCr, 4 = CMYK (Adobe-inverted, needs `/Decode`).
        components: u8,
        data: Vec<u8>,
    },
    /// Decoded 8-bit samples, interleaved row-major.
    Raw {
        width: u32,
        height: u32,
        /// `true` → 1 byte/pixel `/DeviceGray`, else 3 bytes/pixel `/DeviceRGB`.
        gray: bool,
        data: Vec<u8>,
    },
}

/// Sniffs and decodes image bytes into an embed-ready form.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for unrecognized / undecodable image data
/// (the caller degrades it to an [`crate::ExportWarning::ImageDropped`]).
pub(crate) fn prepare_image(bytes: &[u8]) -> Result<PreparedImage> {
    let format = ImageFormat::sniff(bytes).ok_or(Error::InvalidArgument(
        "pdf-typeset: unrecognized image format",
    ))?;
    if format == ImageFormat::Jpeg {
        let profile = image_profile(bytes).ok_or(Error::InvalidArgument(
            "pdf-typeset: unparseable JPEG image",
        ))?;
        let components = match profile.colorspace {
            1 => 1u8,
            4 => 4u8,
            _ => 3u8,
        };
        return Ok(PreparedImage::Jpeg {
            width: profile.width,
            height: profile.height,
            components,
            data: bytes.to_vec(),
        });
    }
    let doc = open_image_document(bytes, Some(format))
        .map_err(|_| Error::InvalidArgument("pdf-typeset: image decode failed"))?;
    let Some(pix) = doc.pages.first() else {
        return Err(Error::InvalidArgument("pdf-typeset: image has no frames"));
    };
    let gray = pix.colorspace == Colorspace::Gray;
    let comps = usize::from(pix.colorspace.components());
    let stride = comps + usize::from(pix.alpha);
    let pixels = (pix.width as usize) * (pix.height as usize);
    let mut data = Vec::with_capacity(pixels * comps);
    for p in 0..pixels {
        let base = p * stride;
        let a = if pix.alpha {
            f64::from(pix.samples[base + comps]) / 255.0
        } else {
            1.0
        };
        for c in 0..comps {
            let v = f64::from(pix.samples[base + c]);
            // Composite over white so transparency degrades predictably.
            let out_v = (v * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0);
            data.push(out_v as u8);
        }
    }
    Ok(PreparedImage::Raw {
        width: pix.width,
        height: pix.height,
        gray,
        data,
    })
}

// --- pass 1: usage scan ----------------------------------------------------------

/// One document's per-page resource usage plus whole-document glyph usage.
struct Usage {
    /// Per page, per [`crate::ops::FaceId`] index: face shown on this page.
    face_used: Vec<Vec<bool>>,
    /// Per face: every shown `(glyph_id, char)` (drives `/W`, ToUnicode and
    /// the glyph subset).
    glyphs: Vec<BTreeMap<u16, char>>,
    /// Per page: prepared-image ids placed on it.
    images: Vec<BTreeSet<usize>>,
    /// Per page: `(ca_bits, CA_bits)` alpha pairs used on it.
    alphas: Vec<BTreeSet<(u64, u64)>>,
}

/// A clamped alpha as ordered bits (map key material).
fn alpha_bits(a: f64) -> u64 {
    let a = if a.is_finite() {
        a.clamp(0.0, 1.0)
    } else {
        1.0
    };
    a.to_bits()
}

/// The `(ca, CA)` pair of a path op, or `None` when fully opaque.
fn path_alphas(fill: Option<&Fill>, stroke: Option<&Stroke>) -> Option<(u64, u64)> {
    let fa = fill.map_or(1.0, |f| f.alpha);
    let sa = stroke.map_or(1.0, |s| s.alpha);
    let (fb, sb) = (alpha_bits(fa), alpha_bits(sa));
    if fb == alpha_bits(1.0) && sb == alpha_bits(1.0) {
        None
    } else {
        Some((fb, sb))
    }
}

/// Recursively records one op list's usage for page `pi`.
fn scan_ops(ops: &[Op], faces: &FaceRegistry, images_len: usize, pi: usize, usage: &mut Usage) {
    for op in ops {
        match op {
            Op::Text { face, text, .. } => {
                if face.0 < faces.len() {
                    usage.face_used[pi][face.0] = true;
                    for ch in text.chars() {
                        usage.glyphs[face.0].insert(faces.gid(*face, ch), ch);
                    }
                }
            }
            Op::Image { id, .. } => {
                if *id < images_len {
                    usage.images[pi].insert(*id);
                }
            }
            Op::Path { fill, stroke, .. } => {
                if let Some(pair) = path_alphas(fill.as_ref(), stroke.as_ref()) {
                    usage.alphas[pi].insert(pair);
                }
            }
            Op::Group { ops, .. } => scan_ops(ops, faces, images_len, pi, usage),
            _ => {}
        }
    }
}

// --- document assembly ------------------------------------------------------------

/// Assembles the final PDF from laid-out pages (two-pass: whole-document
/// usage accumulation, then deterministic object creation).
pub(crate) fn build_pdf(
    pages: &[PageOps],
    faces: &FaceRegistry,
    images: &[PreparedImage],
) -> Result<Vec<u8>> {
    let nfaces = faces.len();
    let mut usage = Usage {
        face_used: vec![vec![false; nfaces]; pages.len()],
        glyphs: vec![BTreeMap::new(); nfaces],
        images: vec![BTreeSet::new(); pages.len()],
        alphas: vec![BTreeSet::new(); pages.len()],
    };
    for (pi, page) in pages.iter().enumerate() {
        scan_ops(&page.ops, faces, images.len(), pi, &mut usage);
    }

    let doc = DocumentStore::from_bytes(empty_seed_pdf(), Limits::default())?;
    let pages_ref = ObjRef::new(2, 0);

    // Fonts: one write_type0 per used face per document (fixed FaceId order).
    let mut font_refs: Vec<Option<ObjRef>> = vec![None; nfaces];
    for (i, refslot) in font_refs.iter_mut().enumerate() {
        if usage.face_used.iter().any(|u| u[i]) {
            *refslot = Some(
                faces
                    .font(crate::ops::FaceId(i))
                    .write_type0(&doc, &usage.glyphs[i])?,
            );
        }
    }

    // ExtGStates: one object per distinct (ca, CA) pair per document.
    let all_alphas: BTreeSet<(u64, u64)> = usage.alphas.iter().flatten().copied().collect();
    let mut gs_refs: BTreeMap<(u64, u64), (usize, ObjRef)> = BTreeMap::new();
    for (k, pair) in all_alphas.iter().enumerate() {
        let mut d = Dict::new();
        d.insert(Name::new("Type"), Object::Name(Name::new("ExtGState")));
        d.insert(Name::new("ca"), Object::Real(f64::from_bits(pair.0)));
        d.insert(Name::new("CA"), Object::Real(f64::from_bits(pair.1)));
        gs_refs.insert(*pair, (k, doc.add_object(Object::Dictionary(d))?));
    }
    let gs_ids: BTreeMap<(u64, u64), usize> =
        gs_refs.iter().map(|(pair, (k, _))| (*pair, *k)).collect();

    // Images: embedded once per document, ascending id order.
    let used_images: BTreeSet<usize> = usage.images.iter().flatten().copied().collect();
    let mut image_refs: BTreeMap<usize, ObjRef> = BTreeMap::new();
    for &id in &used_images {
        image_refs.insert(id, embed_image(&doc, &images[id])?);
    }

    let mut kids: Vec<Object> = Vec::with_capacity(pages.len());
    for (pi, page) in pages.iter().enumerate() {
        let content = emit_content(page, faces, &gs_ids);
        let content_ref = doc.add_object(Object::Stream(StreamObj::new_encoded(
            Dict::from_iter([(Name::new("Length"), Object::Integer(content.len() as i64))]),
            content,
        )))?;

        let mut resources = Dict::new();
        let mut font_dict = Dict::new();
        for (i, r) in font_refs.iter().enumerate() {
            if usage.face_used[pi][i] {
                if let Some(r) = r {
                    font_dict.insert(Name::new(format!("F{i}")), Object::Reference(*r));
                }
            }
        }
        if !font_dict.is_empty() {
            resources.insert(Name::new("Font"), Object::Dictionary(font_dict));
        }
        if !usage.images[pi].is_empty() {
            let mut xdict = Dict::new();
            for id in &usage.images[pi] {
                if let Some(r) = image_refs.get(id) {
                    xdict.insert(Name::new(format!("Im{id}")), Object::Reference(*r));
                }
            }
            resources.insert(Name::new("XObject"), Object::Dictionary(xdict));
        }
        if !usage.alphas[pi].is_empty() {
            let mut gdict = Dict::new();
            for pair in &usage.alphas[pi] {
                if let Some((k, r)) = gs_refs.get(pair) {
                    gdict.insert(Name::new(format!("GS{k}")), Object::Reference(*r));
                }
            }
            resources.insert(Name::new("ExtGState"), Object::Dictionary(gdict));
        }

        let mut leaf = Dict::new();
        leaf.insert(Name::new("Type"), Object::Name(Name::new("Page")));
        leaf.insert(Name::new("Parent"), Object::Reference(pages_ref));
        leaf.insert(
            Name::new("MediaBox"),
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(sane_dim(page.width)),
                Object::Real(sane_dim(page.height)),
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

/// A finite positive page dimension (degenerate values degrade to A4-ish).
fn sane_dim(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        595.32
    }
}

/// A minimal, openable zero-page seed PDF (catalog + empty page tree; the
/// proven pdf-markdown/pdf-image seed). Object 1 = catalog, 2 = `/Pages`.
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

// --- content emission --------------------------------------------------------------

/// Serializes one page's ops into a content stream (top-left → PDF y-up flip).
fn emit_content(
    page: &PageOps,
    faces: &FaceRegistry,
    gs_ids: &BTreeMap<(u64, u64), usize>,
) -> Vec<u8> {
    let ph = sane_dim(page.height);
    let mut out: Vec<u8> = Vec::new();
    emit_ops(&mut out, &page.ops, ph, faces, gs_ids);
    out
}

/// Emits an op list (recursive over groups). All coordinates flip against the
/// page height `ph`; group transforms conjugate with the same flip, so nested
/// ops emit identically at any depth.
fn emit_ops(
    out: &mut Vec<u8>,
    ops: &[Op],
    ph: f64,
    faces: &FaceRegistry,
    gs_ids: &BTreeMap<(u64, u64), usize>,
) {
    for op in ops {
        match op {
            Op::Text {
                face,
                size,
                color,
                x,
                baseline,
                text,
            } => {
                if face.0 >= faces.len() {
                    continue; // unregistered id: degrade, never panic
                }
                out.extend_from_slice(b"BT\n");
                write_line(out, &format!("/F{} {} Tf", face.0, fmt(*size)));
                write_line(out, &color.fill_op());
                write_line(
                    out,
                    &format!("1 0 0 1 {} {} Tm", fmt(*x), fmt(ph - *baseline)),
                );
                out.push(b'<');
                for ch in text.chars() {
                    out.extend_from_slice(format!("{:04X}", faces.gid(*face, ch)).as_bytes());
                }
                out.extend_from_slice(b"> Tj\nET\n");
            }
            Op::FillRect { x, y, w, h, color } => {
                write_line(out, &color.fill_op());
                write_line(
                    out,
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
                write_line(out, &color.stroke_op());
                write_line(out, &format!("{} w", fmt(*line_width)));
                write_line(
                    out,
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
                write_line(out, &color.stroke_op());
                write_line(out, &format!("{} w", fmt(*width)));
                write_line(
                    out,
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
                emit_circle(out, *cx, ph - *cy, *r, *color);
            }
            Op::Image { id, x, y, w, h } => {
                write_line(out, "q");
                write_line(
                    out,
                    &format!(
                        "{} 0 0 {} {} {} cm",
                        fmt(*w),
                        fmt(*h),
                        fmt(*x),
                        fmt(ph - *y - *h)
                    ),
                );
                write_line(out, &format!("/Im{id} Do"));
                write_line(out, "Q");
            }
            Op::Path { segs, fill, stroke } => {
                if fill.is_none() && stroke.is_none() {
                    continue;
                }
                write_line(out, "q");
                if let Some(pair) = path_alphas(fill.as_ref(), stroke.as_ref()) {
                    if let Some(k) = gs_ids.get(&pair) {
                        write_line(out, &format!("/GS{k} gs"));
                    }
                }
                if let Some(f) = fill {
                    write_line(out, &f.color.fill_op());
                }
                if let Some(s) = stroke {
                    write_line(out, &s.color.stroke_op());
                    write_line(out, &format!("{} w", fmt(s.width)));
                    if s.cap != LineCap::Butt {
                        let j = match s.cap {
                            LineCap::Butt => 0,
                            LineCap::Round => 1,
                            LineCap::Square => 2,
                        };
                        write_line(out, &format!("{j} J"));
                    }
                    if s.join != LineJoin::Miter {
                        let j = match s.join {
                            LineJoin::Miter => 0,
                            LineJoin::Round => 1,
                            LineJoin::Bevel => 2,
                        };
                        write_line(out, &format!("{j} j"));
                    }
                    if !s.dashes.is_empty() {
                        let pattern: Vec<String> = s.dashes.iter().map(|d| fmt(*d)).collect();
                        write_line(out, &format!("[{}] 0 d", pattern.join(" ")));
                    }
                }
                emit_segs(out, segs, ph);
                let paint = match (fill, stroke) {
                    (Some(f), Some(_)) => {
                        if f.even_odd {
                            "B*"
                        } else {
                            "B"
                        }
                    }
                    (Some(f), None) => {
                        if f.even_odd {
                            "f*"
                        } else {
                            "f"
                        }
                    }
                    (None, Some(_)) => "S",
                    (None, None) => unreachable!("filtered above"),
                };
                write_line(out, paint);
                write_line(out, "Q");
            }
            Op::Group {
                transform,
                clip,
                ops,
            } => {
                write_line(out, "q");
                if let Some(m) = transform {
                    let flip = Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, ph);
                    let cm = Matrix::concat(&Matrix::concat(&flip, m), &flip);
                    write_line(
                        out,
                        &format!(
                            "{} {} {} {} {} {} cm",
                            fmt(cm.a),
                            fmt(cm.b),
                            fmt(cm.c),
                            fmt(cm.d),
                            fmt(cm.e),
                            fmt(cm.f)
                        ),
                    );
                }
                if let Some(segs) = clip {
                    emit_segs(out, segs, ph);
                    write_line(out, "W n");
                }
                emit_ops(out, ops, ph, faces, gs_ids);
                write_line(out, "Q");
            }
        }
    }
}

/// Emits path segments with the top-left → PDF y flip.
fn emit_segs(out: &mut Vec<u8>, segs: &[PathSeg], ph: f64) {
    for seg in segs {
        match seg {
            PathSeg::MoveTo { x, y } => {
                write_line(out, &format!("{} {} m", fmt(*x), fmt(ph - *y)));
            }
            PathSeg::LineTo { x, y } => {
                write_line(out, &format!("{} {} l", fmt(*x), fmt(ph - *y)));
            }
            PathSeg::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                write_line(
                    out,
                    &format!(
                        "{} {} {} {} {} {} c",
                        fmt(*x1),
                        fmt(ph - *y1),
                        fmt(*x2),
                        fmt(ph - *y2),
                        fmt(*x),
                        fmt(ph - *y)
                    ),
                );
            }
            PathSeg::Close => write_line(out, "h"),
        }
    }
}

/// Appends `line` + `\n` to `out`.
fn write_line(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
}

/// A filled circle at user-space center `(cx, cy)` as four cubic Béziers.
fn emit_circle(out: &mut Vec<u8>, cx: f64, cy: f64, r: f64, color: Rgb) {
    let o = r * KAPPA;
    write_line(out, &color.fill_op());
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

/// Formats a scalar for a content operator: integers without a decimal point,
/// otherwise ≤ 4 fractional digits with trailing zeros trimmed (the pdf-edit
/// convention). Non-finite values degrade to `0`.
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
