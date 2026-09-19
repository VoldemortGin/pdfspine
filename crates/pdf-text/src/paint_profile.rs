//! Strict, read-only accounting of content operators and selected resources.

use std::collections::HashSet;
use std::fmt::Write;
use std::sync::Arc;

use pdf_core::{Dict, DocumentStore, Name, ObjRef, Object};
use sha2::{Digest, Sha256};

use crate::tokenizer::{tokenize_audited, Event, TokenIssueKind};

/// Version of the strict paint-profile contract.
pub const PAINT_PROFILE_VERSION: &str = "strict-paint-profile-v1";
const MAX_FORM_DEPTH: u32 = 16;

/// Whether an observed content construct is represented by the audited path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintDisposition {
    /// The construct is understood by the current audited interpreter.
    Supported,
    /// The construct is valid PDF syntax but its paint semantics are incomplete.
    Unsupported,
    /// The construct or selected resource is malformed or unresolved.
    Malformed,
}

/// How a resource scope obtained its dictionary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceOrigin {
    /// The page or Form supplied its own resource dictionary.
    Direct,
    /// A page-tree ancestor supplied the page's effective resource dictionary.
    Inherited,
    /// A Form omitted `/Resources` and inherited its caller's scope.
    ParentFallback,
}

/// One stable fail-closed paint diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintDiagnostic {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Resource/content scope in which it occurred.
    pub scope_id: String,
    /// Operator ordinal when the fault is tied to an executed operator.
    pub operator_ordinal: Option<usize>,
}

/// One interpreted content operator.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintOperator {
    /// Resource/content scope.
    pub scope_id: String,
    /// Zero-based operator ordinal within this scope.
    pub ordinal: usize,
    /// PDF operator mnemonic.
    pub mnemonic: String,
    /// Current interpreter support status.
    pub disposition: PaintDisposition,
    /// Selected resource name for `Do`, `gs`, `sh`, `CS/cs`, when present.
    pub resource_name: Option<String>,
    /// Numeric operands in source order. This preserves bounded graphics-state
    /// facts such as `J`, `j`, `M`, `w` and dash phase without claiming that a
    /// renderer applied them.
    pub numeric_operands: Vec<f64>,
}

/// Typed selected ExtGState facts. Missing fields retain PDF defaults; unknown
/// keys remain explicit and make a selected state unsupported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtGStateProfile {
    /// `/LW` line width.
    pub line_width: Option<f64>,
    /// `/LC` line cap integer.
    pub line_cap: Option<i64>,
    /// `/LJ` line join integer.
    pub line_join: Option<i64>,
    /// `/ML` miter limit.
    pub miter_limit: Option<f64>,
    /// `/CA` stroke alpha.
    pub stroke_alpha: Option<f64>,
    /// `/ca` fill alpha.
    pub fill_alpha: Option<f64>,
    /// `/BM` name when represented as one name.
    pub blend_mode: Option<String>,
    /// `/SMask`: `None`, `absent`, or the selected unsupported value kind.
    pub soft_mask: String,
    /// Sorted keys whose semantics are outside the bounded profile.
    pub unsupported_keys: Vec<String>,
}

/// One declared resource entry in a resource scope.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintResourceEntry {
    /// Resource category such as `ExtGState`, `XObject`, `Pattern`, `Shading`.
    pub category: String,
    /// Resource name within the category.
    pub name: String,
    /// Indirect object identity when present.
    pub object_ref: Option<ObjRef>,
    /// Resolved object kind (`dictionary`, `stream`, `array`, ...).
    pub value_kind: String,
    /// Whether an executed operator selected this entry.
    pub selected: bool,
    /// Selected ExtGState facts, if this is an ExtGState dictionary.
    pub ext_gstate: Option<ExtGStateProfile>,
}

/// Typed transparency-group attributes. The initial strict profile only accepts
/// a page-level DeviceRGB group with absent/false isolation and knockout flags;
/// Form groups remain unsupported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransparencyGroupProfile {
    /// Indirect identity when `/Group` is a reference.
    pub object_ref: Option<ObjRef>,
    /// `/Type` name.
    pub group_type: Option<String>,
    /// `/S` name.
    pub subtype: Option<String>,
    /// `/CS` name when it is a direct device colour-space name.
    pub color_space: Option<String>,
    /// `/I`, preserving absent versus explicit false/true.
    pub isolated: Option<bool>,
    /// `/K`, preserving absent versus explicit false/true.
    pub knockout: Option<bool>,
    /// Deterministically sorted keys present in the dictionary.
    pub keys: Vec<String>,
    /// Unknown keys or known keys whose values had unsupported types.
    pub unsupported_fields: Vec<String>,
}

/// A page or Form resource scope.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintResourceScope {
    /// Stable scope id (`page:<xref>` or `form:<xref-or-direct>:<ordinal>`).
    pub scope_id: String,
    /// `page` or `form`.
    pub kind: &'static str,
    /// Owning indirect object, if any.
    pub object_ref: Option<ObjRef>,
    /// Parent scope for a Form.
    pub parent_scope_id: Option<String>,
    /// Resource dictionary origin.
    pub origin: ResourceOrigin,
    /// SHA-256 of deterministic pdfspine serialization of this effective
    /// resource dictionary.
    pub resource_sha256: String,
    /// Deterministically sorted declared entries.
    pub entries: Vec<PaintResourceEntry>,
    /// Page/Form `/Group` keys.
    pub group_keys: Vec<String>,
    /// Typed page/Form transparency-group attributes, when present.
    pub group: Option<TransparencyGroupProfile>,
}

