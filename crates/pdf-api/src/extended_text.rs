//! Owned resources and per-block creation flags for atomic TextPage append.

use std::collections::HashMap;
use std::sync::Arc;

use crate::text::TextOutput;
use crate::{Page, RecordedTextResources, Result};
use pdf_text::{textflags, BlockKind, DictBlockRef, ImageResolver, ResolvedImage, TextPage};

#[derive(Clone)]
enum ImageSource {
    Recorded(Arc<RecordedTextResources>, String),
    Materialized(ResolvedImage, bool),
}

/// Append context: resource names are unique across records/documents, and each
/// block retains its own segment's flags. The model contains only visible images.
#[derive(Clone, Default)]
pub struct ExtendedTextResources {
    images: HashMap<String, ImageSource>,
    flags: Vec<u32>,
    transforms: HashMap<String, pdf_core::geom::Matrix>,
}

impl ImageResolver for ExtendedTextResources {
    fn placement_transform(&self, name: Option<&str>) -> Option<pdf_core::geom::Matrix> {
        self.transforms.get(name?).copied()
    }

    fn resolve(&self, name: Option<&str>) -> Option<ResolvedImage> {
        match self.images.get(name?)? {
            ImageSource::Recorded(resources, original) => resources.resolve(Some(original)),
            ImageSource::Materialized(image, _) => Some(image.clone()),
        }
    }
}

impl ExtendedTextResources {
    /// Promotes an ordinary Page-backed target without rebuilding its layout.
    /// Its existing visible image semantics are preserved, including legacy
    /// Page creation flags that were not retained by that older API.
    ///
    /// # Errors
    /// An unreadable visible image prevents promotion rather than losing bytes.
    pub fn from_page(page: &Page, tp: &mut TextPage) -> Result<Self> {
        let mut this = Self::default();
        let images = crate::text::snapshot_textpage_images(page, tp)?;
        let masks: HashMap<_, _> = crate::text::get_images(page)
            .into_iter()
            .map(|image| (image.name, image.smask != 0))
            .collect();
        for (old, image) in images {
            let masked = masks.get(&old).copied().unwrap_or(false);
            this.images
                .insert(old, ImageSource::Materialized(image, masked));
        }
        this.flags.resize(tp.blocks.len(), pdf_text::defaults::DICT);
        Ok(this)
    }

    /// Promotes an already owned DisplayList target with its creation flags.
    #[must_use]
    pub fn from_recorded(
        tp: &mut TextPage,
        resources: Arc<RecordedTextResources>,
        flags: u32,
    ) -> Self {
        let mut this = Self::default();
        this.import(tp, resources, flags, &HashMap::new());
        this
    }

    fn import(
        &mut self,
        tp: &mut TextPage,
        resources: Arc<RecordedTextResources>,
        flags: u32,
        transforms: &HashMap<String, pdf_core::geom::Matrix>,
    ) {
        tp.blocks
            .retain(|b| b.kind != BlockKind::Image || flags & textflags::PRESERVE_IMAGES != 0);
        for block in &mut tp.blocks {
            if let Some(image) = &mut block.image {
                if let Some(original) = &image.name {
                    let mut index = self.images.len();
                    let key = loop {
                        let candidate = format!("append-image-{index}");
                        if !self.images.contains_key(&candidate) {
                            break candidate;
                        }
                        index += 1;
                    };
                    self.images.insert(
                        key.clone(),
                        ImageSource::Recorded(resources.clone(), original.to_string()),
                    );
                    if let Some(transform) = transforms.get(original.as_str()) {
                        self.transforms.insert(key.clone(), *transform);
                    }
                    image.name = Some(key.into());
                }
            }
            self.flags.push(flags);
        }
    }

    /// Appends a newly built segment, preserving all existing block geometry.
    pub fn append(
        &mut self,
        target: &mut TextPage,
        mut new: TextPage,
        resources: Arc<RecordedTextResources>,
        flags: u32,
        transforms: &HashMap<String, pdf_core::geom::Matrix>,
    ) {
        self.import(&mut new, resources, flags, transforms);
        let start = target.blocks.len();
        for (offset, block) in new.blocks.iter_mut().enumerate() {
            block.number = start + offset;
        }
        target.blocks.extend(new.blocks);
    }

    /// Structured blocks share one model/resolver, without concatenating JSON.
    #[must_use]
    pub fn dict_blocks<'a>(&self, tp: &'a TextPage) -> Vec<DictBlockRef<'a>> {
        pdf_text::dict_blocks(tp, textflags::PRESERVE_IMAGES, Some(self))
    }

    /// Serializes a combined page. Text/block dehyphenation stays segment-local;
    /// structured and markup outputs retain a single document wrapper.
    #[must_use]
    pub fn output(&self, tp: &TextPage, opt: &str) -> TextOutput {
        if opt == "text" || opt == "blocks" {
            let mut text = String::new();
            let mut blocks = Vec::new();
            for (block, flags) in tp.blocks.iter().zip(&self.flags) {
                let one = TextPage {
                    width: tp.width,
                    height: tp.height,
                    blocks: vec![block.clone()],
                };
                if opt == "text" {
                    text.push_str(&pdf_text::to_text(&one, *flags));
                } else {
                    blocks.extend(pdf_text::to_blocks(&one, *flags));
                }
            }
            return if opt == "text" {
                TextOutput::Text(text)
            } else {
                TextOutput::Blocks(blocks)
            };
        }
        crate::text::serialize_textpage(tp, opt, Some(textflags::PRESERVE_IMAGES), self)
    }
    /// Image metadata, respecting the TextPage's creation flags.
    #[must_use]
    pub fn image_info(&self, tp: &TextPage) -> Vec<crate::text::ImgInfoEntry> {
        self.dict_blocks(tp)
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
                Some(crate::text::ImgInfoEntry {
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
                    has_mask: name.and_then(|n| self.images.get(n)).is_some_and(|source| {
                        match source {
                            ImageSource::Recorded(resources, original) => {
                                resources.has_mask(original)
                            }
                            ImageSource::Materialized(_, masked) => *masked,
                        }
                    }),
                })
            })
            .collect()
    }
}
