//! Immutable owned callback events. Python controls callback lifecycle outside Rust borrows.
use super::*;
use pdf_api::replay::{PathItem, RenderOp};

type MatrixTuple = (f64, f64, f64, f64, f64, f64);
type RectTuple = (f64, f64, f64, f64);
fn mt(m: Matrix) -> MatrixTuple {
    (m.a, m.b, m.c, m.d, m.e, m.f)
}
fn rt(r: Rect) -> RectTuple {
    (r.x0, r.y0, r.x1, r.y1)
}
fn finite(m: Matrix) -> bool {
    [m.a, m.b, m.c, m.d, m.e, m.f]
        .into_iter()
        .all(f64::is_finite)
}
fn rect_finite(r: Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].into_iter().all(f64::is_finite)
}
fn empty(r: Rect) -> bool {
    r.x0 >= r.x1 || r.y0 >= r.y1
}
fn intersection(a: Rect, b: Rect) -> Rect {
    Rect::new(
        a.x0.max(b.x0),
        a.y0.max(b.y0),
        a.x1.min(b.x1),
        a.y1.min(b.y1),
    )
}
fn union(a: Rect, b: Rect) -> Rect {
    Rect::new(
        a.x0.min(b.x0),
        a.y0.min(b.y0),
        a.x1.max(b.x1),
        a.y1.max(b.y1),
    )
}
fn path_bounds(items: &[PathItem], matrix: Matrix) -> Option<Rect> {
    items
        .iter()
        .map(|i| match i {
            PathItem::Line(a, b) => [*a, *b, *b, *b],
            PathItem::Curve(a, b, c, d) => [*a, *b, *c, *d],
            PathItem::Rect(r) => [
                Point::new(r.x0, r.y0),
                Point::new(r.x1, r.y0),
                Point::new(r.x0, r.y1),
                Point::new(r.x1, r.y1),
            ],
        })
        .flat_map(|p| p.into_iter())
        .map(|p| {
            let p = matrix.transform_point(p);
            Rect::new(p.x, p.y, p.x, p.y)
        })
        .reduce(union)
}
fn valid_op(op: &RenderOp, matrix: Matrix) -> bool {
    let valid_path = |items: &[PathItem]| {
        items.iter().all(|item| {
            let points = match item {
                PathItem::Line(a, b) => [*a, *b, *b, *b],
                PathItem::Curve(a, b, c, d) => [*a, *b, *c, *d],
                PathItem::Rect(r) => [
                    Point::new(r.x0, r.y0),
                    Point::new(r.x1, r.y0),
                    Point::new(r.x0, r.y1),
                    Point::new(r.x1, r.y1),
                ],
            };
            points.into_iter().all(|p| {
                let q = matrix.transform_point(p);
                p.x.is_finite() && p.y.is_finite() && q.x.is_finite() && q.y.is_finite()
            })
        })
    };
    match op {
        RenderOp::Fill { items, .. } | RenderOp::Clip { items, .. } => valid_path(items),
        RenderOp::Stroke {
            items, ctm, width, ..
        } => valid_path(items) && finite(*ctm) && finite(*ctm * matrix) && width.is_finite(),
        RenderOp::Text(t) => {
            finite(t.ctm)
                && finite(t.ctm * matrix)
                && t.stroke_width.is_finite()
                && t.glyphs.iter().all(|g| {
                    finite(g.render_matrix)
                        && finite(g.render_matrix * matrix)
                        && rect_finite(g.cell)
                })
        }
        RenderOp::Image(i) => finite(i.ctm) && finite(i.ctm * matrix),
        RenderOp::Shading(s) => finite(s.ctm) && finite(s.ctm * matrix),
        RenderOp::Save | RenderOp::Restore => true,
    }
}
fn kind(op: &RenderOp) -> &'static str {
    match op {
        RenderOp::Save => "save",
        RenderOp::Restore => "restore",
        RenderOp::Fill { .. } => "fill",
        RenderOp::Stroke { .. } => "stroke",
        RenderOp::Clip { .. } => "clip",
        RenderOp::Text(_) => "text",
        RenderOp::Image(_) => "image",
        RenderOp::Shading(_) => "shading",
    }
}

