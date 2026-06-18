//! OCR integration: render a page, recognize, and build either an OCR
//! [`TextPage`] (`get_textpage_ocr`) or a searchable "sandwich" PDF
//! (`pdfocr_save` / `pdfocr_tobytes`).
//!
//! # Coordinate mapping
//!
//! A page is rendered at `dpi` via [`pdf_render::render_page`], whose device
//! transform is `page_transform(cropbox, rotate) · scale(s)` with `s = dpi/72`.
//! `page_transform` already lands in PyMuPDF **device space** (origin top-left,
//! y down, `/Rotate` applied), so a pixel `(px, py)` maps back to a page point
//! simply by dividing out the scale: `(px/s, py/s)`. That page-point space is
//! exactly the [`TextPage`] / `insert_text` convention, so no extra y-flip is
//! needed — `pdf_text` and `pdf_edit` perform the final flip into PDF user space
//! themselves.

use std::sync::Arc;

use pdf_core::geom::{Point, Rect};
use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits, SaveOptions};

use pdf_edit::content::PageContent;
use pdf_edit::merge::{insert_pdf, InsertOptions};
use pdf_image::imagedoc;
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_render::{render_page, RenderOptions};
use pdf_text::model::{Block, BlockKind, Char, Line, Span, TextPage};

use crate::engine::{OcrEngine, OcrWord};
use crate::error::{Error, Result};

/// Options shared by the OCR entry points (PyMuPDF defaults).
#[derive(Clone, Debug)]
pub struct OcrOptions {
    /// The engine language code(s), e.g. `"eng"` (Tesseract default).
    pub language: String,
    /// The render DPI fed to the rasterizer + the OCR engine. PyMuPDF defaults
    /// to 72 for `get_textpage_ocr`; the sandwich path benefits from a higher
    /// value (recognition quality), so callers pass it explicitly.
    pub dpi: u32,
    /// Whether to OCR the whole page (`true`) or only existing image regions
    /// (`false`). Only `full == true` is implemented; `false` currently falls
    /// back to full-page OCR (documented deferral).
    pub full: bool,
}

impl Default for OcrOptions {
    fn default() -> Self {
        OcrOptions {
            language: "eng".to_string(),
            dpi: 72,
            full: true,
        }
    }
}

/// Renders `page` at `dpi` to an RGB [`Pixmap`] for OCR input.
fn render_for_ocr(page: &Page, dpi: u32) -> Result<Pixmap> {
    if dpi == 0 {
        return Err(Error::InvalidArgument("dpi must be positive"));
    }
    let doc = page.document();
    let opts = RenderOptions {
        dpi: Some(dpi),
        colorspace: Colorspace::Rgb,
        alpha: false,
        ..RenderOptions::default()
    };
    Ok(render_page(doc, page, &opts)?)
}

/// Builds an OCR [`TextPage`] for `page` (PyMuPDF `Page.get_textpage_ocr`).
///
/// The page is rasterized at `opts.dpi`, recognized by `engine`, and each word's
/// pixel box is mapped to page-point device space (`/s`) to populate one text
/// [`Block`] (one [`Line`] per OCR word, one [`Span`] per line). `get_text` /
/// `search_for` over the returned page then work unchanged.
///
/// # Errors
///
/// Propagates render / engine errors (a missing engine is `kind == "unsupported"`).
pub fn textpage_ocr(page: &Page, engine: &dyn OcrEngine, opts: &OcrOptions) -> Result<TextPage> {
    let pix = render_for_ocr(page, opts.dpi)?;
    let words = engine.recognize(&pix, &opts.language, opts.dpi as f32)?;
    let s = f64::from(opts.dpi) / 72.0;
    let (pw, ph) = pdf_text::page_size(page.cropbox(), page.rotation());
    Ok(build_textpage(&words, s, pw, ph))
}

