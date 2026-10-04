#![no_main]
//! Stream-filter fuzz target: every decoder (Flate, LZW, ASCIIHex, ASCII85,
//! RunLength), the PNG/TIFF predictors and short filter chains, under bounded
//! limits. The first input bytes select the chain and predictor parameters;
//! the rest is the encoded stream. Must never panic, OOM or hang — the
//! per-stream cap and the decode-ratio guard bound every decode.
//! Run with `cargo +nightly fuzz run fuzz_filters`.

use libfuzzer_sys::fuzz_target;
use pdf_core::{Dict, Limits, Name, Object};

const FILTERS: [&str; 6] = [
    "FlateDecode",
    "LZWDecode",
    "ASCIIHexDecode",
    "ASCII85Decode",
    "RunLengthDecode",
    "DCTDecode",
];

fuzz_target!(|data: &[u8]| {
    let [sel, pred, cols, rest @ ..] = data else {
        return;
    };
    // 1–3 filters, chosen from `sel`'s bit fields.
    let count = 1 + usize::from(sel & 0b11) % 3;
    let chain: Vec<Object> = (0..count)
        .map(|i| {
            let k = usize::from(sel >> (2 + 2 * i)) % FILTERS.len();
            Object::Name(Name::new(FILTERS[k]))
        })
        .collect();
    let mut dict = Dict::new();
    dict.insert(Name::new("Filter"), Object::Array(chain));
    if pred & 1 == 1 {
        // A predictor on the first filter: TIFF (2) or one of the PNG ones.
        let predictor = [2, 10, 11, 12, 13, 14, 15][usize::from(pred >> 1) % 7];
        let mut parms = Dict::new();
        parms.insert(Name::new("Predictor"), Object::Integer(predictor));
        parms.insert(Name::new("Columns"), Object::Integer(i64::from(*cols)));
        parms.insert(
            Name::new("Colors"),
            Object::Integer(i64::from(pred >> 4 & 3) + 1),
        );
        parms.insert(
            Name::new("BitsPerComponent"),
            Object::Integer([1, 2, 4, 8, 16][usize::from(pred >> 6) % 5]),
        );
        dict.insert(Name::new("DecodeParms"), Object::Dictionary(parms));
    }
    // A 64 MiB per-stream cap keeps each run fast (ratio floor 16 MiB).
    let mut limits = Limits::default();
    limits.max_decompressed_stream = 64 * 1024 * 1024;
    let _ = pdf_core::decode_stream(&dict, rest, &limits);
});
