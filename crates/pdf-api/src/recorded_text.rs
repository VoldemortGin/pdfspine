//! Owned text resources for display-list snapshots, independent of a live Page.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use pdf_core::{DocumentStore, Name, ObjRef, Object, StreamObj};
use pdf_image::codecs::pixmap_from_stream;
use pdf_text::{DictBlockRef, ImageResolver, RenderOp, ResolvedImage, TextPage};

use crate::text::{ImgInfoEntry, TextOutput};

struct RecordedImage {
    store: Arc<DocumentStore>,
    object: Object,
    decoded: OnceLock<Option<ResolvedImage>>,
}

impl RecordedImage {
    fn resolve(&self) -> Option<ResolvedImage> {
        self.decoded
            .get_or_init(|| match &self.object {
                Object::Reference(xref) => {
                    crate::text::extract_resolved_image(&self.store, xref.num)
                }
                Object::Stream(image) => {
                    let raw = self.store.stream_raw_bytes(image).ok()?;
                    let pix = pixmap_from_stream(&self.store, &image.dict, &raw).ok()?;
                    Some(ResolvedImage {
                        ext: "png".to_string(),
                        colorspace: pix.colorspace.components() as i32,
                        bpc: 8,
                        width: pix.width as i32,
                        height: pix.height as i32,
                        xres: 96,
                        yres: 96,
                        image: pix.to_png_bytes().ok()?,
                    })
                }
                _ => None,
            })
            .clone()
    }
}

/// Image data captured while source resources are valid. The keys identify
/// recorded placements, never a form-local resource name such as `/Im1`.
#[derive(Default)]
pub struct RecordedTextResources {
    images: HashMap<String, Arc<RecordedImage>>,
    masks: HashMap<String, bool>,
}

impl ImageResolver for RecordedTextResources {
    fn resolve(&self, name: Option<&str>) -> Option<ResolvedImage> {
        self.images.get(name?)?.resolve()
    }
}

impl RecordedTextResources {
    pub(crate) fn capture(
        doc: &DocumentStore,
        ops: &[RenderOp],
        image_ops: &[Option<usize>],
        color_spaces: &[Option<Object>],
    ) -> Arc<Self> {
        let mut resources = Self::default();
        let mut roots = Vec::new();
        let mut placements = Vec::new();
        for (index, operation) in image_ops.iter().enumerate() {
            let Some(RenderOp::Image(image)) = operation.and_then(|i| ops.get(i)) else {
                continue;
            };
            let override_cs = color_spaces.get(index).and_then(Option::as_ref);
            let root = if image.obj_num.is_some() && override_cs.is_none() {
                Object::Reference(ObjRef::new(image.obj_num.unwrap_or_default(), 0))
            } else {
                // Normalize inline filter keys before applying its contextual
                // colorspace. Neither operation mutates the raster record.
                let mut dict = image.dict.clone();
                if image.obj_num.is_none() {
                    for (short, full) in
                        [("CS", "ColorSpace"), ("F", "Filter"), ("DP", "DecodeParms")]
                    {
                        if let Some(value) = dict.get(&Name::new(short)).cloned() {
                            dict.entry(Name::new(full)).or_insert(value);
                        }
                    }
                }
                if let Some(cs) = override_cs {
                    dict.insert(Name::new("ColorSpace"), cs.clone());
                }
                Object::Stream(StreamObj::new_encoded(dict, image.raw.clone()))
            };
            let key = format!("snapshot-image-{index}");
            resources
                .masks
                .insert(key.clone(), image.dict.contains_key(&Name::new("SMask")));
            placements.push((key, image.obj_num.is_some()));
            roots.push(root);
        }
        if roots.is_empty() {
            return Arc::new(resources);
        }
        if let Ok((store, copied)) = pdf_edit::merge::snapshot_resources(doc, &roots) {
            let store = Arc::new(store);
            let mut shared = HashMap::new();
            for ((key, xobject), object) in placements.into_iter().zip(copied) {
                let Some(mut object) = object else {
                    continue;
                };
                if xobject && object.as_stream().is_some() {
                    let Ok(xref) = store.add_object(object) else {
                        continue;
                    };
                    object = Object::Reference(xref);
                }
                let cached = object
                    .as_reference()
                    .and_then(|xref| shared.get(&xref.num))
                    .cloned();
                let image = cached.unwrap_or_else(|| {
                    Arc::new(RecordedImage {
                        store: Arc::clone(&store),
                        object: object.clone(),
                        decoded: OnceLock::new(),
                    })
                });
                if let Some(xref) = object.as_reference() {
                    shared.insert(xref.num, Arc::clone(&image));
                }
                resources.images.insert(key, image);
            }
        }
        Arc::new(resources)
    }

