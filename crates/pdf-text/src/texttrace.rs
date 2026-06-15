//! Low-level per-glyph text trace + bbox log (PyMuPDF `Page.get_texttrace` /
//! `Page.get_bboxlog`), derived from the structured [`TextPage`] (PRD §8.6).
//!
//! `get_texttrace` returns one entry per text **span**, each carrying its style
//! (font, size, color, writing direction, ascender/descender) and a per-glyph
//! `(unicode, glyph_id, origin, bbox)` list — the device-space glyph geometry
//! MuPDF emits from its `pdf_run_*` device hooks. We reconstruct it from the
//! already-grouped spans (same device space as the `dict`/`rawdict` output),
//! which is the stable, parse-once source of glyph positions.
//!
//! `get_bboxlog` returns the page's paint log as `(op, bbox)` pairs in reading
//! order: `"fill-text"` for each text line, `"fill-image"` for each image block.
//! (Vector path entries are not reconstructed from the structured page, which
//! drops un-texted vector geometry; this matches the text-centric trace.)

use crate::model::{BlockKind, Span, TextPage};

/// One glyph in a [`TraceSpan`]: `(unicode_codepoint, glyph_id, origin, bbox)`
/// mirroring PyMuPDF's texttrace `chars` tuple. `glyph_id` is the font glyph
/// index; for Core-14 / simple fonts we report the Unicode codepoint as a stable
/// stand-in (MuPDF's glyph ids are font-internal and not reproducible here).
#[derive(Clone, Debug, PartialEq)]
pub struct TraceChar {
    /// The Unicode codepoint of the glyph.
    pub ucs: u32,
    /// The glyph id (font glyph index); the codepoint stands in when unknown.
    pub gid: u32,
    /// The glyph origin `(x, y)` (baseline left), device space.
    pub origin: (f64, f64),
    /// The glyph bounding box `(x0, y0, x1, y1)`, device space.
    pub bbox: (f64, f64, f64, f64),
}

/// One span of the texttrace (PyMuPDF `get_texttrace` element).
#[derive(Clone, Debug, PartialEq)]
pub struct TraceSpan {
    /// The writing-direction unit vector `(cos, sin)`.
    pub dir: (f64, f64),
    /// The font name.
    pub font: String,
    /// The writing mode: 0 horizontal, 1 vertical.
    pub wmode: i32,
    /// The PyMuPDF span flag bitfield.
    pub flags: u32,
    /// The number of colorspace components (1 gray, 3 RGB, 4 CMYK; 3 here).
    pub colorspace: i32,
    /// The fill color as an `(r, g, b)` float triple in 0..1.
    pub color: (f64, f64, f64),
    /// The font size `Tfs`.
    pub size: f64,
    /// The fill opacity (always 1.0; alpha tracking is deferred).
    pub opacity: f64,
    /// The ascender, unit font size.
    pub ascender: f64,
    /// The descender, unit font size.
    pub descender: f64,
    /// The span bounding box `(x0, y0, x1, y1)`, device space.
    pub bbox: (f64, f64, f64, f64),
    /// The text render type (0 = fill — the common case).
    pub r#type: i32,
    /// The per-glyph trace.
    pub chars: Vec<TraceChar>,
    /// The paint sequence number (span index in reading order).
    pub seqno: usize,
}

/// One entry of the bbox log: `(operation, bbox)`.
#[derive(Clone, Debug, PartialEq)]
pub struct BBoxLogEntry {
    /// The paint operation, e.g. `"fill-text"`, `"fill-image"`.
    pub op: String,
    /// The bounding box `(x0, y0, x1, y1)`, device space.
    pub bbox: (f64, f64, f64, f64),
}

/// Unpacks a packed `0x00RRGGBB` sRGB color into an `(r, g, b)` float triple.
fn unpack_color(rgb: u32) -> (f64, f64, f64) {
    let r = ((rgb >> 16) & 0xFF) as f64 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f64 / 255.0;
    let b = (rgb & 0xFF) as f64 / 255.0;
    (r, g, b)
}

