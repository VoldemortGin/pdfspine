//! M5-imagedoc integration tests (PRD §8.10, IMGDOC-*).
//!
//! Test images are synthesized in-test with the `image` crate, so no external
//! assets are needed. The produced PDFs are reparsed via `pdf-core` to assert
//! their structure (MediaBox, image XObjects, `/DCTDecode` passthrough,
//! `/SMask`, `/Indexed`, page count).

use std::io::Cursor;

use image::{
    DynamicImage, GrayImage, ImageBuffer, ImageFormat as ImgFmt, Luma, LumaA, Rgb, Rgba, RgbaImage,
};

use pdf_core::{Dict, DocumentStore, Limits, Name, Object};

use pdf_image::imagedoc::{convert_to_pdf, open_image_document, ImageFormat};

// ---------------------------------------------------------------------------
// Helpers: synthesize encoded image bytes
// ---------------------------------------------------------------------------

fn encode(img: &DynamicImage, fmt: ImgFmt) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
    out
}

/// A `w`×`h` RGB PNG with a deterministic gradient.
fn png_rgb(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    encode(&DynamicImage::ImageRgb8(buf), ImgFmt::Png)
}

/// A `w`×`h` RGBA PNG with a varying alpha channel.
fn png_rgba(w: u32, h: u32) -> Vec<u8> {
    let buf: RgbaImage = ImageBuffer::from_fn(w, h, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, 64, ((x * 3) % 256) as u8])
    });
    encode(&DynamicImage::ImageRgba8(buf), ImgFmt::Png)
}

/// A `w`×`h` 8-bit grayscale PNG.
fn png_gray(w: u32, h: u32) -> Vec<u8> {
    let buf: GrayImage = ImageBuffer::from_fn(w, h, |x, _| Luma([(x % 256) as u8]));
    encode(&DynamicImage::ImageLuma8(buf), ImgFmt::Png)
}

/// A genuine palette PNG (IHDR color-type 3) hand-assembled so the loader's
/// `/Indexed` path (gated on a real paletted source) is exercised.
fn png_indexed_native(w: u32, h: u32) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn crc32(data: &[u8]) -> u32 {
        // Standard PNG CRC (polynomial 0xEDB88320), no table cache needed here.
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        crc ^ 0xFFFF_FFFF
    }
    fn chunk(out: &mut Vec<u8>, ctype: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc_input = Vec::with_capacity(4 + data.len());
        crc_input.extend_from_slice(ctype);
        crc_input.extend_from_slice(data);
        out.extend_from_slice(ctype);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }

    let palette: [[u8; 3]; 4] = [
        [255, 0, 0],     // 0 red
        [0, 255, 0],     // 1 green
        [0, 0, 255],     // 2 blue
        [255, 255, 255], // 3 white
    ];

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR: width, height, bitdepth=8, colortype=3 (indexed), 0,0,0.
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);

    // PLTE.
    let mut plte = Vec::new();
    for c in &palette {
        plte.extend_from_slice(c);
    }
    chunk(&mut out, b"PLTE", &plte);

    // IDAT: each scanline prefixed with filter byte 0 (None).
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            raw.push((((x + y) as usize) % palette.len()) as u8);
        }
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).unwrap();
    let idat = enc.finish().unwrap();
    chunk(&mut out, b"IDAT", &idat);

    chunk(&mut out, b"IEND", &[]);
    out
}

/// A `w`×`h` baseline JPEG.
fn jpeg_rgb(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 128u8]));
    encode(&DynamicImage::ImageRgb8(buf), ImgFmt::Jpeg)
}

/// A `w`×`h` BMP.
fn bmp_rgb(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, 100u8, (y % 256) as u8]));
    encode(&DynamicImage::ImageRgb8(buf), ImgFmt::Bmp)
}

/// A `frames`-frame animated GIF, each frame a solid color.
fn gif_animated(w: u32, h: u32, frames: u32) -> Vec<u8> {
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        for f in 0..frames {
            let shade = ((f * 60) % 256) as u8;
            let buf: RgbaImage = ImageBuffer::from_pixel(w, h, Rgba([shade, 0, 255 - shade, 255]));
            enc.encode_frame(Frame::from_parts(
                buf,
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            ))
            .unwrap();
        }
    }
    out
}

