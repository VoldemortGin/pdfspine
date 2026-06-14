//! Text insertion — `insert_text` / `insert_textbox` (PRD §8.8, §7).
//!
//! Two font paths:
//! - **Base-14 standard fonts** (helv/Helvetica, tiro/Times, cour/Courier +
//!   bold/italic, symbol, zapf) need **no embedding**: a `/Type1 /BaseFont …`
//!   resource is registered and glyph advances come from the built-in Core-14
//!   width table (`pdf_fonts::std_widths`). Text bytes are WinAnsi (single-byte
//!   `Tj`).
//! - A **user TTF/OTF** (`fontfile`): the whole font program is embedded as a
//!   `/Type0` Identity-H font (see [`crate::fontfile`]); text is emitted as
//!   2-byte glyph-ID codes via a hex string, with a `/ToUnicode` map so the
//!   inserted text stays extractable / searchable.
//!
//! Coordinates: PyMuPDF passes a **top-left** page point as the text *baseline
//! origin*; [`PageContent::to_user_space`] converts it to PDF user space
//! (y-up). Multi-line text splits on `\n` with a leading of `fontsize * 1.2`.

use std::collections::BTreeMap;

use pdf_core::error::Result;
use pdf_core::geom::{Point, Rect};
use pdf_core::object::{Dict, Name, Object};
use pdf_core::DocumentStore;
use pdf_fonts::std_widths;
use pdf_fonts::widths::normalize_standard_font;

use crate::color::Color;
use crate::content::{escape_pdf_literal, fmt_num, PageContent};
use crate::fontfile::EmbeddedFont;

/// Default line-leading factor (line height = `fontsize * LEADING`), matching
/// PyMuPDF's `insert_text` default.
const LEADING: f64 = 1.2;

/// Horizontal alignment for [`insert_textbox`] (PyMuPDF `TEXT_ALIGN_*`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Align {
    /// Left-aligned (default).
    #[default]
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
    /// Justified — last paragraph line stays left-aligned (treated as Left here
    /// for the simple, common case).
    Justify,
}

/// Options for [`insert_text`] / [`insert_textbox`].
pub struct TextOptions<'a> {
    /// Font selector: a Base-14 alias (`helv`, `tiro`, `cour`, …) when
    /// `fontfile` is `None`.
    pub fontname: &'a str,
    /// Font size in points.
    pub fontsize: f64,
    /// Fill color.
    pub color: Color,
    /// Optional user font program (TTF/OTF) to embed. When set, the Base-14
    /// path is bypassed.
    pub fontfile: Option<&'a [u8]>,
    /// Box alignment (textbox only).
    pub align: Align,
}

impl<'a> Default for TextOptions<'a> {
    fn default() -> Self {
        TextOptions {
            fontname: "helv",
            fontsize: 11.0,
            color: Color::BLACK,
            fontfile: None,
            align: Align::Left,
        }
    }
}

/// Resolves the friendly `fontname` alias to a canonical Base-14 key. Accepts
/// PyMuPDF's short aliases (`helv`, `tiro`, `cour`, `symb`, `zadb`, …) and any
/// `/BaseFont`-style name.
pub fn resolve_base14(fontname: &str) -> &'static str {
    let lower = fontname.to_ascii_lowercase();
    let alias = match lower.as_str() {
        "helv" => "Helvetica",
        "hebo" => "Helvetica-Bold",
        "heit" => "Helvetica-Oblique",
        "hebi" => "Helvetica-BoldOblique",
        "tiro" | "times" => "Times-Roman",
        "tibo" => "Times-Bold",
        "tiit" => "Times-Italic",
        "tibi" => "Times-BoldItalic",
        "cour" => "Courier",
        "cobo" => "Courier-Bold",
        "coit" => "Courier-Oblique",
        "cobi" => "Courier-BoldOblique",
        "symb" | "symbol" => "Symbol",
        "zadb" | "zapf" | "zapfdingbats" => "ZapfDingbats",
        _ => "",
    };
    if !alias.is_empty() {
        return alias;
    }
    normalize_standard_font(fontname).unwrap_or("Helvetica")
}

