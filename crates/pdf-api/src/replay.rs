//! Read-only callback replay access to a frozen display-list recording.

use crate::{geom::Matrix, DisplayList, Error, Result};
pub use pdf_core::{Name, Object};
use pdf_text::ImageResolver;
pub use pdf_text::{PathItem, RenderOp};

impl DisplayList {
    /// Builds a semantic append segment from exact selected recording identities.
    /// Selection is operation-level; target clipping is a separate layout step.
    #[must_use]
    pub fn replay_textpage(
        &self,
        selected: &[usize],
        flags: u32,
        matrix: Matrix,
        target: crate::geom::Rect,
    ) -> Option<pdf_text::TextPage> {
        let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
        let glyphs: Vec<_> = self
            .replay_text
            .iter()
            .filter(|(op, _)| selected.contains(op))
            .flat_map(|(_, range)| self.text.glyphs[range.clone()].iter().cloned())
            .collect();
        let images: Vec<_> = self
            .text
            .images
            .iter()
            .zip(&self.replay_images)
            .filter(|(_, op)| op.is_some_and(|op| selected.contains(&op)))
            .map(|(image, _)| image.clone())
            .collect();
        if glyphs.is_empty() && images.is_empty() {
            return None;
        }
        Some(pdf_text::textpage_from_glyphs_transformed(
            &glyphs,
            &images,
            self.cropbox,
            self.rotation,
            matrix,
            target,
            flags,
        ))
    }

    /// Recorded geometry has its content CTM applied; this adds page and caller transforms once.
    #[must_use]
    pub fn replay_matrix(&self, matrix: Matrix) -> Matrix {
        pdf_text::page_transform(self.cropbox, self.rotation) * matrix
    }

    /// Borrows immutable operations; no live document handle is exposed.
    #[must_use]
    pub fn replay_ops(&self) -> &[RenderOp] {
        self.inner.operations()
    }

    /// Resolves the placement's owned image using its captured resource context.
    /// The format is the actual encoded payload format (currently PNG or JPEG).
    ///
    /// # Errors
    /// An existing image whose payload cannot be resolved produces an explicit error.
    pub fn replay_image(&self, index: usize) -> Result<Option<(String, Vec<u8>)>> {
        if !matches!(self.replay_ops().get(index), Some(RenderOp::Image(_))) {
            return Ok(None);
        }
        let image = self
            .replay_images
            .iter()
            .position(|op| *op == Some(index))
            .and_then(|image| {
                self.text_resources
                    .resolve(Some(&format!("snapshot-image-{image}")))
            })
            .ok_or_else(|| Error::Decode("recorded image payload could not be decoded".into()))?;
        Ok(Some((image.ext, image.image)))
    }

    /// Returns PDF-filter-decoded original embedded font bytes and the PDF program format.
    /// No-file fonts return None; built-in substitutes are never presented as embedded data.
    ///
    /// # Errors
    /// Malformed references, descriptors, streams or PDF filters propagate typed errors.
    pub fn replay_font(&self, index: usize) -> Result<Option<(String, Vec<u8>)>> {
        let Some(RenderOp::Text(run)) = self.replay_ops().get(index) else {
            return Ok(None);
        };
        let doc = &self.doc;
        let mut font = run.font_dict.clone();
        if let Some(descendants) = doc.resolve_dict_key(&font, &Name::new("DescendantFonts"))? {
            let first = descendants
                .as_array()
                .and_then(|a| a.first())
                .ok_or_else(|| Error::Syntax("invalid DescendantFonts".into()))?;
            let resolved = match first {
                Object::Reference(r) => doc.resolve(*r)?,
                o => std::sync::Arc::new(o.clone()),
            };
            font = resolved
                .as_dict()
                .ok_or_else(|| Error::Syntax("invalid descendant font".into()))?
                .clone();
        }
        let Some(descriptor) = doc.resolve_dict_key(&font, &Name::new("FontDescriptor"))? else {
            return Ok(None);
        };
        let descriptor = descriptor
            .as_dict()
            .ok_or_else(|| Error::Syntax("invalid FontDescriptor".into()))?;
        for (key, default_format) in [
            ("FontFile", "Type1"),
            ("FontFile2", "TrueType"),
            ("FontFile3", "FontFile3"),
        ] {
            if let Some(object) = doc.resolve_dict_key(descriptor, &Name::new(key))? {
                let stream = object
                    .as_stream()
                    .ok_or_else(|| Error::Syntax("invalid embedded font stream".into()))?;
                let format = stream
                    .dict
                    .get(&Name::new("Subtype"))
                    .and_then(Object::as_name)
                    .and_then(Name::as_str)
                    .unwrap_or(default_format)
                    .to_string();
                return Ok(Some((format, doc.decode_stream(stream)?.into_decoded()?)));
            }
        }
        Ok(None)
    }
}
