//! Strict page-paint accounting facade.

use pdf_core::page::{EffectiveResourceOrigin, ResourceResolutionError};
use pdf_core::{Dict, Name, Object, Page};

pub use pdf_text::{
    ExtGStateProfile, InlineImageProfile, PaintDiagnostic, PaintDisposition, PaintOperator,
    PaintProfile, PaintResourceEntry, PaintResourceScope, ResourceOrigin, TransparencyGroupProfile,
    PAINT_PROFILE_VERSION,
};

/// Builds the strict paint profile from the same effective page resources used
/// by the text and display-list entry points.
#[must_use]
pub fn page_get_paint_profile(page: &Page) -> PaintProfile {
    let mut page_dict = page.dict().unwrap_or_default();
    let checked = page.effective_resources_checked();
    let (resources, inherited, resource_error) = match checked {
        Ok(checked) => (
            checked.resources,
            checked.origin == EffectiveResourceOrigin::Inherited,
            None,
        ),
        Err(error) => (Dict::new(), false, Some(error)),
    };
    page_dict.insert(
        Name::new("Resources"),
        Object::Dictionary(resources.clone()),
    );
    let mut profile = pdf_text::build_paint_profile(
        page.document(),
        page.obj_ref(),
        &page_dict,
        resources,
        inherited,
    );
    if let Some(error) = resource_error {
        let code = match error {
            ResourceResolutionError::ParentCycle => "page_resource_parent_cycle",
            ResourceResolutionError::DepthExceeded => "page_resource_depth_exceeded",
            ResourceResolutionError::UnresolvedReference => "page_resource_reference_unresolved",
            ResourceResolutionError::WrongType => "page_resources_wrong_type",
            ResourceResolutionError::InvalidParent => "page_resource_parent_invalid",
        };
        profile.diagnostics.insert(
            0,
            pdf_text::PaintDiagnostic {
                code,
                scope_id: format!("page:{}", page.obj_ref().num),
                operator_ordinal: None,
            },
        );
        profile.complete = false;
    }
    profile
}

/// Returns a page dictionary with effective resources materialized. Kept
/// crate-visible so recording entry points cannot drift from paint profiling.
pub(crate) fn page_dict_with_effective_resources(page: &Page) -> Option<Dict> {
    let mut page_dict = page.dict()?;
    if let Some(resources) = page.effective_resources() {
        page_dict.insert(Name::new("Resources"), Object::Dictionary(resources));
    }
    Some(page_dict)
}
