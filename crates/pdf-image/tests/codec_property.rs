//! `CODEC-PROP-*` — totality / fail-closed property tests (PRD §8.1 / §8.4.1).
//!
//! Every codec is **total**: arbitrary bytes (and arbitrary declared
//! dimensions) must terminate with either `Ok` or a typed `Err` — never a panic,
//! never an unbounded allocation. These properties feed each decoder random
//! payloads and random small-but-sometimes-hostile dimensions and assert the
//! call returns and respects the pixel cap.

mod codec_common;

use codec_common::*;

use pdf_core::Object;
use pdf_image::codecs::{ccitt, dct, decode_image_xobject, jbig2, jpx};

use proptest::prelude::*;

/// A params dict with the given (possibly hostile) dimensions.
fn dims_params(w: i64, h: i64) -> pdf_core::Dict {
    dict([("Width", int(w)), ("Height", int(h))])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // CODEC-PROP-001: DCT never panics on arbitrary bytes.
    #[test]
    fn codec_prop_001_dct_total(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let doc = empty_doc();
        let params = dims_params(8, 8);
        let _ = dct::decode(&doc, &data, &params);
    }

    // CODEC-PROP-002: CCITT never panics on arbitrary bytes + dims.
    #[test]
    fn codec_prop_002_ccitt_total(
        data in proptest::collection::vec(any::<u8>(), 0..512),
        w in 1i64..2048,
        h in 0i64..256,
        k in -1i64..3,
    ) {
        let doc = empty_doc();
        let params = dict([
            ("Width", int(w)),
            ("Height", int(h)),
            ("DecodeParms", Object::Dictionary(dict([
                ("K", int(k)),
                ("Columns", int(w)),
                ("Rows", int(h)),
            ]))),
        ]);
        let _ = ccitt::decode(&doc, &data, &params);
    }

    // CODEC-PROP-003: JBIG2 never panics on arbitrary bytes.
    #[test]
    fn codec_prop_003_jbig2_total(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let doc = empty_doc();
        let params = dims_params(16, 16);
        let _ = jbig2::decode(&doc, &data, &params);
    }

    // CODEC-PROP-004: JPX never panics on arbitrary bytes.
    #[test]
    fn codec_prop_004_jpx_total(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let doc = empty_doc();
        let params = dims_params(16, 16);
        let _ = jpx::decode(&doc, &data, &params);
    }

    // CODEC-PROP-005: dispatcher respects the pixel cap for any huge dims and
    // never panics for arbitrary filter names / bytes.
    #[test]
    fn codec_prop_005_dispatch_cap_and_total(
        data in proptest::collection::vec(any::<u8>(), 0..256),
        w in 1i64..200_000,
        h in 1i64..200_000,
        which in 0u8..6,
    ) {
        let doc = empty_doc();
        let filter = match which {
            0 => "DCTDecode",
            1 => "CCITTFaxDecode",
            2 => "JBIG2Decode",
            3 => "JPXDecode",
            4 => "",
            _ => "UnknownDecode",
        };
        let params = dict([
            ("Width", int(w)),
            ("Height", int(h)),
            ("BitsPerComponent", int(8)),
            ("ColorSpace", name_obj("DeviceGray")),
        ]);
        let res = decode_image_xobject(&doc, filter, &data, &params);
        // If the declared raster is beyond the cap, the raw-sample path and the
        // codec paths must NOT return an Ok with a giant buffer.
        if let Ok(img) = res {
            let pixels = u64::from(img.width) * u64::from(img.height);
            prop_assert!(
                pixels <= 256 * 1024 * 1024,
                "decoded raster {pixels} px exceeds the cap"
            );
            // And the data buffer is bounded by the geometry.
            let max_bytes = pixels
                .saturating_mul(u64::from(img.components))
                .saturating_mul(2);
            prop_assert!(img.data.len() as u64 <= max_bytes);
        }
    }
}
