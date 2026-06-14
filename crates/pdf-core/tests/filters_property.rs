//! Filter property tests — the `FILTER-PROP-*` / `FLATE-PROP-*` / `LZW-PROP-*`
//! / `AHX-PROP-*` / `A85-PROP-*` / `RL-PROP-*` catalog (M1b).
//!
//! Spec source of truth: PRD §10.7 — `decode(encode(x)) == x ∀x`,
//! `unpredict(predict(rows,cfg)) == rows`, and "arbitrary bytes never panic".
//! Generators are ours (no external corpora).

use pdf_core::filters::predictor::{predict, unpredict, PredictorParams};
use pdf_core::filters::{ascii85, ascii_hex, flate, lzw, run_length};
use pdf_core::Limits;
use proptest::prelude::*;

fn unbounded() -> Limits {
    Limits::unbounded_decode()
}

proptest! {
    // FLATE-PROP-001: flate round-trip.
    #[test]
    fn flate_prop_001_roundtrip(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let dec = flate::decode(&flate::encode(&x), &unbounded()).unwrap();
        prop_assert_eq!(dec, x);
    }

    // FLATE-PROP-003: flate decode on arbitrary bytes never panics.
    #[test]
    fn flate_prop_003_no_panic(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = flate::decode(&x, &unbounded()); // Ok or Err, just no panic.
    }

    // LZW-PROP-001: lzw round-trip (EarlyChange = 1, the PDF default).
    #[test]
    fn lzw_prop_001_roundtrip(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let dec = lzw::decode(&lzw::encode(&x, true), true, &unbounded()).unwrap();
        prop_assert_eq!(dec, x);
    }

    // LZW-PROP-001b: also holds for EarlyChange = 0.
    #[test]
    fn lzw_prop_001b_roundtrip_ec0(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let dec = lzw::decode(&lzw::encode(&x, false), false, &unbounded()).unwrap();
        prop_assert_eq!(dec, x);
    }

    // LZW-PROP-002: lzw decode on arbitrary bytes never panics (both flags).
    #[test]
    fn lzw_prop_002_no_panic(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = lzw::decode(&x, true, &unbounded());
        let _ = lzw::decode(&x, false, &unbounded());
    }

    // AHX-PROP-001: ascii_hex round-trip + never panics on arbitrary bytes.
    #[test]
    fn ahx_prop_001(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let dec = ascii_hex::decode(&ascii_hex::encode(&x), &unbounded()).unwrap();
        prop_assert_eq!(dec, x.clone());
        let _ = ascii_hex::decode(&x, &unbounded()); // arbitrary input: no panic.
    }

    // A85-PROP-001: ascii85 round-trip + never panics on arbitrary bytes.
    #[test]
    fn a85_prop_001(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let dec = ascii85::decode(&ascii85::encode(&x), &unbounded()).unwrap();
        prop_assert_eq!(dec, x.clone());
        let _ = ascii85::decode(&x, &unbounded());
    }

    // RL-PROP-001: run_length round-trip + never panics on arbitrary bytes.
    #[test]
    fn rl_prop_001(x in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let dec = run_length::decode(&run_length::encode(&x), &unbounded()).unwrap();
        prop_assert_eq!(dec, x.clone());
        let _ = run_length::decode(&x, &unbounded());
    }
}

// FLATE-PROP-002: unpredict(predict(rows, cfg)) == rows, over PNG and TIFF2
// predictors and a matrix of Colors/BitsPerComponent/Columns.

/// A predictor config plus a row count, sized so the raw buffer is an exact
/// number of full rows for the chosen stride.
fn predictor_cfg() -> impl Strategy<Value = (PredictorParams, usize)> {
    // predictor: 2 (TIFF) or 10..=15 (PNG); bpc in {1,2,4,8,16}; colors 1..=4;
    // columns 1..=8; rows 0..=6.
    let predictor = prop_oneof![Just(2i64), 10..=15i64];
    (
        predictor,
        1usize..=4,
        prop_oneof![Just(1usize), Just(2), Just(4), Just(8), Just(16)],
        1usize..=8,
        0usize..=6,
    )
        .prop_map(|(predictor, colors, bpc, columns, rows)| {
            (
                PredictorParams {
                    predictor,
                    colors,
                    bits_per_component: bpc,
                    columns,
                },
                rows,
            )
        })
}

/// Bytes per row for a config: ceil(colors*bpc*columns/8).
fn row_bytes(p: &PredictorParams) -> usize {
    (p.colors * p.bits_per_component * p.columns).div_ceil(8)
}

/// Zeroes the padding bits in the final byte(s) of each row. TIFF predictor 2
/// on sub-byte BPC operates only on the `colors*columns` significant samples
/// and (legitimately) drops trailing padding bits; masking them makes the
/// round-trip comparison fair. PNG predictors are byte-wise and unaffected, so
/// masking is a harmless no-op there.
fn mask_row_padding(raw: &mut [u8], p: &PredictorParams) {
    let rb = row_bytes(p);
    if rb == 0 {
        return;
    }
    let sample_bits = p.colors * p.bits_per_component * p.columns;
    let used_bits_in_last = sample_bits - (rb - 1) * 8; // 1..=8
    if used_bits_in_last == 8 {
        return; // no padding
    }
    let keep_mask = (0xffu16 << (8 - used_bits_in_last)) as u8;
    for row in raw.chunks_mut(rb) {
        if let Some(last) = row.last_mut() {
            *last &= keep_mask;
        }
    }
}

proptest! {
    #[test]
    fn flate_prop_002_predictor_inverse((p, rows) in predictor_cfg(), seed in any::<u64>()) {
        let rb = row_bytes(&p);
        let total = rb * rows;
        // Deterministic LCG fill so the raw buffer is exactly `rows` full rows.
        let mut state = seed;
        let mut raw: Vec<u8> = (0..total)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (state >> 33) as u8
            })
            .collect();
        mask_row_padding(&mut raw, &p);

        let encoded = predict(&raw, &p).unwrap();
        let decoded = unpredict(&encoded, &p, &unbounded()).unwrap();
        prop_assert_eq!(decoded, raw);
    }
}
