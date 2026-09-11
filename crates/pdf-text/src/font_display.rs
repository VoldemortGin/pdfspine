//! Dynamic font-name presentation without changing canonical text layout.

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};

use pdf_core::geom::{Matrix, Point, Quad, Rect};

use crate::model::{Char, Span};

static SUBSET_FONTNAMES: AtomicBool = AtomicBool::new(false);

/// Queries or changes whether structured output retains subset font prefixes.
pub fn set_subset_fontnames(value: Option<bool>) -> bool {
    if let Some(value) = value {
        SUBSET_FONTNAMES.store(value, Ordering::Relaxed);
        value
    } else {
        SUBSET_FONTNAMES.load(Ordering::Relaxed)
    }
}

/// Borrowed presentation of one existing span, or a consecutive raw-font run.
/// Unchanged fields are borrowed from the canonical span via `Deref`.
pub struct SpanView<'a> {
    source: &'a Span,
    /// Characters in this run; no glyph data is copied.
    pub chars: &'a [Char],
    /// Display font name for this run.
    pub font: &'a str,
    /// Text is allocated only when splitting an existing span.
    pub text: Cow<'a, str>,
    /// Union of this run's character boxes.
    pub bbox: Rect,
    /// Directional envelope of this run's character quads.
    pub quad: Quad,
    /// First character's actual device origin.
    pub origin: Point,
    /// First character's actual device matrix.
    pub matrix: Matrix,
    /// First character's rendered size.
    pub rendered_size: f64,
    /// Minimum painting index in this run.
    pub seq: usize,
    /// Source Tm, unavailable for a later newly split run.
    pub text_matrix: Option<Matrix>,
    /// Source CTM, unavailable for a later newly split run.
    pub ctm: Option<Matrix>,
}

impl Deref for SpanView<'_> {
    type Target = Span;
    fn deref(&self) -> &Span {
        self.source
    }
}

fn display_name<'a>(span: &'a Span, ch: &'a Char) -> &'a str {
    ch.raw_font_name
        .as_deref()
        .map_or(span.font.as_str(), |name| name.as_str())
}

/// Iterates display runs under one caller-supplied policy snapshot.
/// False preserves every canonical value exactly and performs no allocation.
pub fn span_views(span: &Span, subset: bool) -> impl Iterator<Item = SpanView<'_>> {
    let mut start = 0;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let font = if subset && !span.chars.is_empty() {
            display_name(span, &span.chars[start])
        } else {
            span.font.as_str()
        };
        let end = if subset {
            span.chars[start..]
                .iter()
                .position(|ch| display_name(span, ch) != font)
                .map_or(span.chars.len(), |offset| start + offset)
        } else {
            span.chars.len()
        };
        let whole = start == 0 && end == span.chars.len();
        let chars = &span.chars[start..end];
        let mut view = SpanView {
            source: span,
            chars,
            font,
            text: Cow::Borrowed(&span.text),
            bbox: span.bbox,
            quad: span.quad,
            origin: span.origin,
            matrix: span.matrix,
            rendered_size: span.rendered_size,
            seq: span.seq,
            text_matrix: (start == 0).then_some(span.text_matrix),
            ctm: (start == 0).then_some(span.ctm),
        };
        if !whole {
            let first = &chars[0];
            view.text = Cow::Owned(chars.iter().map(|ch| ch.c).collect());
            view.bbox = chars
                .iter()
                .skip(1)
                .fold(first.bbox, |bbox, ch| bbox.union(&ch.bbox));
            let mut envelope = crate::layout::DirEnvelope::new(span.dir);
            for ch in chars {
                envelope.add(&ch.quad);
            }
            view.quad = envelope.collapse();
            view.origin = first.origin;
            view.matrix = first.matrix;
            view.rendered_size = first.rendered_size;
            view.seq = chars.iter().map(|ch| ch.seq).min().unwrap_or(span.seq);
        }
        start = end;
        done = end == span.chars.len();
        Some(view)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use smol_str::SmolStr;
    use std::sync::Arc;

    fn sample() -> Span {
        let chars: Vec<_> = [('A', "ABCDEF+Helvetica"), ('B', "UVWXYZ+Helvetica")]
            .into_iter()
            .enumerate()
            .map(|(i, (c, name))| {
                let origin = Point::new(10.0 * i as f64, 20.0);
                let bbox = Rect::new(origin.x, 10.0, origin.x + 10.0, 22.0);
                Char {
                    raw_font_name: Some(Arc::new(SmolStr::new(name))),
                    origin,
                    bbox,
                    c,
                    matrix: Matrix::new(10.0, 0.0, 0.0, -10.0, origin.x, origin.y),
                    quad: Quad::from_rect(&bbox),
                    rendered_size: 10.0,
                    seq: i + 4,
                    synthetic: false,
                }
            })
            .collect();
        let first = &chars[0];
        Span {
            bbox: Rect::new(-1.0, 9.0, 21.0, 23.0),
            font: SmolStr::new("Helvetica"),
            size: 10.0,
            flags: 4,
            color: 123,
            ascender: 1.0,
            descender: -0.2,
            origin: first.origin,
            text: "AB".into(),
            rendered_size: first.rendered_size,
            matrix: first.matrix,
            text_matrix: Matrix::translate(0.0, 20.0),
            ctm: Matrix::scale(2.0, 2.0),
            dir: (1.0, 0.0),
            quad: Quad::from_rect(&Rect::new(-1.0, 9.0, 21.0, 23.0)),
            seq: 4,
            chars,
        }
    }

    #[test]
    fn disabled_borrows_and_preserves_even_nonchar_envelope() {
        let span = sample();
        let views: Vec<_> = span_views(&span, false).collect();
        assert_eq!(views.len(), 1);
        assert!(matches!(views[0].text, Cow::Borrowed("AB")));
        assert!(std::ptr::eq(views[0].chars, span.chars.as_slice()));
        assert_eq!(views[0].bbox, span.bbox);
        assert_eq!(views[0].quad, span.quad);
        assert_eq!(views[0].text_matrix, Some(span.text_matrix));
    }

    #[test]
    fn split_borrows_chars_and_reports_only_known_source_matrices() {
        let span = sample();
        let views: Vec<_> = span_views(&span, true).collect();
        assert_eq!(views.len(), 2);
        for (view, ch) in views.iter().zip(&span.chars) {
            assert_eq!(view.bbox, ch.bbox);
            assert_eq!(view.matrix, ch.matrix);
            assert_eq!(view.origin, ch.origin);
            assert_eq!(view.seq, ch.seq);
            assert_eq!(view.flags, span.flags);
        }
        assert_eq!(views[0].text_matrix, Some(span.text_matrix));
        assert_eq!(views[1].text_matrix, None);
        assert_eq!(views[1].ctm, None);
        assert_eq!(views[1].font, "UVWXYZ+Helvetica");
    }

    #[test]
    fn equal_values_and_empty_spans_do_not_spuriously_split() {
        let mut span = sample();
        span.chars[1].raw_font_name = Some(Arc::new(SmolStr::new("ABCDEF+Helvetica")));
        assert_eq!(span_views(&span, true).count(), 1);
        span.chars.clear();
        span.text.clear();
        let views: Vec<_> = span_views(&span, true).collect();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].font, "Helvetica");
        assert_eq!(views[0].text_matrix, Some(span.text_matrix));
    }
}