/// A single-IFD TIFF (RGB).
fn tiff_rgb(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
    });
    encode(&DynamicImage::ImageRgb8(buf), ImgFmt::Tiff)
}

/// A multi-IFD TIFF with `n` IFDs, by concatenating single-IFD TIFFs? No — TIFF
/// can't be concatenated. Build one via the `tiff` path of the `image` crate by
/// writing pages with the encoder's multi-image support is unavailable, so we
/// hand-assemble a 2-IFD little-endian TIFF from two single-IFD ones.
fn tiff_multi(pages: &[(u32, u32)]) -> Vec<u8> {
    // Build each page as a standalone TIFF, then merge their IFD chains into one
    // file by appending all bytes and relinking the next-IFD pointers. Because
    // each standalone TIFF uses absolute offsets, we shift the second file's
    // internal offsets by its placement base. This mirrors what real multi-page
    // TIFF writers do and exercises the loader's IFD splitter.
    assert!(!pages.is_empty());
    let singles: Vec<Vec<u8>> = pages.iter().map(|&(w, h)| tiff_rgb(w, h)).collect();
    merge_tiffs_le(&singles)
}

/// Merges several little-endian single-IFD TIFFs into one multi-IFD TIFF.
/// (Test-only; assumes each input is a fresh `II*\0` TIFF with one IFD.)
fn merge_tiffs_le(singles: &[Vec<u8>]) -> Vec<u8> {
    // Output: shared 8-byte header (II 42 -> first IFD), then each input's body
    // (everything after its 8-byte header) appended at a known base, with the
    // header's internal IFD offset and the value/offset fields shifted by base.
    // We relink each IFD's next-IFD pointer to the following IFD.
    fn rd_u16(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }
    fn rd_u32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }
    fn elem_size(t: u16) -> usize {
        match t {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => 0,
        }
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0x49, 0x49]); // II
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // first-IFD offset, patched later

    // Record where each input's IFD begins in `out`, to patch next-IFD links.
    let mut ifd_positions: Vec<usize> = Vec::new();

    for single in singles {
        assert_eq!(&single[0..2], b"II");
        let base = out.len();
        let ifd_off_in_single = rd_u32(single, 4) as usize;
        // Append the input's bytes from offset 8 onward, shifting by (base - 8).
        let shift = base as i64 - 8;
        // Copy body.
        let mut body = single[8..].to_vec();
        // The IFD inside `body` is at (ifd_off_in_single - 8). Patch entry
        // value/offset fields and the next-IFD link, plus strip offsets.
        let ifd_local = ifd_off_in_single - 8;
        let count = rd_u16(&body, ifd_local) as usize;
        // Patch each entry whose data is external (byte_len > 4).
        for e in 0..count {
            let eoff = ifd_local + 2 + e * 12;
            let ftype = rd_u16(&body, eoff + 2);
            let ecount = rd_u32(&body, eoff + 4) as usize;
            let tag = rd_u16(&body, eoff);
            let blen = elem_size(ftype) * ecount;
            if blen > 4 {
                let old = rd_u32(&body, eoff + 8) as i64;
                let new = (old + shift) as u32;
                body[eoff + 8..eoff + 12].copy_from_slice(&new.to_le_bytes());
            }
            // StripOffsets(273): elements are absolute offsets -> shift each.
            if tag == 273 {
                if blen <= 4 {
                    // inline single offset (LONG) or shorts
                    if ftype == 4 {
                        let old = rd_u32(&body, eoff + 8) as i64;
                        let new = (old + shift) as u32;
                        body[eoff + 8..eoff + 12].copy_from_slice(&new.to_le_bytes());
                    } else if ftype == 3 {
                        for k in 0..ecount {
                            let p = eoff + 8 + k * 2;
                            let old = rd_u16(&body, p) as i64;
                            let new = (old + shift) as u16;
                            body[p..p + 2].copy_from_slice(&new.to_le_bytes());
                        }
                    }
                } else {
                    // external array (already shifted to new location)
                    let arr_off = (rd_u32(&body, eoff + 8) as i64) as usize; // already shifted? no
                                                                             // We shifted the pointer above; recompute its local position.
                    let local = arr_off - base; // arr_off is absolute-in-out
                    for k in 0..ecount {
                        let p = local + k * elem_size(ftype);
                        if ftype == 4 {
                            let old = rd_u32(&body, p) as i64;
                            let new = (old + shift) as u32;
                            body[p..p + 4].copy_from_slice(&new.to_le_bytes());
                        } else if ftype == 3 {
                            let old = rd_u16(&body, p) as i64;
                            let new = (old + shift) as u16;
                            body[p..p + 2].copy_from_slice(&new.to_le_bytes());
                        }
                    }
                }
            }
        }
        // The next-IFD link (4 bytes) right after the entries.
        let link_local = ifd_local + 2 + count * 12;
        // Leave as 0 for now; patched after we know all positions.
        body[link_local..link_local + 4].copy_from_slice(&0u32.to_le_bytes());

        out.extend_from_slice(&body);
        ifd_positions.push(base + ifd_local);
    }

    // Patch the first-IFD offset in the header.
    let first = ifd_positions[0] as u32;
    out[4..8].copy_from_slice(&first.to_le_bytes());

    // Relink next-IFD pointers to chain the IFDs.
    for i in 0..ifd_positions.len() {
        let ifd_pos = ifd_positions[i];
        let count = rd_u16(&out, ifd_pos) as usize;
        let link = ifd_pos + 2 + count * 12;
        let next = if i + 1 < ifd_positions.len() {
            ifd_positions[i + 1] as u32
        } else {
            0
        };
        out[link..link + 4].copy_from_slice(&next.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// PDF navigation helpers
// ---------------------------------------------------------------------------

fn open_pdf(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default())
        .expect("convert_to_pdf output must reparse cleanly")
}

fn root_dict(doc: &DocumentStore) -> Dict {
    let root = doc
        .resolve_dict_key(doc.trailer(), &Name::new("Root"))
        .unwrap()
        .expect("catalog");
    as_dict(&root)
}

fn as_dict(obj: &Object) -> Dict {
    match obj {
        Object::Dictionary(d) => d.clone(),
        Object::Stream(s) => s.dict.clone(),
        _ => panic!("expected dict/stream, got {obj:?}"),
    }
}

/// Returns the page dicts in order.
fn pages(doc: &DocumentStore) -> Vec<Dict> {
    let catalog = root_dict(doc);
    let pages_obj = doc
        .resolve_dict_key(&catalog, &Name::new("Pages"))
        .unwrap()
        .expect("pages");
    let pages_dict = as_dict(&pages_obj);
    let kids = match doc
        .resolve_dict_key(&pages_dict, &Name::new("Kids"))
        .unwrap()
        .expect("kids")
        .as_ref()
    {
        Object::Array(a) => a.clone(),
        other => panic!("kids not array: {other:?}"),
    };
    kids.iter()
        .map(|k| {
            let p = match k {
                Object::Reference(r) => doc.resolve(*r).unwrap(),
                direct => std::sync::Arc::new(direct.clone()),
            };
            as_dict(&p)
        })
        .collect()
}

/// Resolves a page's first image XObject (name, stream dict, stream object).
fn page_image(doc: &DocumentStore, page: &Dict) -> (Name, pdf_core::StreamObj) {
    let resources = doc
        .resolve_dict_key(page, &Name::new("Resources"))
        .unwrap()
        .expect("resources");
    let resources = as_dict(&resources);
    let xobjects = doc
        .resolve_dict_key(&resources, &Name::new("XObject"))
        .unwrap()
        .expect("xobject dict");
    let xobjects = as_dict(&xobjects);
    let (name, val) = xobjects.iter().next().expect("at least one xobject");
    let obj = match val {
        Object::Reference(r) => doc.resolve(*r).unwrap(),
        direct => std::sync::Arc::new(direct.clone()),
    };
    match obj.as_ref() {
        Object::Stream(s) => (name.clone(), s.clone()),
        other => panic!("xobject not a stream: {other:?}"),
    }
}

fn dict_int(d: &Dict, key: &str) -> i64 {
    match d.get(&Name::new(key)) {
        Some(Object::Integer(i)) => *i,
        other => panic!("key {key} not int: {other:?}"),
    }
}

fn dict_name(d: &Dict, key: &str) -> String {
    match d.get(&Name::new(key)) {
        Some(Object::Name(n)) => n.as_str().unwrap_or_default().to_string(),
        other => panic!("key {key} not name: {other:?}"),
    }
}

fn mediabox(d: &Dict) -> [f64; 4] {
    match d.get(&Name::new("MediaBox")) {
        Some(Object::Array(a)) => {
            let mut out = [0.0; 4];
            for (i, v) in a.iter().enumerate().take(4) {
                out[i] = match v {
                    Object::Integer(n) => *n as f64,
                    Object::Real(r) => *r,
                    other => panic!("mediabox elem: {other:?}"),
                };
            }
            out
        }
        other => panic!("MediaBox not array: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// IMGDOC-SNIFF-* : format detection
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_sniff_all_formats() {
    assert_eq!(ImageFormat::sniff(&png_rgb(2, 2)), Some(ImageFormat::Png));
    assert_eq!(ImageFormat::sniff(&jpeg_rgb(2, 2)), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::sniff(&tiff_rgb(2, 2)), Some(ImageFormat::Tiff));
    assert_eq!(
        ImageFormat::sniff(&gif_animated(2, 2, 1)),
        Some(ImageFormat::Gif)
    );
    assert_eq!(ImageFormat::sniff(&bmp_rgb(2, 2)), Some(ImageFormat::Bmp));
}

#[test]
fn imgdoc_sniff_rejects_non_image() {
    assert_eq!(ImageFormat::sniff(b"not an image at all"), None);
    assert_eq!(ImageFormat::sniff(b""), None);
    assert_eq!(ImageFormat::sniff(b"%PDF-1.7"), None);
}

// ---------------------------------------------------------------------------
// IMGDOC-PNG-* : PNG -> 1-page doc, correct MediaBox, image XObject
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_png_single_page_mediabox() {
    let bytes = png_rgb(100, 50);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    assert_eq!(pages.len(), 1, "PNG -> single page");
    // No DPI metadata -> 1px = 1pt.
    let mb = mediabox(&pages[0]);
    assert_eq!(mb, [0.0, 0.0, 100.0, 50.0]);
}

#[test]
fn imgdoc_png_image_xobject_dimensions() {
    let bytes = png_rgb(40, 24);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    let (_name, img) = page_image(&doc, &pages[0]);
    assert_eq!(dict_name(&img.dict, "Subtype"), "Image");
    assert_eq!(dict_int(&img.dict, "Width"), 40);
    assert_eq!(dict_int(&img.dict, "Height"), 24);
    assert_eq!(dict_name(&img.dict, "ColorSpace"), "DeviceRGB");
    assert_eq!(dict_int(&img.dict, "BitsPerComponent"), 8);
    assert_eq!(dict_name(&img.dict, "Filter"), "FlateDecode");
}

#[test]
fn imgdoc_png_gray_devicegray() {
    let bytes = png_gray(16, 16);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    let (_n, img) = page_image(&doc, &pages[0]);
    assert_eq!(dict_name(&img.dict, "ColorSpace"), "DeviceGray");
}

#[test]
fn imgdoc_png_open_document_pixmap() {
    let bytes = png_rgb(20, 10);
    let doc = open_image_document(&bytes, Some(ImageFormat::Png)).unwrap();
    assert_eq!(doc.page_count(), 1);
    assert_eq!(doc.format, ImageFormat::Png);
    let pm = &doc.pages[0];
    assert_eq!((pm.width, pm.height), (20, 10));
    assert_eq!(pm.n, 3); // RGB, no alpha
    assert_eq!(pm.samples.len(), 20 * 10 * 3);
}

// ---------------------------------------------------------------------------
// IMGDOC-JPEG-* : passthrough -> /DCTDecode, byte-equal embedded stream
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_jpeg_dctdecode_passthrough_byte_equal() {
    let bytes = jpeg_rgb(32, 24);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Jpeg)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    assert_eq!(pages.len(), 1);
    let (_n, img) = page_image(&doc, &pages[0]);
    assert_eq!(dict_name(&img.dict, "Filter"), "DCTDecode");
    assert_eq!(dict_int(&img.dict, "Width"), 32);
    assert_eq!(dict_int(&img.dict, "Height"), 24);
    // The embedded stream must be byte-equal to the original JPEG bytes.
    let embedded = doc.stream_raw_bytes(&img).unwrap();
    assert_eq!(
        embedded.as_ref(),
        bytes.as_slice(),
        "JPEG bytes must be embedded verbatim (lossless passthrough)"
    );
}

#[test]
fn imgdoc_jpeg_colorspace_from_sof() {
    let bytes = jpeg_rgb(8, 8);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Jpeg)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    let (_n, img) = page_image(&doc, &pages[0]);
    // 3-component baseline JPEG -> DeviceRGB.
    assert_eq!(dict_name(&img.dict, "ColorSpace"), "DeviceRGB");
}

// ---------------------------------------------------------------------------
// IMGDOC-ALPHA-* : RGBA PNG -> /SMask present
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_alpha_smask_present() {
    let bytes = png_rgba(24, 24);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    let (_n, img) = page_image(&doc, &pages[0]);
    // The image must carry an /SMask referencing a DeviceGray image.
    let smask = img
        .dict
        .get(&Name::new("SMask"))
        .expect("RGBA -> /SMask present");
    let smask_obj = match smask {
        Object::Reference(r) => doc.resolve(*r).unwrap(),
        direct => std::sync::Arc::new(direct.clone()),
    };
    let sm = as_dict(&smask_obj);
    assert_eq!(dict_name(&sm, "Subtype"), "Image");
    assert_eq!(dict_name(&sm, "ColorSpace"), "DeviceGray");
    assert_eq!(dict_int(&sm, "Width"), 24);
    assert_eq!(dict_int(&sm, "Height"), 24);
}

#[test]
fn imgdoc_alpha_open_document_has_alpha() {
    let bytes = png_rgba(12, 8);
    let doc = open_image_document(&bytes, Some(ImageFormat::Png)).unwrap();
    let pm = &doc.pages[0];
    assert!(pm.alpha, "RGBA pixmap must report alpha");
    assert_eq!(pm.n, 4);
}

// ---------------------------------------------------------------------------
// IMGDOC-PALETTE-* : palette PNG -> /Indexed
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_palette_indexed_colorspace() {
    let bytes = png_indexed_native(16, 16);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    let (_n, img) = page_image(&doc, &pages[0]);
    // ColorSpace must be an [/Indexed /DeviceRGB hival lut] array.
    match img.dict.get(&Name::new("ColorSpace")) {
        Some(Object::Array(a)) => {
            assert!(matches!(&a[0], Object::Name(n) if n.as_str() == Some("Indexed")));
            assert!(matches!(&a[1], Object::Name(n) if n.as_str() == Some("DeviceRGB")));
            // hival = palette_len - 1; our palette has 4 colors -> 3.
            assert!(matches!(&a[2], Object::Integer(h) if *h >= 1 && *h <= 255));
        }
        other => panic!("expected /Indexed colorspace array, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// IMGDOC-TIFF-* : single and multi-IFD
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_tiff_single_page() {
    let bytes = tiff_rgb(30, 20);
    let doc = open_image_document(&bytes, Some(ImageFormat::Tiff)).unwrap();
    assert_eq!(doc.page_count(), 1);
    assert_eq!((doc.pages[0].width, doc.pages[0].height), (30, 20));
}

#[test]
fn imgdoc_tiff_multi_page_count_equals_ifds() {
    let bytes = tiff_multi(&[(10, 8), (12, 6), (4, 4)]);
    let doc = open_image_document(&bytes, Some(ImageFormat::Tiff)).unwrap();
    assert_eq!(doc.page_count(), 3, "page_count == IFD count");
    assert_eq!((doc.pages[0].width, doc.pages[0].height), (10, 8));
    assert_eq!((doc.pages[1].width, doc.pages[1].height), (12, 6));
    assert_eq!((doc.pages[2].width, doc.pages[2].height), (4, 4));
}

#[test]
fn imgdoc_tiff_multi_convert_pages() {
    let bytes = tiff_multi(&[(10, 8), (12, 6)]);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Tiff)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    assert_eq!(pages.len(), 2);
    assert_eq!(mediabox(&pages[0]), [0.0, 0.0, 10.0, 8.0]);
    assert_eq!(mediabox(&pages[1]), [0.0, 0.0, 12.0, 6.0]);
}

// ---------------------------------------------------------------------------
// IMGDOC-GIF-* : animated GIF -> one page per frame
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_gif_animated_one_page_per_frame() {
    let bytes = gif_animated(8, 8, 3);
    let doc = open_image_document(&bytes, Some(ImageFormat::Gif)).unwrap();
    assert_eq!(doc.page_count(), 3);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Gif)).unwrap();
    let pdoc = open_pdf(&pdf);
    assert_eq!(pages(&pdoc).len(), 3);
}

// ---------------------------------------------------------------------------
// IMGDOC-BMP-* / IMGDOC-FORMAT-* : other single-frame formats
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_bmp_single_page() {
    let bytes = bmp_rgb(18, 9);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Bmp)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    assert_eq!(pages.len(), 1);
    assert_eq!(mediabox(&pages[0]), [0.0, 0.0, 18.0, 9.0]);
}

#[test]
fn imgdoc_autodetect_format_from_bytes() {
    // format = None -> sniff.
    let bytes = png_rgb(8, 8);
    let pdf = convert_to_pdf(&bytes, None).unwrap();
    let doc = open_pdf(&pdf);
    assert_eq!(pages(&doc).len(), 1);

    let idoc = open_image_document(&jpeg_rgb(6, 6), None).unwrap();
    assert_eq!(idoc.format, ImageFormat::Jpeg);
}

// ---------------------------------------------------------------------------
// IMGDOC-CONVERT-* : output reparses clean + qpdf --check
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_convert_reparse_clean() {
    for bytes in [png_rgb(20, 20), jpeg_rgb(20, 20), bmp_rgb(20, 20)] {
        let pdf = convert_to_pdf(&bytes, None).unwrap();
        // Reparse must succeed and find the page.
        let doc = open_pdf(&pdf);
        assert_eq!(pages(&doc).len(), 1);
    }
}

#[test]
fn imgdoc_convert_qpdf_check() {
    // Only run if qpdf is available on PATH.
    let have_qpdf = std::process::Command::new("which")
        .arg("qpdf")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !have_qpdf {
        eprintln!("skipping qpdf --check (qpdf not found)");
        return;
    }
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("png", png_rgb(40, 30)),
        ("png-alpha", png_rgba(20, 20)),
        ("jpeg", jpeg_rgb(50, 20)),
        ("palette", png_indexed_native(16, 16)),
        ("tiff-multi", tiff_multi(&[(10, 8), (6, 6)])),
        ("gif-anim", gif_animated(8, 8, 2)),
    ];
    for (label, bytes) in cases {
        let pdf = convert_to_pdf(&bytes, None).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("oxipdf_imgdoc_{label}.pdf"));
        std::fs::write(&path, &pdf).unwrap();
        let out = std::process::Command::new("qpdf")
            .arg("--check")
            .arg(&path)
            .output()
            .expect("run qpdf");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "qpdf --check failed for {label}:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
        );
        let _ = std::fs::remove_file(&path);
    }
}