/// One inline-image observation. Bodies are deliberately not exposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineImageProfile {
    /// Containing content scope.
    pub scope_id: String,
    /// Operator ordinal.
    pub ordinal: usize,
    /// Parsed parameter keys.
    pub parameter_keys: Vec<String>,
    /// Current strict support status.
    pub disposition: PaintDisposition,
}

/// Immutable strict accounting result for one page.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintProfile {
    /// Contract version.
    pub version: &'static str,
    /// True only when no fail-closed diagnostic was recorded.
    pub complete: bool,
    /// Effective page + reached Form resource scopes.
    pub resource_scopes: Vec<PaintResourceScope>,
    /// Executed operators, in scope-local order.
    pub operators: Vec<PaintOperator>,
    /// Inline images without payload bytes.
    pub inline_images: Vec<InlineImageProfile>,
    /// Stable diagnostics.
    pub diagnostics: Vec<PaintDiagnostic>,
}

struct Builder<'a> {
    doc: &'a DocumentStore,
    scopes: Vec<PaintResourceScope>,
    operators: Vec<PaintOperator>,
    inline_images: Vec<InlineImageProfile>,
    diagnostics: Vec<PaintDiagnostic>,
}

/// Builds a strict profile from the page leaf and its already-resolved effective
/// resources. The same effective dictionary must be supplied to page replay.
#[must_use]
pub fn build_paint_profile(
    doc: &DocumentStore,
    page_ref: ObjRef,
    page: &Dict,
    effective_resources: Dict,
    inherited: bool,
) -> PaintProfile {
    let scope_id = format!("page:{}", page_ref.num);
    let mut builder = Builder {
        doc,
        scopes: Vec::new(),
        operators: Vec::new(),
        inline_images: Vec::new(),
        diagnostics: Vec::new(),
    };
    let origin = if inherited {
        ResourceOrigin::Inherited
    } else {
        ResourceOrigin::Direct
    };
    let page_group = builder.inspect_group(&scope_id, page.get(&Name::new("Group")), true);
    builder.push_scope(
        &scope_id,
        "page",
        Some(page_ref),
        None,
        origin,
        &effective_resources,
        page_group,
    );
    let content = builder.page_content(page, &scope_id);
    builder.walk_content(
        &content,
        &effective_resources,
        &scope_id,
        0,
        &mut HashSet::new(),
    );
    PaintProfile {
        version: PAINT_PROFILE_VERSION,
        complete: builder.diagnostics.is_empty(),
        resource_scopes: builder.scopes,
        operators: builder.operators,
        inline_images: builder.inline_images,
        diagnostics: builder.diagnostics,
    }
}