    pub(crate) fn has_mask(&self, name: &str) -> bool {
        self.masks.get(name).copied().unwrap_or(false)
    }

    /// Serializes the snapshot using the flags captured by its TextPage.
    #[must_use]
    pub fn output(&self, tp: &TextPage, opt: &str, flags: u32) -> TextOutput {
        crate::text::serialize_textpage(tp, opt, Some(flags), self)
    }

    /// Borrows text blocks and resolves owned image bytes for native dict output.
    #[must_use]
    pub fn dict_blocks<'a>(&self, tp: &'a TextPage, flags: u32) -> Vec<DictBlockRef<'a>> {
        pdf_text::dict_blocks(tp, flags, Some(self))
    }

    /// Image metadata, respecting the TextPage's creation flags.
    #[must_use]
    pub fn image_info(&self, tp: &TextPage, flags: u32) -> Vec<ImgInfoEntry> {
        self.dict_blocks(tp, flags)
            .into_iter()
            .filter_map(|block| {
                let DictBlockRef::Image(image) = block else {
                    return None;
                };
                let name = tp
                    .blocks
                    .iter()
                    .find(|b| b.number == image.number as usize)
                    .and_then(|b| b.image.as_ref())
                    .and_then(|i| i.name.as_deref());
                Some(ImgInfoEntry {
                    number: image.number as usize,
                    bbox: image.bbox,
                    transform: image.transform,
                    width: image.width as u32,
                    height: image.height as u32,
                    colorspace: image.colorspace,
                    cs_name: match image.colorspace {
                        1 => "DeviceGray",
                        3 => "DeviceRGB",
                        4 => "DeviceCMYK",
                        _ => "",
                    }
                    .to_string(),
                    xres: image.xres,
                    yres: image.yres,
                    bpc: image.bpc,
                    size: image.image.len(),
                    has_mask: name
                        .and_then(|n| self.masks.get(n))
                        .copied()
                        .unwrap_or(false),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::geom::Matrix;
    use pdf_core::{Dict, Limits};
    use pdf_text::{ImageOp, ImageRef};

    #[test]
    fn capture_is_lazy_and_creation_flags_control_decoding() {
        let doc = DocumentStore::from_bytes(
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R /Size 2 >>\n%%EOF\n".to_vec(),
            Limits::default(),
        ).unwrap();
        let mut dict = Dict::new();
        dict.insert(Name::new("Width"), Object::Integer(1));
        dict.insert(Name::new("Height"), Object::Integer(1));
        dict.insert(
            Name::new("ColorSpace"),
            Object::Name(Name::new("DeviceRGB")),
        );
        dict.insert(Name::new("BitsPerComponent"), Object::Integer(8));
        let image = RenderOp::Image(ImageOp {
            dict,
            raw: vec![255, 0, 0],
            obj_num: None,
            ctm: Matrix::IDENTITY,
            fill_color: 0,
            alpha: 255,
        });
        let resources = RecordedTextResources::capture(&doc, &[image], &[Some(0)], &[]);
        let recorded = &resources.images["snapshot-image-0"];
        assert!(recorded.decoded.get().is_none());
        let image = ImageRef {
            name: Some("snapshot-image-0".into()),
            inline: true,
            ctm: Matrix::IDENTITY,
            width: Some(1),
            height: Some(1),
        };
        let rect = pdf_core::geom::Rect::new(0.0, 0.0, 20.0, 20.0);
        let tp = pdf_text::textpage_from_glyphs_flagged(&[], &[image], rect, 0, Some(rect), 3);
        assert!(resources.dict_blocks(&tp, 3).is_empty());
        assert!(recorded.decoded.get().is_none());
        assert_eq!(resources.dict_blocks(&tp, 7).len(), 1);
        assert!(recorded.decoded.get().unwrap().is_some());
    }
}