// ---------------------------------------------------------------------------
// IMGDOC-PROP-* : corrupt/arbitrary bytes -> typed error, no panic; caps
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_prop_non_image_invalid_argument() {
    let err = convert_to_pdf(b"this is definitely not an image", None).unwrap_err();
    assert_eq!(err.kind(), "invalid-argument");
    let err2 = open_image_document(b"\x00\x01\x02\x03plain garbage", None).unwrap_err();
    assert_eq!(err2.kind(), "invalid-argument");
}

#[test]
fn imgdoc_prop_truncated_png_is_typed_error() {
    let mut bytes = png_rgb(32, 32);
    bytes.truncate(20); // keep the signature, drop the body
                        // Sniff still says PNG, but decode must fail with a typed (decode) error.
    let err = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap_err();
    assert!(
        matches!(err.kind(), "decode" | "invalid-argument"),
        "got kind {}",
        err.kind()
    );
}

#[test]
fn imgdoc_prop_corrupt_jpeg_typed_error() {
    // Has a JPEG SOI but no SOF -> typed decode error, no panic.
    let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
    let err = convert_to_pdf(&bytes, Some(ImageFormat::Jpeg)).unwrap_err();
    assert_eq!(err.kind(), "decode");
}

#[test]
fn imgdoc_prop_arbitrary_bytes_never_panic() {
    // Fuzz-ish: a spread of byte patterns must never panic, only Err or Ok.
    let samples: Vec<Vec<u8>> = vec![
        vec![],
        vec![0xFF],
        vec![0x89, b'P', b'N', b'G'],           // PNG sig only
        vec![0xFF, 0xD8],                       // JPEG SOI only
        b"II\x2A\x00\x08\x00\x00\x00".to_vec(), // TIFF header, empty IFD ptr region
        b"GIF89a".to_vec(),
        b"RIFF\x00\x00\x00\x00WEBP".to_vec(),
        vec![0u8; 64],
    ];
    for s in samples {
        let _ = open_image_document(&s, None);
        let _ = convert_to_pdf(&s, None);
        // Forcing a format must also never panic.
        let _ = convert_to_pdf(&s, Some(ImageFormat::Png));
        let _ = convert_to_pdf(&s, Some(ImageFormat::Tiff));
        let _ = open_image_document(&s, Some(ImageFormat::Jpeg));
    }
}