impl Builder<'_> {
    fn diagnostic(&mut self, code: &'static str, scope_id: &str, ordinal: Option<usize>) {
        self.diagnostics.push(PaintDiagnostic {
            code,
            scope_id: scope_id.to_string(),
            operator_ordinal: ordinal,
        });
    }

    fn page_content(&mut self, page: &Dict, scope_id: &str) -> Vec<u8> {
        let Some(raw_contents) = page.get(&Name::new("Contents")) else {
            return Vec::new();
        };
        let contents = match resolve_value(self.doc, raw_contents) {
            Ok(contents) => contents,
            Err(()) => {
                self.diagnostic("content_reference_unresolved", scope_id, None);
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        match contents.as_ref() {
            Object::Stream(_) => self.append_content(contents, scope_id, &mut out),
            Object::Array(items) => {
                for item in items {
                    match resolve_value(self.doc, item) {
                        Ok(value) => self.append_content(value, scope_id, &mut out),
                        Err(()) => self.diagnostic("content_reference_unresolved", scope_id, None),
                    }
                }
            }
            _ => self.diagnostic("contents_wrong_type", scope_id, None),
        }
        out
    }

    fn append_content(&mut self, obj: Arc<Object>, scope_id: &str, out: &mut Vec<u8>) {
        let Some(stream) = obj.as_stream() else {
            self.diagnostic("content_not_stream", scope_id, None);
            return;
        };
        match self
            .doc
            .decode_stream(stream)
            .and_then(|v| v.into_decoded())
        {
            Ok(bytes) => {
                if !out.is_empty() {
                    out.push(b'\n');
                }
                out.extend_from_slice(&bytes);
            }
            Err(_) => self.diagnostic("content_decode_failed", scope_id, None),
        }
    }

    fn walk_content(
        &mut self,
        content: &[u8],
        resources: &Dict,
        scope_id: &str,
        depth: u32,
        visited: &mut HashSet<u32>,
    ) {
        let tokenized = tokenize_audited(content);
        for issue in tokenized.issues {
            let code = match issue.kind {
                TokenIssueKind::LexerRecovery => "content_lexer_recovery",
                TokenIssueKind::UnexpectedToken => "content_unexpected_token",
                TokenIssueKind::InlineImageMissingDataMarker => "inline_image_missing_id",
                TokenIssueKind::InlineImageMissingTerminator => "inline_image_missing_ei",
                TokenIssueKind::UnterminatedArray => "unterminated_array",
                TokenIssueKind::UnterminatedDictionary => "unterminated_dictionary",
                TokenIssueKind::MissingDictionaryValue => "missing_dictionary_value",
            };
            self.diagnostic(code, scope_id, None);
        }
        let mut operands = Vec::new();
        let mut ordinal = 0usize;
        let mut graphics_depth = 0usize;
        let mut text_depth = 0usize;
        let mut marked_content_depth = 0usize;
        for event in tokenized.events {
            match event {
                Event::Operand(value) => operands.push(value),
                Event::InlineImage { params, .. } => {
                    let keys = params
                        .as_dict()
                        .map(|d| {
                            d.keys()
                                .filter_map(Name::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    self.inline_images.push(InlineImageProfile {
                        scope_id: scope_id.to_string(),
                        ordinal,
                        parameter_keys: keys,
                        disposition: PaintDisposition::Unsupported,
                    });
                    self.operators.push(PaintOperator {
                        scope_id: scope_id.to_string(),
                        ordinal,
                        mnemonic: "BI".to_string(),
                        disposition: PaintDisposition::Unsupported,
                        resource_name: None,
                        numeric_operands: Vec::new(),
                    });
                    self.diagnostic(
                        "inline_image_not_strictly_accounted",
                        scope_id,
                        Some(ordinal),
                    );
                    ordinal += 1;
                    operands.clear();
                }
                Event::Operator(bytes) => {
                    let mnemonic = String::from_utf8_lossy(&bytes).into_owned();
                    let resource_name = selected_name(&mnemonic, &operands);
                    let numeric_operands = operands.iter().filter_map(Object::as_f64).collect();
                    let mut disposition = operator_disposition(&mnemonic);
                    if let Some(code) = validate_operator(&mnemonic, &operands) {
                        disposition = PaintDisposition::Malformed;
                        self.diagnostic(code, scope_id, Some(ordinal));
                    }
                    match mnemonic.as_str() {
                        "q" if disposition == PaintDisposition::Supported => graphics_depth += 1,
                        "Q" if disposition == PaintDisposition::Supported => {
                            if graphics_depth == 0 {
                                disposition = PaintDisposition::Malformed;
                                self.diagnostic(
                                    "graphics_state_stack_underflow",
                                    scope_id,
                                    Some(ordinal),
                                );
                            } else {
                                graphics_depth -= 1;
                            }
                        }
                        "BT" if disposition == PaintDisposition::Supported => {
                            if text_depth != 0 {
                                disposition = PaintDisposition::Malformed;
                                self.diagnostic("nested_text_object", scope_id, Some(ordinal));
                            } else {
                                text_depth = 1;
                            }
                        }
                        "ET" if disposition == PaintDisposition::Supported => {
                            if text_depth == 0 {
                                disposition = PaintDisposition::Malformed;
                                self.diagnostic("text_object_underflow", scope_id, Some(ordinal));
                            } else {
                                text_depth -= 1;
                            }
                        }
                        "BMC" | "BDC" if disposition == PaintDisposition::Supported => {
                            marked_content_depth += 1;
                        }
                        "EMC" if disposition == PaintDisposition::Supported => {
                            if marked_content_depth == 0 {
                                disposition = PaintDisposition::Malformed;
                                self.diagnostic(
                                    "marked_content_underflow",
                                    scope_id,
                                    Some(ordinal),
                                );
                            } else {
                                marked_content_depth -= 1;
                            }
                        }
                        _ => {}
                    }
                    match mnemonic.as_str() {
                        "gs" => {
                            if let Some(name) = resource_name.as_deref() {
                                disposition =
                                    self.inspect_extgstate(resources, scope_id, ordinal, name);
                            } else {
                                disposition = PaintDisposition::Malformed;
                                self.diagnostic("gs_missing_name", scope_id, Some(ordinal));
                            }
                        }
                        "Do" => {
                            if let Some(name) = resource_name.as_deref() {
                                disposition = self.walk_xobject(
                                    resources, scope_id, ordinal, name, depth, visited,
                                );
                            } else {
                                disposition = PaintDisposition::Malformed;
                                self.diagnostic("do_missing_name", scope_id, Some(ordinal));
                            }
                        }
                        "sh" => {
                            if let Some(name) = resource_name.as_deref() {
                                self.mark_resource_selected(scope_id, "Shading", name);
                            }
                            disposition = PaintDisposition::Unsupported;
                            self.diagnostic(
                                "shading_not_strictly_accounted",
                                scope_id,
                                Some(ordinal),
                            );
                        }
                        "CS" | "cs" => {
                            if disposition != PaintDisposition::Malformed {
                                disposition = self.inspect_colorspace(
                                    resources,
                                    scope_id,
                                    ordinal,
                                    resource_name.as_deref(),
                                );
                            }
                        }
                        "Tf" => {
                            if disposition != PaintDisposition::Malformed {
                                disposition = self.inspect_font(
                                    resources,
                                    scope_id,
                                    ordinal,
                                    resource_name.as_deref(),
                                );
                            }
                        }
                        "Tr" if disposition == PaintDisposition::Supported
                            && operands[0].as_i64().is_some_and(|value| value >= 4) =>
                        {
                            disposition = PaintDisposition::Unsupported;
                            self.diagnostic(
                                "text_clipping_not_strictly_accounted",
                                scope_id,
                                Some(ordinal),
                            );
                        }
                        "BDC" | "DP" => {
                            if is_optional_content(&operands) {
                                if let Some(name) = operands
                                    .iter()
                                    .rev()
                                    .find_map(Object::as_name)
                                    .and_then(Name::as_str)
                                {
                                    self.mark_resource_selected(scope_id, "Properties", name);
                                }
                                disposition = PaintDisposition::Unsupported;
                                self.diagnostic(
                                    "optional_content_not_strictly_accounted",
                                    scope_id,
                                    Some(ordinal),
                                );
                            }
                        }
                        "scn" | "SCN" if operands.iter().any(|v| v.as_name().is_some()) => {
                            if let Some(name) = operands
                                .last()
                                .and_then(Object::as_name)
                                .and_then(Name::as_str)
                            {
                                self.mark_resource_selected(scope_id, "Pattern", name);
                            }
                            disposition = PaintDisposition::Unsupported;
                            self.diagnostic(
                                "pattern_not_strictly_accounted",
                                scope_id,
                                Some(ordinal),
                            );
                        }
                        _ if disposition == PaintDisposition::Unsupported => {
                            self.diagnostic(
                                "unsupported_content_operator",
                                scope_id,
                                Some(ordinal),
                            );
                        }
                        _ => {}
                    }
                    self.operators.push(PaintOperator {
                        scope_id: scope_id.to_string(),
                        ordinal,
                        mnemonic,
                        disposition,
                        resource_name,
                        numeric_operands,
                    });
                    ordinal += 1;
                    operands.clear();
                }
            }
        }
        if !operands.is_empty() {
            self.diagnostic("trailing_operands", scope_id, None);
        }
        if graphics_depth != 0 {
            self.diagnostic("graphics_state_stack_unbalanced", scope_id, None);
        }
        if text_depth != 0 {
            self.diagnostic("text_object_unbalanced", scope_id, None);
        }
        if marked_content_depth != 0 {
            self.diagnostic("marked_content_unbalanced", scope_id, None);
        }
    }

    fn walk_xobject(
        &mut self,
        resources: &Dict,
        parent_scope: &str,
        ordinal: usize,
        name: &str,
        depth: u32,
        visited: &mut HashSet<u32>,
    ) -> PaintDisposition {
        let Some(xobjects) = resolve_key(self.doc, resources, "XObject") else {
            self.diagnostic("xobject_dictionary_unresolved", parent_scope, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(xdict) = xobjects.as_dict() else {
            self.diagnostic("xobject_dictionary_wrong_type", parent_scope, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let key = Name::new(name);
        let Some(raw) = xdict.get(&key) else {
            self.diagnostic("xobject_missing", parent_scope, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        self.mark_resource_selected(parent_scope, "XObject", name);
        let object_ref = raw.as_reference();
        let Ok(value) = resolve_value(self.doc, raw) else {
            self.diagnostic("xobject_reference_unresolved", parent_scope, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(stream) = value.as_stream() else {
            self.diagnostic("xobject_wrong_type", parent_scope, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        match stream
            .dict
            .get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .and_then(Name::as_str)
        {
            Some("Image") => {
                self.diagnostic("image_not_strictly_accounted", parent_scope, Some(ordinal));
                PaintDisposition::Unsupported
            }
            Some("Form") | None => {
                if depth + 1 > MAX_FORM_DEPTH {
                    self.diagnostic("form_depth_exceeded", parent_scope, Some(ordinal));
                    return PaintDisposition::Malformed;
                }
                if let Some(reference) = object_ref {
                    if !visited.insert(reference.num) {
                        self.diagnostic("form_cycle", parent_scope, Some(ordinal));
                        return PaintDisposition::Malformed;
                    }
                }
                let child_scope = format!(
                    "{parent_scope}/form:{}:{ordinal}",
                    object_ref.map_or_else(|| "direct".to_string(), |r| r.num.to_string()),
                );
                if stream.dict.contains_key(&Name::new("OC")) {
                    self.diagnostic("form_optional_content_unsupported", &child_scope, None);
                }
                if !valid_form_bbox(&stream.dict) {
                    self.diagnostic("form_bbox_invalid", &child_scope, None);
                }
                if !valid_optional_matrix(&stream.dict) {
                    self.diagnostic("form_matrix_invalid", &child_scope, None);
                }
                let (form_resources, origin) = match stream.dict.get(&Name::new("Resources")) {
                    None | Some(Object::Null) => {
                        (resources.clone(), ResourceOrigin::ParentFallback)
                    }
                    Some(raw_resources) => match resolve_value(self.doc, raw_resources) {
                        Ok(value) => match value.as_dict() {
                            Some(dict) => (dict.clone(), ResourceOrigin::Direct),
                            None => {
                                self.diagnostic("form_resources_wrong_type", &child_scope, None);
                                (Dict::new(), ResourceOrigin::Direct)
                            }
                        },
                        Err(()) => {
                            self.diagnostic("form_resources_unresolved", &child_scope, None);
                            (Dict::new(), ResourceOrigin::Direct)
                        }
                    },
                };
                let group =
                    self.inspect_group(&child_scope, stream.dict.get(&Name::new("Group")), false);
                self.push_scope(
                    &child_scope,
                    "form",
                    object_ref,
                    Some(parent_scope.to_string()),
                    origin,
                    &form_resources,
                    group,
                );
                match self
                    .doc
                    .decode_stream(stream)
                    .and_then(|v| v.into_decoded())
                {
                    Ok(bytes) => {
                        self.walk_content(&bytes, &form_resources, &child_scope, depth + 1, visited)
                    }
                    Err(_) => self.diagnostic("form_decode_failed", &child_scope, None),
                }
                if let Some(reference) = object_ref {
                    visited.remove(&reference.num);
                }
                PaintDisposition::Supported
            }
            _ => {
                self.diagnostic("xobject_subtype_unsupported", parent_scope, Some(ordinal));
                PaintDisposition::Unsupported
            }
        }
    }

    fn inspect_extgstate(
        &mut self,
        resources: &Dict,
        scope_id: &str,
        ordinal: usize,
        name: &str,
    ) -> PaintDisposition {
        let Some(states) = resolve_key(self.doc, resources, "ExtGState") else {
            self.diagnostic("extgstate_dictionary_unresolved", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(states) = states.as_dict() else {
            self.diagnostic("extgstate_dictionary_wrong_type", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(raw) = states.get(&Name::new(name)) else {
            self.diagnostic("extgstate_missing", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Ok(value) = resolve_value(self.doc, raw) else {
            self.diagnostic("extgstate_reference_unresolved", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(dict) = value.as_dict() else {
            self.diagnostic("extgstate_wrong_type", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let profile = extgstate_profile(dict);
        self.mark_selected(scope_id, "ExtGState", name, profile.clone());
        let blend_ok = profile
            .blend_mode
            .as_deref()
            .is_none_or(|mode| mode == "Normal");
        let smask_ok = matches!(profile.soft_mask.as_str(), "absent" | "None");
        if !profile.unsupported_keys.is_empty() || !blend_ok || !smask_ok {
            self.diagnostic("extgstate_semantics_unsupported", scope_id, Some(ordinal));
            PaintDisposition::Unsupported
        } else {
            PaintDisposition::Supported
        }
    }

    fn inspect_colorspace(
        &mut self,
        resources: &Dict,
        scope_id: &str,
        ordinal: usize,
        name: Option<&str>,
    ) -> PaintDisposition {
        let Some(name) = name else {
            self.diagnostic("colorspace_missing_name", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        if matches!(name, "DeviceGray" | "DeviceRGB" | "DeviceCMYK") {
            return PaintDisposition::Supported;
        }
        self.mark_resource_selected(scope_id, "ColorSpace", name);
        let Some(spaces) = resolve_key(self.doc, resources, "ColorSpace") else {
            self.diagnostic("colorspace_dictionary_unresolved", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(spaces) = spaces.as_dict() else {
            self.diagnostic("colorspace_dictionary_wrong_type", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        if !spaces.contains_key(&Name::new(name)) {
            self.diagnostic("colorspace_missing", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        }
        self.diagnostic(
            "named_colorspace_not_strictly_accounted",
            scope_id,
            Some(ordinal),
        );
        PaintDisposition::Unsupported
    }

    fn inspect_font(
        &mut self,
        resources: &Dict,
        scope_id: &str,
        ordinal: usize,
        name: Option<&str>,
    ) -> PaintDisposition {
        let Some(name) = name else {
            self.diagnostic("font_missing_name", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        self.mark_resource_selected(scope_id, "Font", name);
        let Some(fonts) = resolve_key(self.doc, resources, "Font") else {
            self.diagnostic("font_dictionary_unresolved", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(fonts) = fonts.as_dict() else {
            self.diagnostic("font_dictionary_wrong_type", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        let Some(raw) = fonts.get(&Name::new(name)) else {
            self.diagnostic("font_missing", scope_id, Some(ordinal));
            return PaintDisposition::Malformed;
        };
        match resolve_value(self.doc, raw) {
            Ok(value) if value.as_dict().is_some() => PaintDisposition::Supported,
            Ok(_) => {
                self.diagnostic("font_wrong_type", scope_id, Some(ordinal));
                PaintDisposition::Malformed
            }
            Err(()) => {
                self.diagnostic("font_reference_unresolved", scope_id, Some(ordinal));
                PaintDisposition::Malformed
            }
        }
    }

    fn mark_selected(
        &mut self,
        scope_id: &str,
        category: &str,
        name: &str,
        profile: ExtGStateProfile,
    ) {
        self.mark_resource_selected(scope_id, category, name);
        if let Some(entry) = self
            .scopes
            .iter_mut()
            .find(|s| s.scope_id == scope_id)
            .and_then(|s| {
                s.entries
                    .iter_mut()
                    .find(|e| e.category == category && e.name == name)
            })
        {
            entry.ext_gstate = Some(profile);
        }
    }

    fn mark_resource_selected(&mut self, scope_id: &str, category: &str, name: &str) {
        if let Some(entry) = self
            .scopes
            .iter_mut()
            .find(|scope| scope.scope_id == scope_id)
            .and_then(|scope| {
                scope
                    .entries
                    .iter_mut()
                    .find(|entry| entry.category == category && entry.name == name)
            })
        {
            entry.selected = true;
        }
    }

    fn inspect_group(
        &mut self,
        scope_id: &str,
        raw: Option<&Object>,
        allow_bounded_page_group: bool,
    ) -> Option<TransparencyGroupProfile> {
        let raw = raw?;
        let object_ref = raw.as_reference();
        let value = match resolve_value(self.doc, raw) {
            Ok(value) => value,
            Err(()) => {
                self.diagnostic("transparency_group_reference_unresolved", scope_id, None);
                return None;
            }
        };
        let Some(dict) = value.as_dict() else {
            self.diagnostic("transparency_group_wrong_type", scope_id, None);
            return None;
        };
        let mut keys = dict
            .keys()
            .filter_map(Name::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        let mut unsupported_fields = keys
            .iter()
            .filter(|key| !matches!(key.as_str(), "Type" | "S" | "CS" | "I" | "K"))
            .cloned()
            .collect::<Vec<_>>();
        let group_type = group_name(dict, "Type", &mut unsupported_fields);
        let subtype = group_name(dict, "S", &mut unsupported_fields);
        let color_space = group_name(dict, "CS", &mut unsupported_fields);
        let isolated = group_bool(dict, "I", &mut unsupported_fields);
        let knockout = group_bool(dict, "K", &mut unsupported_fields);
        unsupported_fields.sort();
        unsupported_fields.dedup();
        let profile = TransparencyGroupProfile {
            object_ref,
            group_type,
            subtype,
            color_space,
            isolated,
            knockout,
            keys,
            unsupported_fields,
        };
        let bounded_page_group = allow_bounded_page_group
            && profile.group_type.as_deref() == Some("Group")
            && profile.subtype.as_deref() == Some("Transparency")
            && profile.color_space.as_deref() == Some("DeviceRGB")
            && profile.isolated.is_none_or(|value| !value)
            && profile.knockout.is_none_or(|value| !value)
            && profile.unsupported_fields.is_empty();
        if !bounded_page_group {
            self.diagnostic("transparency_group_unsupported", scope_id, None);
        }
        Some(profile)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_scope(
        &mut self,
        scope_id: &str,
        kind: &'static str,
        object_ref: Option<ObjRef>,
        parent_scope_id: Option<String>,
        origin: ResourceOrigin,
        resources: &Dict,
        group: Option<TransparencyGroupProfile>,
    ) {
        let mut entries = Vec::new();
        for category in [
            "ExtGState",
            "XObject",
            "Pattern",
            "Shading",
            "ColorSpace",
            "Properties",
            "Font",
        ] {
            let Some(values) = resolve_key(self.doc, resources, category) else {
                continue;
            };
            let Some(dict) = values.as_dict() else {
                continue;
            };
            for (name, raw) in dict {
                let name = name.as_str().unwrap_or_default().to_string();
                let object_ref = raw.as_reference();
                let (kind, ext_gstate) = match resolve_value(self.doc, raw) {
                    Ok(value) => (
                        object_kind(value.as_ref()).to_string(),
                        (category == "ExtGState")
                            .then(|| value.as_dict().map(extgstate_profile))
                            .flatten(),
                    ),
                    Err(()) => ("unresolved".to_string(), None),
                };
                entries.push(PaintResourceEntry {
                    category: category.to_string(),
                    name,
                    object_ref,
                    value_kind: kind,
                    selected: false,
                    ext_gstate,
                });
            }
        }
        entries.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
        let group_keys = group
            .as_ref()
            .map(|profile| profile.keys.clone())
            .unwrap_or_default();
        self.scopes.push(PaintResourceScope {
            scope_id: scope_id.to_string(),
            kind,
            object_ref,
            parent_scope_id,
            origin,
            resource_sha256: object_sha256(&Object::Dictionary(resources.clone())),
            entries,
            group_keys,
            group,
        });
    }
}

fn object_sha256(value: &Object) -> String {
    let bytes = pdf_core::serialize::write_object(value);
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn resolve_key(doc: &DocumentStore, dict: &Dict, key: &str) -> Option<Arc<Object>> {
    doc.resolve_dict_key(dict, &Name::new(key)).ok().flatten()
}

fn resolve_value(doc: &DocumentStore, value: &Object) -> Result<Arc<Object>, ()> {
    match value {
        Object::Reference(reference) => doc
            .resolve(*reference)
            .map_err(|_| ())
            .and_then(|value| (!value.is_null()).then_some(value).ok_or(())),
        direct => Ok(Arc::new(direct.clone())),
    }
}

fn object_kind(value: &Object) -> &'static str {
    match value {
        Object::Null => "null",
        Object::Boolean(_) => "boolean",
        Object::Integer(_) => "integer",
        Object::Real(_) => "real",
        Object::String(_) => "string",
        Object::Name(_) => "name",
        Object::Array(_) => "array",
        Object::Dictionary(_) => "dictionary",
        Object::Stream(_) => "stream",
        Object::Reference(_) => "reference",
    }
}

fn group_name(dict: &Dict, key: &str, unsupported_fields: &mut Vec<String>) -> Option<String> {
    match dict.get(&Name::new(key)) {
        None => None,
        Some(value) => match value.as_name().and_then(Name::as_str) {
            Some(value) => Some(value.to_string()),
            None => {
                unsupported_fields.push(format!("invalid:{key}"));
                None
            }
        },
    }
}

fn group_bool(dict: &Dict, key: &str, unsupported_fields: &mut Vec<String>) -> Option<bool> {
    match dict.get(&Name::new(key)) {
        None => None,
        Some(Object::Boolean(value)) => Some(*value),
        Some(_) => {
            unsupported_fields.push(format!("invalid:{key}"));
            None
        }
    }
}

fn extgstate_profile(dict: &Dict) -> ExtGStateProfile {
    const SUPPORTED: &[&str] = &["Type", "LW", "LC", "LJ", "ML", "CA", "ca", "BM", "SMask"];
    let mut unsupported_keys: Vec<String> = dict
        .keys()
        .filter_map(Name::as_str)
        .filter(|key| !SUPPORTED.contains(key))
        .map(str::to_string)
        .collect();
    for key in ["LW", "ML", "CA", "ca"] {
        if dict.contains_key(&Name::new(key))
            && dict.get(&Name::new(key)).and_then(Object::as_f64).is_none()
        {
            unsupported_keys.push(format!("invalid:{key}"));
        }
    }
    for (key, valid) in [("LC", 0..=2), ("LJ", 0..=2)] {
        if dict
            .get(&Name::new(key))
            .is_some_and(|value| value.as_i64().is_none_or(|value| !valid.contains(&value)))
        {
            unsupported_keys.push(format!("invalid:{key}"));
        }
    }
    if dict.get(&Name::new("ML")).is_some_and(|value| {
        value
            .as_f64()
            .is_none_or(|value| !value.is_finite() || value < 1.0)
    }) {
        unsupported_keys.push("invalid:ML".to_string());
    }
    for key in ["CA", "ca"] {
        if dict.get(&Name::new(key)).is_some_and(|value| {
            value
                .as_f64()
                .is_none_or(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        }) {
            unsupported_keys.push(format!("invalid:{key}"));
        } else if dict.get(&Name::new(key)).is_some_and(|value| {
            value
                .as_f64()
                .is_some_and(|value| value != 0.0 && value != 1.0)
        }) {
            unsupported_keys.push(format!("non_binary:{key}"));
        }
    }
    if dict
        .get(&Name::new("BM"))
        .is_some_and(|value| value.as_name().is_none())
    {
        unsupported_keys.push("invalid:BM".to_string());
    }
    for key in ["LW", "LC", "LJ", "ML"] {
        if dict.contains_key(&Name::new(key)) {
            unsupported_keys.push(format!("unapplied:{key}"));
        }
    }
    if dict.get(&Name::new("Type")).is_some_and(|value| {
        value
            .as_name()
            .and_then(Name::as_str)
            .is_none_or(|value| value != "ExtGState")
    }) {
        unsupported_keys.push("invalid:Type".to_string());
    }
    unsupported_keys.sort();
    unsupported_keys.dedup();
    let soft_mask = match dict.get(&Name::new("SMask")) {
        None => "absent".to_string(),
        Some(Object::Name(name)) if name.as_str() == Some("None") => "None".to_string(),
        Some(value) => object_kind(value).to_string(),
    };
    ExtGStateProfile {
        line_width: dict.get(&Name::new("LW")).and_then(Object::as_f64),
        line_cap: dict.get(&Name::new("LC")).and_then(Object::as_i64),
        line_join: dict.get(&Name::new("LJ")).and_then(Object::as_i64),
        miter_limit: dict.get(&Name::new("ML")).and_then(Object::as_f64),
        stroke_alpha: dict.get(&Name::new("CA")).and_then(Object::as_f64),
        fill_alpha: dict.get(&Name::new("ca")).and_then(Object::as_f64),
        blend_mode: dict
            .get(&Name::new("BM"))
            .and_then(Object::as_name)
            .and_then(Name::as_str)
            .map(str::to_string),
        soft_mask,
        unsupported_keys,
    }
}

fn selected_name(operator: &str, operands: &[Object]) -> Option<String> {
    matches!(operator, "Do" | "gs" | "sh" | "CS" | "cs" | "Tf")
        .then(|| {
            operands
                .iter()
                .rev()
                .find_map(Object::as_name)
                .and_then(Name::as_str)
                .map(str::to_string)
        })
        .flatten()
}

fn valid_form_bbox(dict: &Dict) -> bool {
    dict.get(&Name::new("BBox"))
        .and_then(Object::as_array)
        .is_some_and(|values| {
            values.len() == 4
                && values
                    .iter()
                    .all(|value| value.as_f64().is_some_and(f64::is_finite))
        })
}

fn valid_optional_matrix(dict: &Dict) -> bool {
    dict.get(&Name::new("Matrix")).is_none_or(|value| {
        value.as_array().is_some_and(|values| {
            values.len() == 6
                && values
                    .iter()
                    .all(|value| value.as_f64().is_some_and(f64::is_finite))
        })
    })
}

fn is_optional_content(operands: &[Object]) -> bool {
    operands
        .first()
        .and_then(Object::as_name)
        .and_then(Name::as_str)
        == Some("OC")
}

fn all_numbers(operands: &[Object], count: usize) -> bool {
    operands.len() == count
        && operands
            .iter()
            .all(|value| value.as_f64().is_some_and(f64::is_finite))
}

fn one_name(operands: &[Object]) -> bool {
    operands.len() == 1 && operands[0].as_name().is_some()
}

fn validate_operator(operator: &str, operands: &[Object]) -> Option<&'static str> {
    let valid = match operator {
        "q" | "Q" | "h" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" | "W"
        | "W*" | "BT" | "ET" | "T*" | "EMC" | "BX" | "EX" => operands.is_empty(),
        "cm" | "c" | "Tm" | "d1" => all_numbers(operands, 6),
        "m" | "l" | "Td" | "TD" | "d0" => all_numbers(operands, 2),
        "v" | "y" | "re" | "K" | "k" => all_numbers(operands, 4),
        "RG" | "rg" => all_numbers(operands, 3),
        "w" => all_numbers(operands, 1) && operands[0].as_f64().is_some_and(|v| v >= 0.0),
        "J" | "j" => {
            operands.len() == 1
                && operands[0]
                    .as_i64()
                    .is_some_and(|value| (0..=2).contains(&value))
        }
        "M" => all_numbers(operands, 1) && operands[0].as_f64().is_some_and(|v| v >= 1.0),
        "d" => {
            operands.len() == 2
                && operands[1]
                    .as_f64()
                    .is_some_and(|phase| phase.is_finite() && phase >= 0.0)
                && operands[0].as_array().is_some_and(|values| {
                    values.iter().all(|value| {
                        value
                            .as_f64()
                            .is_some_and(|number| number.is_finite() && number >= 0.0)
                    }) && (values.is_empty()
                        || values
                            .iter()
                            .any(|value| value.as_f64().is_some_and(|number| number > 0.0)))
                })
        }
        "ri" | "CS" | "cs" | "Do" | "sh" | "gs" | "MP" | "BMC" => one_name(operands),
        "i" => {
            all_numbers(operands, 1)
                && operands[0]
                    .as_f64()
                    .is_some_and(|value| (0.0..=100.0).contains(&value))
        }
        "Tr" => {
            operands.len() == 1
                && operands[0]
                    .as_i64()
                    .is_some_and(|value| (0..=7).contains(&value))
        }
        "G" | "g" | "Tc" | "Tw" | "Tz" | "TL" | "Ts" => all_numbers(operands, 1),
        "Tf" => {
            operands.len() == 2
                && operands[0].as_name().is_some()
                && operands[1].as_f64().is_some_and(f64::is_finite)
        }
        "Tj" | "'" => operands.len() == 1 && matches!(operands[0], Object::String(_)),
        "TJ" => {
            operands.len() == 1
                && operands[0].as_array().is_some_and(|values| {
                    values.iter().all(|value| {
                        matches!(value, Object::String(_))
                            || value.as_f64().is_some_and(f64::is_finite)
                    })
                })
        }
        "\"" => {
            operands.len() == 3
                && operands[0].as_f64().is_some_and(f64::is_finite)
                && operands[1].as_f64().is_some_and(f64::is_finite)
                && matches!(operands[2], Object::String(_))
        }
        "SC" | "sc" => {
            !operands.is_empty()
                && operands
                    .iter()
                    .all(|value| value.as_f64().is_some_and(f64::is_finite))
        }
        "SCN" | "scn" => {
            !operands.is_empty()
                && operands.iter().enumerate().all(|(index, value)| {
                    value.as_f64().is_some_and(f64::is_finite)
                        || index + 1 == operands.len() && value.as_name().is_some()
                })
        }
        "BDC" | "DP" => {
            operands.len() == 2
                && operands[0].as_name().is_some()
                && (operands[1].as_name().is_some() || operands[1].as_dict().is_some())
        }
        _ => true,
    };
    (!valid).then_some("invalid_operator_operands")
}

fn operator_disposition(operator: &str) -> PaintDisposition {
    if matches!(
        operator,
        "q" | "Q"
            | "cm"
            | "w"
            | "J"
            | "j"
            | "M"
            | "d"
            | "gs"
            | "m"
            | "l"
            | "c"
            | "v"
            | "y"
            | "h"
            | "re"
            | "S"
            | "s"
            | "f"
            | "F"
            | "f*"
            | "B"
            | "B*"
            | "b"
            | "b*"
            | "n"
            | "W"
            | "W*"
            | "BT"
            | "ET"
            | "Tc"
            | "Tw"
            | "Tz"
            | "TL"
            | "Tf"
            | "Tr"
            | "Ts"
            | "Td"
            | "TD"
            | "Tm"
            | "T*"
            | "Tj"
            | "TJ"
            | "'"
            | "\""
            | "CS"
            | "cs"
            | "SC"
            | "SCN"
            | "sc"
            | "scn"
            | "G"
            | "g"
            | "RG"
            | "rg"
            | "K"
            | "k"
            | "Do"
            | "sh"
            | "MP"
            | "DP"
            | "BMC"
            | "BDC"
            | "EMC"
            | "BX"
            | "EX"
    ) {
        PaintDisposition::Supported
    } else {
        PaintDisposition::Unsupported
    }
}
