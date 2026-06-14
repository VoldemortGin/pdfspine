//! Image insertion — `insert_image` (PRD §8.8, §7).
//!
//! Embeds an image XObject and places it with a `cm` matrix mapping the unit
//! square `[0,1]×[0,1]` to the target `rect` (in PyMuPDF top-left space,
//! converted to PDF user space). Two input paths for this milestone:
//!
//! - **JPEG passthrough** (`/DCTDecode`): the JPEG bytes are stored verbatim —
//!   **no re-encode** — which is the key P1 path. Dimensions and component count
//!   are read from the JPEG frame header (SOF marker) to fill `/Width`,
//!   `/Height` and pick the `/ColorSpace`.
//! - **Raw RGB pixels** (`/FlateDecode`): a `width×height×3` byte buffer stored
//!   Flate-compressed with `/ColorSpace /DeviceRGB`, `/BitsPerComponent 8`.
//!
//! Full PNG / alpha (`/SMask`) / palette handling leans on M5's `pdf-image`
//! decoder and is deferred (PRD §8.8 / §8.10).

use pdf_core::error::{Error, Result};
use pdf_core::filters::flate;
use pdf_core::geom::Rect;
use pdf_core::object::{Dict, Name, Object, StreamObj};
use pdf_core::DocumentStore;

use crate::content::{fmt_num, PageContent};

/// Inserts a **JPEG** image (passed through as `/DCTDecode`) into the page at
/// `page_index`, placed to fill `rect` (PyMuPDF top-left space). Returns the
/// chosen `/XObject` resource name.
///
/// # Errors
///
/// [`Error::Unsupported`] if `jpeg` is not a parseable JPEG (no re-encode is
/// attempted); never panics.
pub fn insert_image_jpeg(
    doc: &DocumentStore,
    page_index: usize,
    rect: Rect,
    jpeg: &[u8],
) -> Result<String> {
    let info = jpeg_info(jpeg).ok_or(Error::Unsupported("insert_image: not a parseable JPEG"))?;

    let mut dict = Dict::new();
    dict.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    dict.insert(Name::new("Subtype"), Object::Name(Name::new("Image")));
    dict.insert(Name::new("Width"), Object::Integer(i64::from(info.width)));
    dict.insert(Name::new("Height"), Object::Integer(i64::from(info.height)));
    dict.insert(
        Name::new("ColorSpace"),
        Object::Name(Name::new(info.color_space())),
    );
    dict.insert(Name::new("BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::new("Filter"), Object::Name(Name::new("DCTDecode")));
    dict.insert(Name::new("Length"), Object::Integer(jpeg.len() as i64));
    // CMYK JPEGs from Adobe are inverted; signal it with a `/Decode` array.
    if info.components == 4 {
        dict.insert(
            Name::new("Decode"),
            Object::Array(
                [1, 0, 1, 0, 1, 0, 1, 0]
                    .iter()
                    .map(|v| Object::Integer(*v))
                    .collect(),
            ),
        );
    }

    place_image(
        doc,
        page_index,
        rect,
        StreamObj::new_encoded(dict, jpeg.to_vec()),
    )
}

/// Inserts a **raw RGB** image (`width×height` 8-bit RGB triples) into the page,
/// Flate-compressed as a `/DeviceRGB` XObject, placed to fill `rect`. Returns
/// the chosen `/XObject` resource name.
///
/// # Errors
///
/// [`Error::Unsupported`] if `pixels.len() != width*height*3`.
pub fn insert_image_rgb(
    doc: &DocumentStore,
    page_index: usize,
    rect: Rect,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(3))
        .ok_or(Error::Unsupported(
            "insert_image: image dimensions overflow",
        ))?;
    if pixels.len() != expected {
        return Err(Error::Unsupported(
            "insert_image: RGB buffer length != width*height*3",
        ));
    }

    let compressed = flate::encode(pixels);
    let mut dict = Dict::new();
    dict.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    dict.insert(Name::new("Subtype"), Object::Name(Name::new("Image")));
    dict.insert(Name::new("Width"), Object::Integer(i64::from(width)));
    dict.insert(Name::new("Height"), Object::Integer(i64::from(height)));
    dict.insert(
        Name::new("ColorSpace"),
        Object::Name(Name::new("DeviceRGB")),
    );
    dict.insert(Name::new("BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
    dict.insert(
        Name::new("Length"),
        Object::Integer(compressed.len() as i64),
    );

    place_image(
        doc,
        page_index,
        rect,
        StreamObj::new_encoded(dict, compressed),
    )
}

/// Registers `image` under `/Resources /XObject` and appends a
/// `q cm /Img Do Q` chunk placing it at `rect` (top-left space → user space).
fn place_image(
    doc: &DocumentStore,
    page_index: usize,
    rect: Rect,
    image: StreamObj,
) -> Result<String> {
    let pc = PageContent::new(doc, page_index)?;
    let img_ref = doc.add_object(Object::Stream(image))?;
    let name = pc.add_resource("XObject", "Img", Object::Reference(img_ref))?;

    // Map the unit square to the user-space rect: scale by (w, h), translate to
    // the lower-left corner. (Images draw bottom-up from their lower-left, so no
    // y-flip is needed beyond the rect conversion.)
    let ur = pc.rect_to_user_space(rect);
    let w = ur.width();
    let h = ur.height();
    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"q\n");
    chunk.extend_from_slice(
        format!(
            "{} 0 0 {} {} {} cm\n",
            fmt_num(w),
            fmt_num(h),
            fmt_num(ur.x0),
            fmt_num(ur.y0)
        )
        .as_bytes(),
    );
    chunk.extend_from_slice(format!("/{name} Do\n").as_bytes());
    chunk.extend_from_slice(b"Q\n");
    pc.append_content(&chunk)?;
    Ok(name)
}

/// Minimal JPEG frame metadata read from the SOF marker.
struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
}

impl JpegInfo {
    /// The PDF `/ColorSpace` name implied by the component count.
    fn color_space(&self) -> &'static str {
        match self.components {
            1 => "DeviceGray",
            4 => "DeviceCMYK",
            _ => "DeviceRGB",
        }
    }
}

/// Parses a JPEG (JFIF/EXIF) header far enough to read the frame size + channel
/// count from the first Start-Of-Frame marker. Returns `None` for non-JPEG /
/// truncated input (never panics).
fn jpeg_info(data: &[u8]) -> Option<JpegInfo> {
    // SOI.
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 1 < data.len() {
        // Markers begin with 0xFF; skip fill bytes.
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        // Standalone markers (no length): RSTn, SOI, EOI, TEM.
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            continue;
        }
        if i + 1 >= data.len() {
            return None;
        }
        let seg_len = ((data[i] as usize) << 8) | (data[i + 1] as usize);
        if seg_len < 2 || i + seg_len > data.len() {
            return None;
        }
        // SOF0..SOF15 (baseline / progressive / etc.), excluding DHT(C4),
        // JPG(C8), DAC(CC).
        let is_sof =
            (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_sof {
            // Segment body: [precision(1)][height(2)][width(2)][components(1)]…
            if i + 6 >= data.len() {
                return None;
            }
            let height = ((data[i + 3] as u32) << 8) | (data[i + 4] as u32);
            let width = ((data[i + 5] as u32) << 8) | (data[i + 6] as u32);
            let components = data[i + 7];
            if width == 0 || height == 0 {
                return None;
            }
            return Some(JpegInfo {
                width,
                height,
                components,
            });
        }
        i += seg_len;
    }
    None
}