/// Maps OCR words (pixel space) to a [`TextPage`] in page-point device space.
/// `s = dpi/72`; `(width, height)` are the displayed page size in points.
fn build_textpage(words: &[OcrWord], s: f64, width: f64, height: f64) -> TextPage {
    let mut lines = Vec::with_capacity(words.len());
    let mut page_bbox = Rect::new(width, height, 0.0, 0.0); // grows to a union
    let mut any = false;

    for (i, w) in words.iter().enumerate() {
        let bbox =
            Rect::new(w.bbox.x0 / s, w.bbox.y0 / s, w.bbox.x1 / s, w.bbox.y1 / s).normalize();
        let size = (bbox.height()).max(1.0);
        // The text baseline origin: bottom-left of the cell (PyMuPDF span origin
        // is y-down, baseline ≈ bbox bottom minus the descender; we use the box
        // bottom, which is correct enough for extraction + search).
        let origin = Point::new(bbox.x0, bbox.y1);
        let chars = char_cells(&w.text, &bbox);
        let span = Span {
            bbox,
            font: "GlyphlessFont".into(),
            size,
            flags: 0,
            color: 0,
            ascender: 0.8,
            descender: -0.2,
            origin,
            chars,
            text: w.text.clone(),
        };
        lines.push(Line {
            bbox,
            wmode: 0,
            dir: (1.0, 0.0),
            spans: vec![span],
            seq: i,
        });
        page_bbox = if any { page_bbox | bbox } else { bbox };
        any = true;
    }

    let block_bbox = if any {
        page_bbox
    } else {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    };
    let block = Block {
        bbox: block_bbox,
        kind: BlockKind::Text,
        lines,
        image: None,
        number: 0,
        seq: 0,
    };

    TextPage {
        width,
        height,
        blocks: if any { vec![block] } else { Vec::new() },
    }
}

/// Distributes per-character cells evenly across the word box (OCR gives no
/// per-glyph boxes; even spacing is the fitz-compatible `rawdict` approximation).
fn char_cells(text: &str, bbox: &Rect) -> Vec<Char> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len().max(1) as f64;
    let cell_w = bbox.width() / n;
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let x0 = bbox.x0 + cell_w * i as f64;
            let x1 = x0 + cell_w;
            Char {
                origin: Point::new(x0, bbox.y1),
                bbox: Rect::new(x0, bbox.y0, x1, bbox.y1),
                c,
            }
        })
        .collect()
}

/// Produces a searchable **sandwich PDF**: every page of `doc` is rendered to an
/// image, OCR'd, and rebuilt as a new page that draws the page image with an
/// invisible (render-mode-3) OCR text layer on top. The result is selectable /
/// searchable while looking identical to the rendered original (PyMuPDF
/// `Document.pdfocr_tobytes`).
///
/// # Errors
///
/// Propagates render / engine / save errors. A missing engine is `unsupported`.
pub fn pdfocr_bytes(
    doc: &Arc<DocumentStore>,
    engine: &dyn OcrEngine,
    opts: &OcrOptions,
) -> Result<Vec<u8>> {
    let page_refs = pdf_core::pagetree::page_refs(doc);
    if page_refs.is_empty() {
        return Err(Error::InvalidArgument("document has no pages"));
    }
    // The output document, assembled page by page.
    let out = DocumentStore::from_bytes(empty_pdf(), Limits::default())?;

    for (idx, &leaf) in page_refs.iter().enumerate() {
        let page = Page::new(Arc::clone(doc), idx, leaf);
        let single = sandwich_page(&page, engine, opts)?;
        let src = DocumentStore::from_bytes(single, Limits::default())?;
        insert_pdf(&out, &src, &InsertOptions::default())?;
    }

    out.save_to_vec(&SaveOptions::default().with_garbage(1))
        .map_err(Error::from)
}

/// Builds a one-page sandwich PDF for a single page: image layer + invisible
/// OCR text layer.
fn sandwich_page(page: &Page, engine: &dyn OcrEngine, opts: &OcrOptions) -> Result<Vec<u8>> {
    let pix = render_for_ocr(page, opts.dpi)?;
    let words = engine.recognize(&pix, &opts.language, opts.dpi as f32)?;
    let s = f64::from(opts.dpi) / 72.0;

    // Image layer: the rendered pixmap as a one-page PDF whose MediaBox is
    // `px / dpi * 72` points — i.e. the page size in points (1pt = 1px / s).
    let png = pix.to_png_bytes()?;
    let img_pdf = imagedoc::convert_to_pdf(&png, None)?;
    let single = DocumentStore::from_bytes(img_pdf, Limits::default())?;

    // Collect the full set of Unicode characters across all OCR words and assign
    // each a stable 2-byte code (the invisible text layer is a Type0/Identity-H
    // font whose `/ToUnicode` maps code -> the real character, so `get_text`
    // recovers arbitrary Unicode — incl. CJK — not just WinAnsi).
    let codes = CodeTable::build(&words);

    // Invisible text layer on page 0 of the freshly-built single-page doc.
    let pc = PageContent::new(&single, 0)?;
    let font = unicode_font(&single, &codes)?;
    let font_name = pc.add_resource("Font", "F", font)?;
    let chunk = invisible_text_chunk(&pc, &font_name, &words, s, &codes);
    if !chunk.is_empty() {
        pc.append_content(&chunk)?;
    }

    single
        .save_to_vec(&SaveOptions::default().with_garbage(1))
        .map_err(Error::from)
}