/// A Base-14 `/Type1` font resource object for the canonical standard font
/// `std_name` (no embedding — ISO 32000-1 §9.6.2.2).
fn base14_font_object(std_name: &str) -> Object {
    let mut d = Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Type1")));
    d.insert(Name::new("BaseFont"), Object::Name(Name::new(std_name)));
    // WinAnsi so single-byte 0x20..0xFF map predictably (Symbol / ZapfDingbats
    // carry a built-in encoding and take no /Encoding).
    if std_name != "Symbol" && std_name != "ZapfDingbats" {
        d.insert(
            Name::new("Encoding"),
            Object::Name(Name::new("WinAnsiEncoding")),
        );
    }
    Object::Dictionary(d)
}

/// Encodes `text` to WinAnsi bytes for a Base-14 `Tj` operand. ASCII
/// (0x20..0x7E) and the Latin-1 supplement (0xA0..0xFF) map directly; other
/// chars degrade to `?` so the operand stays valid (never panics).
fn winansi_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|ch| {
            let cp = ch as u32;
            if (0x20..=0x7e).contains(&cp) || (0xa0..=0xff).contains(&cp) {
                cp as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Inserts `text` at `point` (PyMuPDF top-left baseline origin) on the page at
/// `page_index`, returning the number of lines written (PyMuPDF returns this).
///
/// Multi-line text splits on `\n`, each successive line dropping by
/// `fontsize * 1.2`. The font is registered (Base-14) or embedded (TTF) and a
/// new content chunk is appended after the existing content (wrapped in
/// `q … Q`), so existing content stays intact (PRD §8.8).
///
/// # Errors
///
/// Propagates resolve / ChangeSet errors; an unparseable `fontfile` yields a
/// typed error (never panics).
pub fn insert_text(
    doc: &DocumentStore,
    page_index: usize,
    point: Point,
    text: &str,
    opts: &TextOptions,
) -> Result<usize> {
    let pc = PageContent::new(doc, page_index)?;
    let origin = pc.to_user_space(point);
    let lines: Vec<&str> = text.split('\n').collect();
    let leading = opts.fontsize * LEADING;

    if let Some(program) = opts.fontfile {
        let font = EmbeddedFont::parse(program)?;
        let mut used = BTreeMap::new();
        let mut shows = Vec::with_capacity(lines.len());
        for line in &lines {
            let mut hex = String::from("<");
            for ch in line.chars() {
                let gid = font.glyph_id(ch);
                used.insert(gid, ch);
                hex.push_str(&format!("{gid:04X}"));
            }
            hex.push('>');
            shows.push(hex.into_bytes());
        }
        let font_ref = font.write_type0(doc, &used)?;
        let name = pc.add_resource("Font", "F", Object::Reference(font_ref))?;
        let chunk = build_text_chunk(&name, opts, leading, origin, &shows);
        pc.append_content(&chunk)?;
    } else {
        let std_name = resolve_base14(opts.fontname);
        let shows: Vec<Vec<u8>> = lines
            .iter()
            .map(|line| {
                let esc = escape_pdf_literal(&winansi_bytes(line));
                let mut s = vec![b'('];
                s.extend_from_slice(&esc);
                s.push(b')');
                s
            })
            .collect();
        let name = pc.add_resource("Font", "F", base14_font_object(std_name))?;
        let chunk = build_text_chunk(&name, opts, leading, origin, &shows);
        pc.append_content(&chunk)?;
    }
    Ok(lines.len())
}

/// Builds a complete `q BT … ET Q` content chunk: select the font (`Tf`), set
/// the fill color and leading, position at `origin` (`Tm`), then show each
/// pre-rendered line operand (`Tj`) with `T*` line advances. `show` operands are
/// already `( … )` or `< … >` strings.
fn build_text_chunk(
    font_name: &str,
    opts: &TextOptions,
    leading: f64,
    origin: Point,
    shows: &[Vec<u8>],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"q\nBT\n");
    out.extend_from_slice(format!("/{} {} Tf\n", font_name, fmt_num(opts.fontsize)).as_bytes());
    out.extend_from_slice(format!("{}\n", opts.color.fill_op()).as_bytes());
    out.extend_from_slice(format!("{} TL\n", fmt_num(leading)).as_bytes());
    out.extend_from_slice(
        format!("1 0 0 1 {} {} Tm\n", fmt_num(origin.x), fmt_num(origin.y)).as_bytes(),
    );
    for (i, operand) in shows.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b"T*\n");
        }
        out.extend_from_slice(operand);
        out.extend_from_slice(b" Tj\n");
    }
    out.extend_from_slice(b"ET\nQ\n");
    out
}

