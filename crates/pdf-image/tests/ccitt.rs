//! `CCITT-*` — CCITTFaxDecode (Group 3 / Group 4) codec tests (PRD §8.4).
//!
//! Known bitmaps are encoded to Group 4 (T.6) with the `fax` crate's encoder,
//! then decoded back through our `hayro-ccitt`-backed codec and compared. The
//! `fax` encoder emits an `EOFB`, so we decode with `EndOfBlock` true.

mod codec_common;

use codec_common::*;

use pdf_core::Object;
use pdf_image::codecs::{ccitt, ColorSpaceHint};

use fax::encoder::Encoder;
use fax::{Color, VecWriter};

/// Encodes a packed (MSB-first, 1=black) bitmap to CCITT Group 4. Each row is
/// `width` pixels; `bits` is row-major, byte-aligned per row.
fn encode_g4(bits: &[u8], width: u32, rows: u32) -> Vec<u8> {
    let row_bytes = (width as usize).div_ceil(8);
    let mut enc = Encoder::new(VecWriter::new());
    for r in 0..rows as usize {
        let row = &bits[r * row_bytes..(r + 1) * row_bytes];
        let pels = (0..width as usize).map(|x| {
            let byte = row[x / 8];
            let bit = (byte >> (7 - (x % 8))) & 1;
            if bit == 1 {
                Color::Black
            } else {
                Color::White
            }
        });
        enc.encode_line(pels, width as u16).expect("encode line");
    }
    enc.finish().expect("finish").finish()
}

/// Builds a packed bitmap from a closure mapping (x,y) → is_black, in the
/// `1 = black` convention used by [`encode_g4`].
fn make_bitmap(width: u32, rows: u32, f: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let row_bytes = (width as usize).div_ceil(8);
    let mut out = vec![0u8; row_bytes * rows as usize];
    for y in 0..rows {
        for x in 0..width {
            if f(x, y) {
                out[y as usize * row_bytes + (x / 8) as usize] |= 1 << (7 - (x % 8));
            }
        }
    }
    out
}

/// The decoder emits the standard 1-bpc DeviceGray polarity (`1 = white`), the
/// bitwise inverse of the `1 = black` reference from [`make_bitmap`]. All test
/// widths are multiples of 8, so there are no padding bits to differ.
fn to_devicegray(black_packed: &[u8]) -> Vec<u8> {
    black_packed.iter().map(|b| !b).collect()
}

// --- CCITT-G4-001: round-trip a known bitmap ------------------------------

#[test]
fn ccitt_g4_001_roundtrip() {
    let doc = empty_doc();
    let (w, h) = (16u32, 8u32);
    // Vertical stripes: black where x is even.
    let original = make_bitmap(w, h, |x, _| x % 2 == 0);
    let encoded = encode_g4(&original, w, h);

    let params = dict([
        ("Width", int(w as i64)),
        ("Height", int(h as i64)),
        (
            "DecodeParms",
            Object::Dictionary(dict([
                ("K", int(-1)),
                ("Columns", int(w as i64)),
                ("Rows", int(h as i64)),
                ("EndOfBlock", boolean(true)),
            ])),
        ),
    ]);

    let img = ccitt::decode(&doc, &encoded, &params).expect("decode g4");
    assert_eq!((img.width, img.height), (w, h));
    assert_eq!(img.components, 1);
    assert_eq!(img.bits, 1);
    assert_eq!(img.colorspace, ColorSpaceHint::Gray);
    assert_eq!(img.data, to_devicegray(&original), "G4 round trip mismatch");
}

// --- CCITT-G4-002: a more complex pattern ---------------------------------

#[test]
fn ccitt_g4_002_roundtrip_pattern() {
    let doc = empty_doc();
    let (w, h) = (24u32, 12u32);
    // A diagonal + a solid block.
    let original = make_bitmap(w, h, |x, y| {
        x == y || ((4..10).contains(&x) && (2..6).contains(&y))
    });
    let encoded = encode_g4(&original, w, h);

    let params = dict([
        ("Width", int(w as i64)),
        ("Height", int(h as i64)),
        (
            "DecodeParms",
            Object::Dictionary(dict([
                ("K", int(-1)),
                ("Columns", int(w as i64)),
                ("Rows", int(h as i64)),
            ])),
        ),
    ]);

    let img = ccitt::decode(&doc, &encoded, &params).expect("decode g4 pattern");
    assert_eq!(
        img.data,
        to_devicegray(&original),
        "G4 pattern round trip mismatch"
    );
}

// --- CCITT-BLACKIS1-001: /BlackIs1 inverts the bitmap ---------------------

#[test]
fn ccitt_blackis1_001_inverts() {
    let doc = empty_doc();
    let (w, h) = (16u32, 4u32);
    let original = make_bitmap(w, h, |x, _| x < 8); // left half black

    let encoded = encode_g4(&original, w, h);
    let base = |black_is_1: bool| {
        dict([
            ("Width", int(w as i64)),
            ("Height", int(h as i64)),
            (
                "DecodeParms",
                Object::Dictionary(dict([
                    ("K", int(-1)),
                    ("Columns", int(w as i64)),
                    ("Rows", int(h as i64)),
                    ("BlackIs1", boolean(black_is_1)),
                ])),
            ),
        ])
    };

    let normal = ccitt::decode(&doc, &encoded, &base(false)).expect("decode normal");
    let inverted = ccitt::decode(&doc, &encoded, &base(true)).expect("decode blackis1");
    assert_eq!(normal.data.len(), inverted.data.len());
    // BlackIs1 flips every pixel.
    for (a, b) in normal.data.iter().zip(inverted.data.iter()) {
        assert_eq!(*a, !*b, "BlackIs1 must invert each byte");
    }
}

// --- CCITT-DEFAULT-COLUMNS-001: default /Columns 1728 ---------------------

#[test]
fn ccitt_default_columns_001() {
    let doc = empty_doc();
    // A 1-row all-white image at the default width 1728. fax encoder needs the
    // exact width; encode an all-white 1728x1 row.
    let (w, h) = (1728u32, 1u32);
    let original = make_bitmap(w, h, |_, _| false);
    let encoded = encode_g4(&original, w, h);

    // No /Columns ⇒ default 1728. /Rows from /Height.
    let params = dict([
        ("Width", int(w as i64)),
        ("Height", int(h as i64)),
        ("DecodeParms", Object::Dictionary(dict([("K", int(-1))]))),
    ]);
    let img = ccitt::decode(&doc, &encoded, &params).expect("decode default columns");
    assert_eq!(img.width, 1728);
    assert_eq!(img.data, to_devicegray(&original));
}

// --- CCITT-ERR-001: garbage fails closed (no panic) -----------------------

#[test]
fn ccitt_err_001_garbage() {
    let doc = empty_doc();
    let params = dict([
        ("Width", int(16)),
        ("Height", int(4)),
        (
            "DecodeParms",
            Object::Dictionary(dict([
                ("K", int(-1)),
                ("Columns", int(16)),
                ("Rows", int(4)),
            ])),
        ),
    ]);
    // Random non-fax bytes: must not panic; either errors or produces a bounded
    // raster of the declared size.
    let res = ccitt::decode(&doc, &[0xFF, 0x00, 0xAB, 0xCD, 0x12, 0x34], &params);
    if let Ok(img) = res {
        assert_eq!((img.width, img.height), (16, 4));
        assert_eq!(img.data.len(), 2 * 4);
    }
}