/// Assigns each distinct character across the OCR words a stable 2-byte code for
/// the Identity-H invisible-text font. Code `0` is reserved (`/notdef`), so codes
/// run `1..=n`; the count never exceeds the number of distinct characters on a
/// page, well under the 16-bit limit.
struct CodeTable {
    /// `char` -> assigned 2-byte code.
    map: std::collections::HashMap<char, u16>,
    /// Codes in assignment order, paired with their character (for `/ToUnicode`).
    entries: Vec<(u16, char)>,
}

impl CodeTable {
    fn build(words: &[OcrWord]) -> Self {
        let mut map = std::collections::HashMap::new();
        let mut entries = Vec::new();
        let mut next: u16 = 1;
        for w in words {
            for c in w.text.chars() {
                map.entry(c).or_insert_with(|| {
                    let code = next;
                    next = next.saturating_add(1);
                    entries.push((code, c));
                    code
                });
            }
        }
        CodeTable { map, entries }
    }

    /// The 2-byte code assigned to `c` (every char in a word is registered at
    /// build time, so this is always `Some` for chars drawn from the same words).
    fn code(&self, c: char) -> u16 {
        self.map.get(&c).copied().unwrap_or(0)
    }
}

/// Emits one `BT … ET` chunk drawing every OCR word in **render mode 3**
/// (invisible). Each word is positioned at its page-point top-left and scaled so
/// the (single-line) text roughly fills the word box width — extraction / search
/// only needs the text present and positioned, not pixel-perfect glyph metrics.
/// Text is written as 2-byte Identity-H hex codes so `get_text` recovers the
/// real Unicode via the font's `/ToUnicode` CMap.
fn invisible_text_chunk(
    pc: &PageContent,
    font_name: &str,
    words: &[OcrWord],
    s: f64,
    codes: &CodeTable,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"BT\n");
    out.extend_from_slice(b"3 Tr\n"); // invisible text render mode (Tr 3)
    for w in words {
        if w.text.trim().is_empty() {
            continue;
        }
        let bbox =
            Rect::new(w.bbox.x0 / s, w.bbox.y0 / s, w.bbox.x1 / s, w.bbox.y1 / s).normalize();
        let size = bbox.height().max(1.0);
        // Baseline origin in PDF user space: top-left page point -> bottom-left
        // user-space, then drop to the box bottom for the baseline.
        let origin = pc.to_user_space(Point::new(bbox.x0, bbox.y1));
        // Horizontal scale (Tz, percent) so the rendered glyph run ≈ box width.
        // Approximate the advance as `1.0 em` per char (the descendant `/DW`).
        let n = w.text.chars().count().max(1) as f64;
        let nominal = size * n;
        let tz = if nominal > 0.0 {
            (bbox.width() / nominal * 100.0).clamp(1.0, 1000.0)
        } else {
            100.0
        };
        out.extend_from_slice(format!("/{} {} Tf\n", font_name, fmt_num(size)).as_bytes());
        out.extend_from_slice(format!("{} Tz\n", fmt_num(tz)).as_bytes());
        out.extend_from_slice(
            format!("1 0 0 1 {} {} Tm\n", fmt_num(origin.x), fmt_num(origin.y)).as_bytes(),
        );
        // Identity-H operand: a hex string of 2-byte big-endian codes.
        out.push(b'<');
        for c in w.text.chars() {
            out.extend_from_slice(format!("{:04X}", codes.code(c)).as_bytes());
        }
        out.extend_from_slice(b"> Tj\n");
    }
    out.extend_from_slice(b"ET\n");
    out
}