/// Inserts wrapped, aligned text into `rect` (PyMuPDF `insert_textbox`),
/// returning the **unused height** (>0) or a **negative overflow** when the text
/// does not fit (PyMuPDF convention). Base-14 path (the common case).
///
/// # Errors
///
/// Propagates resolve / ChangeSet errors.
pub fn insert_textbox(
    doc: &DocumentStore,
    page_index: usize,
    rect: Rect,
    text: &str,
    opts: &TextOptions,
) -> Result<f64> {
    let pc = PageContent::new(doc, page_index)?;
    let user_rect = pc.rect_to_user_space(rect);
    let std_name = resolve_base14(opts.fontname);

    let max_width = user_rect.width();
    let space_w = std_widths::string_advance(std_name, " ", opts.fontsize);
    let mut wrapped: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let words: Vec<&str> = para.split_whitespace().collect();
        if words.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_w = 0.0;
        for w in words {
            let w_width = std_widths::string_advance(std_name, w, opts.fontsize);
            let added = if line.is_empty() {
                w_width
            } else {
                space_w + w_width
            };
            if !line.is_empty() && line_w + added > max_width {
                wrapped.push(std::mem::take(&mut line));
                line.push_str(w);
                line_w = w_width;
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(w);
                line_w += added;
            }
        }
        wrapped.push(line);
    }

    let leading = opts.fontsize * LEADING;
    let total_height = leading * wrapped.len() as f64;

    let name = pc.add_resource("Font", "F", base14_font_object(std_name))?;
    let mut inner = Vec::new();
    inner.extend_from_slice(format!("/{} {} Tf\n", name, fmt_num(opts.fontsize)).as_bytes());
    inner.extend_from_slice(format!("{}\n", opts.color.fill_op()).as_bytes());

    let top_baseline = user_rect.y1 - opts.fontsize;
    for (i, line) in wrapped.iter().enumerate() {
        let y = top_baseline - leading * i as f64;
        if y < user_rect.y0 - opts.fontsize {
            break; // past the bottom edge — surplus reported via the return value
        }
        let line_w = std_widths::string_advance(std_name, line, opts.fontsize);
        let x = match opts.align {
            Align::Left | Align::Justify => user_rect.x0,
            Align::Center => user_rect.x0 + (max_width - line_w) / 2.0,
            Align::Right => user_rect.x1 - line_w,
        };
        let esc = escape_pdf_literal(&winansi_bytes(line));
        inner.extend_from_slice(format!("1 0 0 1 {} {} Tm\n", fmt_num(x), fmt_num(y)).as_bytes());
        inner.push(b'(');
        inner.extend_from_slice(&esc);
        inner.extend_from_slice(b") Tj\n");
    }

    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"q\nBT\n");
    chunk.extend_from_slice(&inner);
    chunk.extend_from_slice(b"ET\nQ\n");
    pc.append_content(&chunk)?;

    // Unused height (positive) when it fits; negative overflow otherwise.
    Ok(user_rect.height() - total_height)
}