#[pyclass(name = "ReplayEvent", module = "pdfspine", frozen)]
pub(crate) struct PyReplayEvent {
    record: Arc<ApiDisplayList>,
    index: Option<usize>,
    kind: &'static str,
    sequence: usize,
    matrix: Matrix,
    bounds: Option<Rect>,
    area: Option<Rect>,
    depth: usize,
}

#[pymethods]
impl PyReplayEvent {
    #[getter]
    fn kind(&self) -> &'static str {
        self.kind
    }
    #[getter]
    fn sequence(&self) -> usize {
        self.sequence
    }
    #[getter]
    fn matrix(&self) -> MatrixTuple {
        mt(self.matrix)
    }
    #[getter]
    fn bounds(&self) -> Option<RectTuple> {
        self.bounds.map(rt)
    }
    #[getter]
    fn font_buffer<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyBytes>>> {
        self.font_data()
            .map(|x| x.map(|(_, b)| PyBytes::new(py, &b)))
    }
    #[getter]
    fn font_format(&self) -> PyResult<Option<String>> {
        Ok(self.font_data()?.map(|x| x.0))
    }
    #[getter]
    fn image_bytes<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyBytes>>> {
        self.image_data()
            .map(|x| x.map(|(_, b)| PyBytes::new(py, &b)))
    }
    #[getter]
    fn image_format(&self) -> PyResult<Option<String>> {
        Ok(self.image_data()?.map(|x| x.0))
    }
    #[getter]
    fn payload<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let d = PyDict::new(py);
        d.set_item("op_index", self.index)?;
        match self.index.map(|i| &self.record.replay_ops()[i]) {
            None => {
                d.set_item("source_rect", self.record.rect())?;
                d.set_item("area", self.area.map(rt))?;
            }
            Some(RenderOp::Save | RenderOp::Restore) => d.set_item("depth", self.depth)?,
            Some(RenderOp::Fill {
                items,
                close,
                color,
                alpha,
                even_odd,
            }) => {
                d.set_item("path", path_payload(py, items)?)?;
                d.set_item("close", close)?;
                d.set_item("color", color)?;
                d.set_item("alpha", alpha)?;
                d.set_item("even_odd", even_odd)?;
            }
            Some(RenderOp::Stroke {
                items,
                close,
                color,
                alpha,
                width,
                ctm,
                dashes,
            }) => {
                d.set_item("path", path_payload(py, items)?)?;
                d.set_item("close", close)?;
                d.set_item("color", color)?;
                d.set_item("alpha", alpha)?;
                d.set_item("width", width)?;
                d.set_item("ctm", mt(*ctm))?;
                d.set_item("dashes", dashes)?;
            }
            Some(RenderOp::Clip { items, even_odd }) => {
                d.set_item("path", path_payload(py, items)?)?;
                d.set_item("even_odd", even_odd)?;
                d.set_item("depth", self.depth)?;
            }
            Some(RenderOp::Text(run)) => {
                let glyphs = run.glyphs.iter().enumerate().map(|(i, g)| {
                    let q = g.cell.quad().transform(&g.render_matrix);
                    (
                        g.unicode.as_str(),
                        g.code,
                        run.gids.get(i).copied(),
                        (g.origin.x, g.origin.y),
                        (
                            q.ul.x, q.ul.y, q.ur.x, q.ur.y, q.ll.x, q.ll.y, q.lr.x, q.lr.y,
                        ),
                        mt(g.render_matrix),
                    )
                });
                d.set_item("glyphs", PyTuple::new(py, glyphs)?)?;
                d.set_item(
                    "font_name",
                    run.glyphs.first().map(|g| g.font_name.as_str()),
                )?;
                d.set_item("resource_id", self.index)?;
                d.set_item("fill_color", run.fill_color)?;
                d.set_item("stroke_color", run.stroke_color)?;
                d.set_item("alpha", run.fill_alpha)?;
                d.set_item("render_mode", run.render_mode)?;
                d.set_item("stroke_width", run.stroke_width)?;
                d.set_item("ctm", mt(run.ctm))?;
            }
            Some(RenderOp::Image(image)) => {
                d.set_item("ctm", mt(image.ctm))?;
                d.set_item("alpha", image.alpha)?;
                d.set_item("fill_color", image.fill_color)?;
                d.set_item("resource_id", self.index)?;
                for (key, short, full) in [("width", "W", "Width"), ("height", "H", "Height")] {
                    d.set_item(
                        key,
                        image
                            .dict
                            .get(&pdf_api::replay::Name::new(full))
                            .or_else(|| image.dict.get(&pdf_api::replay::Name::new(short)))
                            .and_then(pdf_api::replay::Object::as_i64),
                    )?;
                }
            }
            Some(RenderOp::Shading(shade)) => {
                d.set_item("ctm", mt(shade.ctm))?;
                d.set_item("alpha", shade.alpha)?;
                d.set_item("resource_id", self.index)?;
                d.set_item(
                    "shading_type",
                    shade
                        .dict
                        .get(&pdf_api::replay::Name::new("ShadingType"))
                        .and_then(pdf_api::replay::Object::as_i64),
                )?;
            }
        }
        py.import("types")?.getattr("MappingProxyType")?.call1((d,))
    }
}
impl PyReplayEvent {
    fn font_data(&self) -> PyResult<Option<(String, Vec<u8>)>> {
        match self.index {
            Some(i) => self.record.replay_font(i).map_err(map_err),
            None => Ok(None),
        }
    }
    fn image_data(&self) -> PyResult<Option<(String, Vec<u8>)>> {
        match self.index {
            Some(i) => self.record.replay_image(i).map_err(map_err),
            None => Ok(None),
        }
    }
}
fn path_payload<'py>(py: Python<'py>, items: &[PathItem]) -> PyResult<Bound<'py, PyTuple>> {
    let mut out = Vec::with_capacity(items.len());
    for i in items {
        let object = match i {
            PathItem::Line(a, b) => ("l", (a.x, a.y), (b.x, b.y))
                .into_pyobject(py)?
                .into_any()
                .unbind(),
            PathItem::Curve(a, b, c, d) => ("c", (a.x, a.y), (b.x, b.y), (c.x, c.y), (d.x, d.y))
                .into_pyobject(py)?
                .into_any()
                .unbind(),
            PathItem::Rect(r) => ("re", rt(*r)).into_pyobject(py)?.into_any().unbind(),
        };
        out.push(object);
    }
    PyTuple::new(py, out)
}