/// Builds the invisible-text-layer font: a Type0 / Identity-H font over a
/// non-embedded CIDFontType2 descendant, with a `/ToUnicode` CMap mapping each
/// assigned 2-byte code back to its real Unicode character. The glyphs are never
/// painted (render mode 3), so no glyph program is needed — only the code ->
/// Unicode mapping matters, which lets `get_text` recover arbitrary Unicode
/// (incl. CJK), unlike a WinAnsi Base-14 font.
fn unicode_font(doc: &DocumentStore, codes: &CodeTable) -> Result<pdf_core::object::Object> {
    use pdf_core::object::{Dict, Name, Object, StreamObj};

    // FontDescriptor (no FontFile — glyphs are invisible). The flags/bbox are
    // nominal; nothing renders from them.
    let mut desc = Dict::new();
    desc.insert(Name::new("Type"), Object::Name(Name::new("FontDescriptor")));
    desc.insert(Name::new("FontName"), Object::Name(Name::new("OXOCR")));
    desc.insert(Name::new("Flags"), Object::Integer(4)); // Symbolic
    desc.insert(
        Name::new("FontBBox"),
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1000),
            Object::Integer(1000),
        ]),
    );
    desc.insert(Name::new("ItalicAngle"), Object::Integer(0));
    desc.insert(Name::new("Ascent"), Object::Integer(1000));
    desc.insert(Name::new("Descent"), Object::Integer(0));
    desc.insert(Name::new("CapHeight"), Object::Integer(1000));
    desc.insert(Name::new("StemV"), Object::Integer(80));
    let desc_ref = doc.add_object(Object::Dictionary(desc))?;

    // CIDSystemInfo (Adobe Identity 0 for an Identity-H font).
    let mut csi = Dict::new();
    csi.insert(
        Name::new("Registry"),
        Object::String(pdf_core::object::PdfString::literal("Adobe")),
    );
    csi.insert(
        Name::new("Ordering"),
        Object::String(pdf_core::object::PdfString::literal("Identity")),
    );
    csi.insert(Name::new("Supplement"), Object::Integer(0));

    // Descendant CIDFontType2 (Identity CID->GID; uniform `/DW` advance).
    let mut cid = Dict::new();
    cid.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    cid.insert(
        Name::new("Subtype"),
        Object::Name(Name::new("CIDFontType2")),
    );
    cid.insert(Name::new("BaseFont"), Object::Name(Name::new("OXOCR")));
    cid.insert(Name::new("CIDSystemInfo"), Object::Dictionary(csi));
    cid.insert(Name::new("FontDescriptor"), Object::Reference(desc_ref));
    cid.insert(Name::new("DW"), Object::Integer(1000));
    cid.insert(
        Name::new("CIDToGIDMap"),
        Object::Name(Name::new("Identity")),
    );
    let cid_ref = doc.add_object(Object::Dictionary(cid))?;

    // ToUnicode CMap stream: code (2-byte) -> the character (UTF-16BE).
    let cmap = to_unicode_cmap(codes);
    let mut tdict = Dict::new();
    tdict.insert(Name::new("Length"), Object::Integer(cmap.len() as i64));
    let tu_ref = doc.add_object(Object::Stream(StreamObj::new_encoded(tdict, cmap)))?;

    // The composite Type0 font.
    let mut f = Dict::new();
    f.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    f.insert(Name::new("Subtype"), Object::Name(Name::new("Type0")));
    f.insert(Name::new("BaseFont"), Object::Name(Name::new("OXOCR")));
    f.insert(Name::new("Encoding"), Object::Name(Name::new("Identity-H")));
    f.insert(
        Name::new("DescendantFonts"),
        Object::Array(vec![Object::Reference(cid_ref)]),
    );
    f.insert(Name::new("ToUnicode"), Object::Reference(tu_ref));
    Ok(Object::Dictionary(f))
}

/// Builds a `/ToUnicode` CMap mapping each assigned 2-byte code to its character
/// (as a UTF-16BE value), so a PDF reader / `get_text` recovers the real text.
fn to_unicode_cmap(codes: &CodeTable) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(
        "/CIDInit /ProcSet findresource begin\n\
12 dict begin\n\
begincmap\n\
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
/CMapName /Adobe-Identity-UCS def\n\
/CMapType 2 def\n\
1 begincodespacerange\n\
<0000> <FFFF>\n\
endcodespacerange\n",
    );

    // `beginbfchar` blocks are capped at 100 entries each (PDF spec).
    for chunk in codes.entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for &(code, ch) in chunk {
            out.push_str(&format!("<{code:04X}> <"));
            // The destination is the character encoded as UTF-16BE hex.
            let mut buf = [0u16; 2];
            for u in ch.encode_utf16(&mut buf) {
                out.push_str(&format!("{u:04X}"));
            }
            out.push_str(">\n");
        }
        out.push_str("endbfchar\n");
    }

    out.push_str(
        "endcmap\n\
CMapName currentdict /CMap defineresource pop\n\
end\n\
end\n",
    );
    out.into_bytes()
}

/// Formats a coordinate / scalar for a content operator (no trailing zeros).
/// Inlined from `pdf_edit::content` (which keeps it `pub(crate)`).
fn fmt_num(v: f64) -> String {
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

/// A minimal empty single-document PDF used as the sandwich assembly target.
fn empty_pdf() -> Vec<u8> {
    b"%PDF-1.7\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[]/Count 0>>endobj\n\
trailer<</Root 1 0 R/Size 3>>\n\
%%EOF\n"
        .to_vec()
}
