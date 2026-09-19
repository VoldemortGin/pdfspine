use pdf_api::{PaintDisposition, PaintProfile, ResourceOrigin};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

#[pyclass(name = "PaintProfile", module = "pdfspine", frozen)]
pub(crate) struct PyPaintProfile {
    inner: PaintProfile,
}

impl PyPaintProfile {
    pub(crate) fn new(inner: PaintProfile) -> Self {
        Self { inner }
    }
}

fn readonly<'py>(py: Python<'py>, dict: &Bound<'py, PyDict>) -> PyResult<Bound<'py, PyAny>> {
    py.import("types")?
        .getattr("MappingProxyType")?
        .call1((dict,))
}

fn disposition(value: PaintDisposition) -> &'static str {
    match value {
        PaintDisposition::Supported => "supported",
        PaintDisposition::Unsupported => "unsupported",
        PaintDisposition::Malformed => "malformed",
    }
}

fn origin(value: ResourceOrigin) -> &'static str {
    match value {
        ResourceOrigin::Direct => "direct",
        ResourceOrigin::Inherited => "inherited",
        ResourceOrigin::ParentFallback => "parent_fallback",
    }
}

#[pymethods]
impl PyPaintProfile {
    #[getter]
    fn version(&self) -> &'static str {
        self.inner.version
    }

    #[getter]
    fn complete(&self) -> bool {
        self.inner.complete
    }

    #[getter]
    fn resource_scopes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let mut scopes = Vec::with_capacity(self.inner.resource_scopes.len());
        for scope in &self.inner.resource_scopes {
            let mut entry_objects = Vec::with_capacity(scope.entries.len());
            for entry in &scope.entries {
                let dict = PyDict::new(py);
                dict.set_item("category", &entry.category)?;
                dict.set_item("name", &entry.name)?;
                dict.set_item("object_ref", entry.object_ref.map(|r| (r.num, r.gen)))?;
                dict.set_item("value_kind", &entry.value_kind)?;
                dict.set_item("selected", entry.selected)?;
                if let Some(state) = &entry.ext_gstate {
                    let value = PyDict::new(py);
                    value.set_item("line_width", state.line_width)?;
                    value.set_item("line_cap", state.line_cap)?;
                    value.set_item("line_join", state.line_join)?;
                    value.set_item("miter_limit", state.miter_limit)?;
                    value.set_item("stroke_alpha", state.stroke_alpha)?;
                    value.set_item("fill_alpha", state.fill_alpha)?;
                    value.set_item("blend_mode", &state.blend_mode)?;
                    value.set_item("soft_mask", &state.soft_mask)?;
                    value.set_item(
                        "unsupported_keys",
                        PyTuple::new(py, &state.unsupported_keys)?,
                    )?;
                    dict.set_item("ext_gstate", readonly(py, &value)?)?;
                } else {
                    dict.set_item("ext_gstate", py.None())?;
                }
                entry_objects.push(readonly(py, &dict)?.unbind());
            }
            let entries = PyTuple::new(py, entry_objects)?;
            let dict = PyDict::new(py);
            dict.set_item("scope_id", &scope.scope_id)?;
            dict.set_item("kind", scope.kind)?;
            dict.set_item("object_ref", scope.object_ref.map(|r| (r.num, r.gen)))?;
            dict.set_item("parent_scope_id", &scope.parent_scope_id)?;
            dict.set_item("origin", origin(scope.origin))?;
            dict.set_item("resource_sha256", &scope.resource_sha256)?;
            dict.set_item("entries", entries)?;
            dict.set_item("group_keys", PyTuple::new(py, &scope.group_keys)?)?;
            if let Some(group) = &scope.group {
                let value = PyDict::new(py);
                value.set_item(
                    "object_ref",
                    group
                        .object_ref
                        .map(|reference| (reference.num, reference.gen)),
                )?;
                value.set_item("type", &group.group_type)?;
                value.set_item("subtype", &group.subtype)?;
                value.set_item("color_space", &group.color_space)?;
                value.set_item("isolated", group.isolated)?;
                value.set_item("knockout", group.knockout)?;
                value.set_item("keys", PyTuple::new(py, &group.keys)?)?;
                value.set_item(
                    "unsupported_fields",
                    PyTuple::new(py, &group.unsupported_fields)?,
                )?;
                dict.set_item("group", readonly(py, &value)?)?;
            } else {
                dict.set_item("group", py.None())?;
            }
            scopes.push(readonly(py, &dict)?.unbind());
        }
        PyTuple::new(py, scopes)
    }

    #[getter]
    fn operators<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let mut objects = Vec::with_capacity(self.inner.operators.len());
        for operator in &self.inner.operators {
            let dict = PyDict::new(py);
            dict.set_item("scope_id", &operator.scope_id)?;
            dict.set_item("ordinal", operator.ordinal)?;
            dict.set_item("mnemonic", &operator.mnemonic)?;
            dict.set_item("disposition", disposition(operator.disposition))?;
            dict.set_item("resource_name", &operator.resource_name)?;
            dict.set_item(
                "numeric_operands",
                PyTuple::new(py, &operator.numeric_operands)?,
            )?;
            objects.push(readonly(py, &dict)?.unbind());
        }
        PyTuple::new(py, objects)
    }

    #[getter]
    fn inline_images<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let mut objects = Vec::with_capacity(self.inner.inline_images.len());
        for image in &self.inner.inline_images {
            let dict = PyDict::new(py);
            dict.set_item("scope_id", &image.scope_id)?;
            dict.set_item("ordinal", image.ordinal)?;
            dict.set_item("parameter_keys", PyTuple::new(py, &image.parameter_keys)?)?;
            dict.set_item("disposition", disposition(image.disposition))?;
            objects.push(readonly(py, &dict)?.unbind());
        }
        PyTuple::new(py, objects)
    }

    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let mut objects = Vec::with_capacity(self.inner.diagnostics.len());
        for diagnostic in &self.inner.diagnostics {
            let dict = PyDict::new(py);
            dict.set_item("code", diagnostic.code)?;
            dict.set_item("scope_id", &diagnostic.scope_id)?;
            dict.set_item("operator_ordinal", diagnostic.operator_ordinal)?;
            objects.push(readonly(py, &dict)?.unbind());
        }
        PyTuple::new(py, objects)
    }
}