/// Builds one [`TraceSpan`] from a structured [`Span`] (+ its line's direction /
/// writing mode), assigning `seqno`.
fn trace_span(span: &Span, dir: (f64, f64), wmode: u8, seqno: usize) -> TraceSpan {
    let chars = span
        .chars
        .iter()
        .map(|c| {
            let ucs = c.c as u32;
            TraceChar {
                ucs,
                gid: ucs,
                origin: (c.origin.x, c.origin.y),
                bbox: (c.bbox.x0, c.bbox.y0, c.bbox.x1, c.bbox.y1),
            }
        })
        .collect();
    TraceSpan {
        dir,
        font: span.font.to_string(),
        wmode: i32::from(wmode),
        flags: span.flags,
        colorspace: 3,
        color: unpack_color(span.color),
        size: span.size,
        opacity: 1.0,
        ascender: span.ascender,
        descender: span.descender,
        bbox: (span.bbox.x0, span.bbox.y0, span.bbox.x1, span.bbox.y1),
        r#type: 0,
        chars,
        seqno,
    }
}

/// The page's text trace: one [`TraceSpan`] per text span in reading order
/// (PyMuPDF `Page.get_texttrace`).
#[must_use]
pub fn get_texttrace(tp: &TextPage) -> Vec<TraceSpan> {
    let mut out = Vec::new();
    let mut seqno = 0usize;
    for block in &tp.blocks {
        if block.kind != BlockKind::Text {
            continue;
        }
        for line in &block.lines {
            let dir = line.dir;
            for span in &line.spans {
                out.push(trace_span(span, dir, line.wmode, seqno));
                seqno += 1;
            }
        }
    }
    out
}

/// The page's bbox paint log (PyMuPDF `Page.get_bboxlog`): `("fill-text", bbox)`
/// per text line and `("fill-image", bbox)` per image block, in reading order.
#[must_use]
pub fn get_bboxlog(tp: &TextPage) -> Vec<BBoxLogEntry> {
    let mut out = Vec::new();
    for block in &tp.blocks {
        match block.kind {
            BlockKind::Text => {
                for line in &block.lines {
                    out.push(BBoxLogEntry {
                        op: "fill-text".to_string(),
                        bbox: (line.bbox.x0, line.bbox.y0, line.bbox.x1, line.bbox.y1),
                    });
                }
            }
            BlockKind::Image => {
                out.push(BBoxLogEntry {
                    op: "fill-image".to_string(),
                    bbox: (block.bbox.x0, block.bbox.y0, block.bbox.x1, block.bbox.y1),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, BlockKind, Char, Line, Span, TextPage};
    use pdf_core::geom::{Point, Rect};
    use smol_str::SmolStr;

    fn sample_page() -> TextPage {
        let ch = Char {
            origin: Point::new(72.0, 100.0),
            bbox: Rect::new(72.0, 84.0, 86.0, 104.0),
            c: 'H',
        };
        let span = Span {
            bbox: Rect::new(72.0, 84.0, 90.0, 104.0),
            font: SmolStr::new("Helvetica"),
            size: 20.0,
            flags: 0,
            color: 0x00_00_00_00,
            ascender: 1.075,
            descender: -0.299,
            origin: Point::new(72.0, 100.0),
            chars: vec![ch],
            text: "H".to_string(),
        };
        let line = Line {
            bbox: Rect::new(72.0, 84.0, 90.0, 104.0),
            wmode: 0,
            dir: (1.0, 0.0),
            spans: vec![span],
            seq: 0,
        };
        let block = Block {
            bbox: Rect::new(72.0, 84.0, 90.0, 104.0),
            kind: BlockKind::Text,
            lines: vec![line],
            image: None,
            number: 0,
            seq: 0,
        };
        TextPage {
            width: 200.0,
            height: 200.0,
            blocks: vec![block],
        }
    }

    #[test]
    fn texttrace_shape() {
        let tp = sample_page();
        let tt = get_texttrace(&tp);
        assert_eq!(tt.len(), 1);
        let s = &tt[0];
        assert_eq!(s.font, "Helvetica");
        assert_eq!(s.size, 20.0);
        assert_eq!(s.dir, (1.0, 0.0));
        assert_eq!(s.wmode, 0);
        assert_eq!(s.colorspace, 3);
        assert_eq!(s.color, (0.0, 0.0, 0.0));
        assert_eq!(s.chars.len(), 1);
        assert_eq!(s.chars[0].ucs, 'H' as u32);
        assert_eq!(s.chars[0].origin, (72.0, 100.0));
        assert_eq!(s.seqno, 0);
    }

    #[test]
    fn bboxlog_shape() {
        let tp = sample_page();
        let bl = get_bboxlog(&tp);
        assert_eq!(bl.len(), 1);
        assert_eq!(bl[0].op, "fill-text");
        assert_eq!(bl[0].bbox, (72.0, 84.0, 90.0, 104.0));
    }
}
