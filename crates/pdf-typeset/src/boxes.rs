//! Text-box layout (TS-5, PRD §10): a fixed rect that does its own vertical
//! anchoring, `normAutofit` font scaling, rotation and clipping — deliberately
//! **not** `insert_textbox`, which silently drops overflow (PRD §10 TRAP).
//!
//! Two-pass layout: the blocks are laid out at the box width through the
//! shared flow core ([`layout_box_content`]), the measured content height
//! drives the [`VAnchor`] offset, then the positioned ops are translated into
//! page coordinates. With `font_scale: Some(_)` (autofit on), overflowing
//! content binary-searches the largest scale that fits over the pure measure
//! path — deterministic (fixed iteration count). Rotation wraps the ops in an
//! [`Op::Group`] transform about the box center; `clip` adds the box rect as
//! the group's `W n` clip path and reports [`ExportWarning::BoxOverflowClipped`]
//! when content is actually lost.

use crate::flow::{layout_box_content, EPS};
use crate::model::{Block, TextBoxSpec, VAnchor};
use crate::ops::{translate_ops, Op, PathSeg};
use crate::warn::ExportWarning;
use crate::{Matrix, Typesetter};

/// Autofit binary-search floor (content that cannot fit even at 5% keeps 5%).
const MIN_AUTOFIT_SCALE: f64 = 0.05;
/// Autofit binary-search iterations (fixed count ⇒ deterministic output).
const AUTOFIT_ITERS: u32 = 16;

/// Lays out one text box and returns its positioned page-coordinate ops
/// (a single [`Op::Group`] when rotated and/or clipped).
pub(crate) fn layout_text_box(ts: &mut Typesetter, spec: &TextBoxSpec) -> Vec<Op> {
    let bx = spec.rect.x0.min(spec.rect.x1);
    let by = spec.rect.y0.min(spec.rect.y1);
    let bw = (spec.rect.x1 - spec.rect.x0).abs().max(1.0);
    let bh = (spec.rect.y1 - spec.rect.y0).abs().max(1.0);

    let mut scale = spec.font_scale.unwrap_or(1.0);
    if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
        scale = 1.0; // normAutofit fontScale is in (0, 1]
    }

    let (mut ops, mut content_h, mut content_max_x) = lay(ts, spec, scale, bw);
    if spec.font_scale.is_some() && content_h > bh + EPS {
        // Autofit: largest scale in [MIN_AUTOFIT_SCALE, scale] whose content
        // fits the box height (`lo` tracks the best known fit).
        let mut lo = MIN_AUTOFIT_SCALE.min(scale);
        let mut hi = scale;
        for _ in 0..AUTOFIT_ITERS {
            let mid = 0.5 * (lo + hi);
            let (_, h, _) = lay(ts, spec, mid, bw);
            if h <= bh + EPS {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (o, h, mx) = lay(ts, spec, lo, bw);
        ops = o;
        content_h = h;
        content_max_x = mx;
    }

    let dy = match spec.v_anchor {
        VAnchor::Top => 0.0,
        VAnchor::Middle => (bh - content_h) / 2.0,
        VAnchor::Bottom => bh - content_h,
    };
    translate_ops(&mut ops, bx, by + dy);

    let rotated = spec.rotation_deg.is_finite() && spec.rotation_deg.rem_euclid(360.0) != 0.0;
    if !(rotated || spec.clip) {
        return ops;
    }

    let transform = rotated.then(|| {
        // Visual counter-clockwise rotation about the box center in top-left
        // (y-down) coordinates = mathematical rotation by −deg, conjugated
        // with translations to the center.
        let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
        let to_origin = Matrix::translate(-cx, -cy);
        let rot = Matrix::rotate(-spec.rotation_deg);
        let back = Matrix::translate(cx, cy);
        Matrix::concat(&Matrix::concat(&to_origin, &rot), &back)
    });
    let clip = spec.clip.then(|| {
        vec![
            PathSeg::MoveTo { x: bx, y: by },
            PathSeg::LineTo { x: bx + bw, y: by },
            PathSeg::LineTo {
                x: bx + bw,
                y: by + bh,
            },
            PathSeg::LineTo { x: bx, y: by + bh },
            PathSeg::Close,
        ]
    });
    if spec.clip {
        let overflow = (content_h - bh).max(content_max_x - bw).max(0.0);
        if overflow > EPS {
            ts.warn(ExportWarning::BoxOverflowClipped {
                overflow_pt: overflow,
            });
        }
    }
    vec![Op::Group {
        transform,
        clip,
        ops,
    }]
}

/// One layout pass at `scale` (run sizes multiplied before layout — the
/// `normAutofit` semantics; geometry is untouched).
fn lay(ts: &mut Typesetter, spec: &TextBoxSpec, scale: f64, width: f64) -> (Vec<Op>, f64, f64) {
    if (scale - 1.0).abs() < EPS {
        return layout_box_content(ts, &spec.blocks, width, spec.wrap);
    }
    let scaled = scale_blocks(&spec.blocks, scale);
    layout_box_content(ts, &scaled, width, spec.wrap)
}

/// Deep-copies blocks with every run size multiplied by `s` (tables included;
/// image display sizes and paragraph geometry stay untouched).
fn scale_blocks(blocks: &[Block], s: f64) -> Vec<Block> {
    blocks.iter().map(|b| scale_block(b, s)).collect()
}

fn scale_block(block: &Block, s: f64) -> Block {
    match block {
        Block::Paragraph(props, runs) => Block::Paragraph(
            props.clone(),
            runs.iter()
                .map(|r| {
                    let mut r = r.clone();
                    r.style.size *= s;
                    r
                })
                .collect(),
        ),
        Block::Table(spec) => {
            let mut spec = spec.clone();
            for row in &mut spec.rows {
                for cell in &mut row.cells {
                    cell.blocks = scale_blocks(&cell.blocks, s);
                }
            }
            Block::Table(spec)
        }
        Block::Image(spec) => Block::Image(spec.clone()),
        Block::PageBreak => Block::PageBreak,
    }
}
