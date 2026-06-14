//! `INTERP-ROBUST-*` — the interpreter is total: arbitrary / truncated content
//! never panics, unknown operators + operand underflow are tolerated, and every
//! emitted glyph carries finite geometry (PRD §8.1 / §8.6.2).

mod common;

use common::*;
use pdf_core::Object;
use proptest::prelude::*;

/// A font where every code has width 500 (resource `F1`).
fn font_w500() -> Object {
    let widths: Vec<i64> = (0..95).map(|_| 500).collect();
    winansi_type1("Helvetica", 32, &widths)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    // INTERP-ROBUST-001: arbitrary bytes as content never panic.
    #[test]
    fn interp_robust_001_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let res = run_with_font(font_w500(), &bytes);
        // Every glyph (if any) has finite geometry.
        for g in &res.glyphs {
            prop_assert!(g.origin.x.is_finite() && g.origin.y.is_finite());
            prop_assert!(g.bbox.x0.is_finite() && g.bbox.y0.is_finite());
            prop_assert!(g.bbox.x1.is_finite() && g.bbox.y1.is_finite());
        }
    }

    // INTERP-ROBUST-002: a stream of random *tokens* (operators + operands)
    // including unknown operators + operand underflow never panics.
    #[test]
    fn interp_robust_002_random_tokens(tokens in proptest::collection::vec(token_strategy(), 0..200)) {
        let mut content = String::from("BT /F1 10 Tf ");
        for t in &tokens {
            content.push_str(t);
            content.push(' ');
        }
        content.push_str("ET");
        let res = run_with_font(font_w500(), content.as_bytes());
        for g in &res.glyphs {
            prop_assert!(g.bbox.x0.is_finite() && g.bbox.x1.is_finite());
        }
    }

    // INTERP-ROBUST-003: truncated structures (BT / string / TJ array) never
    // panic — feed prefixes of a well-formed stream.
    #[test]
    fn interp_robust_003_truncations(cut in 0usize..64) {
        let full = b"BT /F1 10 Tf 1 0 0 1 0 700 Tm [(Hello) -50 (World)] TJ T* (next) Tj ET";
        let end = cut.min(full.len());
        let res = run_with_font(font_w500(), &full[..end]);
        for g in &res.glyphs {
            prop_assert!(g.origin.x.is_finite());
        }
    }

    // INTERP-ROBUST-004: every emitted glyph has a finite bbox/origin even with
    // extreme matrices.
    #[test]
    fn interp_robust_004_extreme_matrices(
        a in -1e6f64..1e6, d in -1e6f64..1e6, e in -1e9f64..1e9, f in -1e9f64..1e9,
    ) {
        let content = format!("BT /F1 10 Tf {a} 0 0 {d} {e} {f} Tm (AB) Tj ET");
        let res = run_with_font(font_w500(), content.as_bytes());
        for g in &res.glyphs {
            prop_assert!(g.origin.x.is_finite() && g.origin.y.is_finite());
            prop_assert!(g.bbox.x0.is_finite() && g.bbox.x1.is_finite());
            prop_assert!(g.bbox.y0.is_finite() && g.bbox.y1.is_finite());
        }
    }
}

/// A strategy producing content-stream token strings: numbers, names, strings,
/// known operators and garbage operators.
fn token_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        (-1000i64..1000).prop_map(|n| n.to_string()),
        any::<f32>().prop_map(|f| format!("{:.3}", f)),
        "[A-Za-z]{1,4}".prop_map(|s| s), // garbage operator / name run
        Just("(abc)".to_string()),
        Just("[(x) -100 (y)]".to_string()),
        Just("Tj".to_string()),
        Just("TJ".to_string()),
        Just("Td".to_string()),
        Just("Tm".to_string()),
        Just("Tc".to_string()),
        Just("Tw".to_string()),
        Just("q".to_string()),
        Just("Q".to_string()),
        Just("cm".to_string()),
        Just("T*".to_string()),
        Just("/F1".to_string()),
    ]
}
