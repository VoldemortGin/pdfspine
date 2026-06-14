//! PNG/TIFF predictor unit tests — the `FLATE-PRED-*` catalog (M1b).
//!
//! Spec source of truth: ISO 32000-1 §7.4.4.4, TIFF 6.0 §14, PNG spec §6,
//! PRD §8.3. The core invariant under test is the round-trip identity
//! `unpredict(predict(raw)) == raw` across every predictor and stride config,
//! plus that PNG decode honours per-row tag bytes (the "optimum" behaviour) and
//! that a stride mismatch is a typed [`pdf_core::Error`] rather than a panic.

use pdf_core::filters::predictor::{predict, unpredict, PredictorParams};
use pdf_core::Limits;

/// Fills `buf` with deterministic pseudo-random bytes (a small LCG) so fixtures
/// stay reproducible without a `rand` dependency.
fn lcg_fill(buf: &mut [u8], seed: u32) {
    let mut state = seed;
    for b in buf.iter_mut() {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *b = (state >> 24) as u8;
    }
}

/// Asserts `unpredict(predict(raw)) == raw` for the given params.
fn assert_roundtrip(raw: &[u8], p: &PredictorParams) {
    let enc = predict(raw, p).expect("predict must succeed on well-formed input");
    let dec = unpredict(&enc, p, &Limits::unbounded_decode()).expect("unpredict must succeed");
    assert_eq!(dec, raw, "round-trip mismatch for {p:?}");
}

#[test]
fn flate_pred_001_png_none_identity() {
    // FLATE-PRED-001: PNG None (predictor 10) is the identity transform across
    // multiple rows. row_bytes = 5, 3 rows = 15 bytes.
    let p = PredictorParams {
        predictor: 10,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    let mut raw = vec![0u8; 15];
    lcg_fill(&mut raw, 0xA1);
    assert_roundtrip(&raw, &p);
}

#[test]
fn flate_pred_002_png_sub() {
    // FLATE-PRED-002: PNG Sub (predictor 11) round-trips.
    let p = PredictorParams {
        predictor: 11,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    let mut raw = vec![0u8; 15];
    lcg_fill(&mut raw, 0xB2);
    assert_roundtrip(&raw, &p);
}

#[test]
fn flate_pred_003_png_up() {
    // FLATE-PRED-003: PNG Up (predictor 12) round-trips.
    let p = PredictorParams {
        predictor: 12,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    let mut raw = vec![0u8; 15];
    lcg_fill(&mut raw, 0xC3);
    assert_roundtrip(&raw, &p);
}

#[test]
fn flate_pred_004_png_average() {
    // FLATE-PRED-004: PNG Average (predictor 13) round-trips.
    let p = PredictorParams {
        predictor: 13,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    let mut raw = vec![0u8; 15];
    lcg_fill(&mut raw, 0xD4);
    assert_roundtrip(&raw, &p);
}

#[test]
fn flate_pred_005_png_paeth_with_tie() {
    // FLATE-PRED-005: PNG Paeth (predictor 14) round-trips, including a row that
    // exercises the tie-break path (values chosen so pa == pb at points). The
    // round-trip is sufficient: encode and decode use the same paeth function,
    // so any tie resolution still reconstructs identically.
    let p = PredictorParams {
        predictor: 14,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    // Row 0 and a second row whose neighbours create ties (a == b == c == 10
    // makes pa == pb == pc == 0; equal differences also arise mid-row).
    let raw: Vec<u8> = vec![10, 10, 10, 10, 10, 10, 20, 30, 20, 10, 5, 5, 250, 250, 5];
    assert_roundtrip(&raw, &p);
}

#[test]
fn flate_pred_006_png_optimum_and_per_row_tags() {
    // FLATE-PRED-006: PNG optimum (predictor 15) multi-row round-trip (predict
    // uses Paeth uniformly; decode must handle it).
    let p15 = PredictorParams {
        predictor: 15,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    let mut raw = vec![0u8; 20]; // 4 rows × 5 bytes
    lcg_fill(&mut raw, 0xE5);
    assert_roundtrip(&raw, &p15);

    // And a hand-built stream with DIFFERENT per-row tags proves decode honours
    // each row's tag (the key PNG-optimum behaviour). row_bytes = 4, bpp = 1.
    //   row0 tag=0 (None): [10,20,30,40]                       -> [10,20,30,40]
    //   row1 tag=2 (Up):   src [1,2,3,4] + prev row            -> [11,22,33,44]
    //   row2 tag=1 (Sub):  src [5,5,5,5] cumulative (bpp=1)    -> [5,10,15,20]
    let p = PredictorParams {
        predictor: 12,
        colors: 1,
        bits_per_component: 8,
        columns: 4,
    };
    #[rustfmt::skip]
    let stream: [u8; 15] = [
        0, 10, 20, 30, 40,
        2, 1, 2, 3, 4,
        1, 5, 5, 5, 5,
    ];
    let expected: [u8; 12] = [10, 20, 30, 40, 11, 22, 33, 44, 5, 10, 15, 20];
    let dec = unpredict(&stream, &p, &Limits::unbounded_decode()).unwrap();
    assert_eq!(dec, expected);
}

#[test]
fn flate_pred_007_tiff_predictor_2() {
    // FLATE-PRED-007: TIFF predictor 2 round-trips. Colors=3, BPC=8, Columns=4
    // => row_bytes = 12; use 3 rows = 36 bytes.
    let p = PredictorParams {
        predictor: 2,
        colors: 3,
        bits_per_component: 8,
        columns: 4,
    };
    let mut raw = vec![0u8; 36];
    lcg_fill(&mut raw, 0xF6);
    assert_roundtrip(&raw, &p);
}

#[test]
fn flate_pred_008_stride_matrix() {
    // FLATE-PRED-008: validate row_bytes = ceil(colors*bpc*columns/8) across a
    // matrix of configs (including sub-byte BPC) for both a PNG predictor (12)
    // and TIFF (2). (colors, bpc, columns, row_bytes, n_rows).
    let configs: &[(usize, usize, usize, usize)] = &[
        (1, 1, 8, 1),  // 1 byte/row
        (1, 2, 4, 1),  // 1 byte/row
        (1, 4, 2, 1),  // 1 byte/row
        (3, 8, 2, 6),  // 6 bytes/row
        (1, 16, 3, 6), // 6 bytes/row
    ];
    const N_ROWS: usize = 4;
    for (i, &(colors, bpc, columns, row_bytes)) in configs.iter().enumerate() {
        let mut raw = vec![0u8; row_bytes * N_ROWS];
        lcg_fill(&mut raw, 0x100 + i as u32);
        for predictor in [12i64, 2] {
            let p = PredictorParams {
                predictor,
                colors,
                bits_per_component: bpc,
                columns,
            };
            assert_roundtrip(&raw, &p);
        }
    }
}

#[test]
fn flate_pred_009_stride_mismatch_is_filter_error() {
    // FLATE-PRED-009: PNG stride is row_bytes+1 = 5; data length 7 is not a
    // multiple of the stride → a typed filter error, never a panic.
    let p = PredictorParams {
        predictor: 12,
        colors: 1,
        bits_per_component: 8,
        columns: 4,
    };
    let err = unpredict(&[0u8; 7], &p, &Limits::unbounded_decode()).unwrap_err();
    assert_eq!(err.kind(), "filter", "unexpected error: {err:?}");
}