#[test]
fn imgdoc_prop_cyclic_tiff_no_panic() {
    // TIFF whose IFD points back to itself -> typed error, no panic / no hang.
    // Header: II 42 first-IFD=8; IFD at 8: count=1 entry, next-IFD=8 (cycle).
    let mut t: Vec<u8> = Vec::new();
    t.extend_from_slice(&[0x49, 0x49]);
    t.extend_from_slice(&42u16.to_le_bytes());
    t.extend_from_slice(&8u32.to_le_bytes()); // first IFD at 8
    t.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    t.extend_from_slice(&256u16.to_le_bytes()); // tag ImageWidth
    t.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    t.extend_from_slice(&1u32.to_le_bytes()); // count 1
    t.extend_from_slice(&[1, 0, 0, 0]); // value
    t.extend_from_slice(&8u32.to_le_bytes()); // next IFD -> 8 (cycle!)
    let err = open_image_document(&t, Some(ImageFormat::Tiff)).unwrap_err();
    assert_eq!(err.kind(), "decode");
}

// ---------------------------------------------------------------------------
// Extra: LumaA helper smoke (ensure imports used)
// ---------------------------------------------------------------------------

#[test]
fn imgdoc_gray_alpha_smask() {
    let buf: ImageBuffer<LumaA<u8>, Vec<u8>> =
        ImageBuffer::from_fn(10, 10, |x, _| LumaA([(x % 256) as u8, 128]));
    let bytes = encode(&DynamicImage::ImageLumaA8(buf), ImgFmt::Png);
    let pdf = convert_to_pdf(&bytes, Some(ImageFormat::Png)).unwrap();
    let doc = open_pdf(&pdf);
    let pages = pages(&doc);
    let (_n, img) = page_image(&doc, &pages[0]);
    assert_eq!(dict_name(&img.dict, "ColorSpace"), "DeviceGray");
    assert!(img.dict.contains_key(&Name::new("SMask")));
}