pub(crate) fn prepare(
    record: Arc<ApiDisplayList>,
    matrix: Option<MatrixTuple>,
    area: Option<RectTuple>,
) -> PyResult<Vec<PyReplayEvent>> {
    let m = matrix.map_or(Matrix::IDENTITY, |(a, b, c, d, e, f)| {
        Matrix::new(a, b, c, d, e, f)
    });
    let matrix = record.replay_matrix(m);
    let area = area.map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1, y1));
    if !finite(m) || !finite(matrix) || area.is_some_and(|r| !rect_finite(r)) {
        return Err(PyValueError::new_err("replay geometry must be finite"));
    }
    let mut events = Vec::new();
    events.push(PyReplayEvent {
        record: record.clone(),
        index: None,
        kind: "begin",
        sequence: 0,
        matrix,
        bounds: None,
        area,
        depth: 0,
    });
    let mut clip: Option<Rect> = None;
    let mut stack = Vec::new();
    for (index, op) in record.replay_ops().iter().enumerate() {
        if !valid_op(op, matrix) {
            return Err(PyValueError::new_err(
                "recorded replay geometry is nonfinite or overflows",
            ));
        }
        let state = matches!(
            op,
            RenderOp::Save | RenderOp::Restore | RenderOp::Clip { .. }
        );
        let bounds = match op {
            RenderOp::Fill { items, .. } | RenderOp::Clip { items, .. } => {
                path_bounds(items, matrix)
            }
            RenderOp::Image(i) => Some(
                Rect::new(0.0, 0.0, 1.0, 1.0)
                    .quad()
                    .transform(&(i.ctm * matrix))
                    .rect(),
            ),
            // No recorded join/miter limit: retaining strokes is safer than an underestimated bbox.
            // Advance cells do not bound italic/Type3 ink. Do not cull text by an
            // unproven glyph envelope; payload still carries exact cell geometry.
            _ => None,
        };
        if bounds.is_some_and(|r| !rect_finite(r)) {
            return Err(PyValueError::new_err("replay transformed bounds overflow"));
        }
        match op {
            RenderOp::Save => stack.push(clip),
            RenderOp::Restore => {
                if let Some(saved) = stack.pop() {
                    clip = saved;
                }
            }
            RenderOp::Clip { .. } => {
                if let Some(b) = bounds {
                    clip = Some(clip.map_or(b, |old| intersection(old, b)));
                }
            }
            _ => {}
        }
        let paint_bounds = match (bounds, clip) {
            (Some(b), Some(c)) => Some(intersection(b, c)),
            (b, None) => b,
            (None, c) => c,
        };
        // Text clip modes affect later operations; retain those stateful text events conservatively.
        let text_clip = matches!(op,RenderOp::Text(t) if t.render_mode>=4);
        if !state
            && !text_clip
            && (area.is_some_and(empty)
                || clip.is_some_and(empty)
                || area
                    .zip(paint_bounds)
                    .is_some_and(|(a, b)| empty(intersection(a, b))))
        {
            continue;
        }
        if !state && area.is_some_and(empty) {
            continue;
        }
        events.push(PyReplayEvent {
            record: record.clone(),
            index: Some(index),
            kind: kind(op),
            sequence: events.len(),
            matrix,
            bounds,
            area,
            depth: stack.len(),
        });
    }
    events.push(PyReplayEvent {
        record,
        index: None,
        kind: "end",
        sequence: events.len(),
        matrix,
        bounds: None,
        area,
        depth: 0,
    });
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transformed_control_hull_contains_curve_and_applies_matrix_once() {
        let items = [PathItem::Curve(
            Point::new(1.0, 2.0),
            Point::new(8.0, -3.0),
            Point::new(-4.0, 9.0),
            Point::new(3.0, 7.0),
        )];
        let matrix = Matrix::new(0.0, 3.0, 2.0, 0.0, 5.0, 7.0);
        assert_eq!(
            path_bounds(&items, matrix).map(rt),
            Some((-1.0, -5.0, 23.0, 31.0))
        );
    }

    #[test]
    fn nonfinite_control_points_cannot_be_hidden_by_min_max() {
        let op = RenderOp::Fill {
            items: vec![PathItem::Curve(
                Point::new(0.0, 0.0),
                Point::new(f64::NAN, 1.0),
                Point::new(2.0, 2.0),
                Point::new(3.0, 3.0),
            )],
            close: false,
            color: 0,
            alpha: 255,
            even_odd: false,
        };
        assert!(!valid_op(&op, Matrix::IDENTITY));
    }

    #[test]
    fn finite_input_that_overflows_transform_is_rejected() {
        let op = RenderOp::Fill {
            items: vec![PathItem::Line(Point::new(1e308, 0.0), Point::new(0.0, 0.0))],
            close: false,
            color: 0,
            alpha: 255,
            even_odd: false,
        };
        assert!(!valid_op(&op, Matrix::new(2.0, 0.0, 0.0, 1.0, 0.0, 0.0)));
    }
}
