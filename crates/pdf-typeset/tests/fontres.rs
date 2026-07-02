//! TS-2 resolver tests against an **injected deterministic database** —
//! bundled Liberation/Noto faces plus synthesized fixtures only, no system
//! font dependence (green on all 3 CI OSes; PRD §10 TS-2 acceptance).

use pdf_fonts::liberation::{liberation_face, LiberationFamily};
use pdf_typeset::{ExportWarning, FontResolver, Platform, Substitutions};

/// Collects the resolver output of one request.
fn resolve(
    resolver: &FontResolver,
    family: &str,
    bold: bool,
    italic: bool,
) -> (pdf_typeset::ResolvedFace, Vec<ExportWarning>) {
    let mut warnings = Vec::new();
    let face = resolver.resolve(family, bold, italic, &mut warnings);
    (face, warnings)
}

#[test]
fn bundled_faces_resolve_without_warnings() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (face, warnings) = resolve(&resolver, "Liberation Sans", false, false);
    assert_eq!(face.family, "Liberation Sans");
    assert_eq!(face.post_script_name, "LiberationSans");
    assert_eq!(face.index, 0);
    assert!(warnings.is_empty());
}

#[test]
fn bold_italic_map_to_weight_style_slots() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    for (bold, italic, expected_ps) in [
        (true, false, "LiberationSans-Bold"),
        (false, true, "LiberationSans-Italic"),
        (true, true, "LiberationSans-BoldItalic"),
    ] {
        let (face, warnings) = resolve(&resolver, "Liberation Sans", bold, italic);
        assert_eq!(face.post_script_name, expected_ps);
        assert!(warnings.is_empty(), "{expected_ps}: {warnings:?}");
    }
}

#[test]
fn folded_name_index_matches_case_width_and_space_variants() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (canonical, _) = resolve(&resolver, "Liberation Sans", false, false);
    for variant in [
        "liberation sans",
        "LIBERATION SANS",
        "  Liberation \t Sans ",
        "Ｌｉｂｅｒａｔｉｏｎ\u{3000}Ｓａｎｓ", // full-width + ideographic space
    ] {
        let (face, warnings) = resolve(&resolver, variant, false, false);
        assert_eq!(face.key(), canonical.key(), "variant {variant:?}");
        assert!(warnings.is_empty(), "variant {variant:?}: {warnings:?}");
    }
}

#[test]
fn unknown_family_returns_liberation_with_exactly_one_substitution_warning() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (face, warnings) = resolve(&resolver, "Definitely Not A Font 42", false, false);
    assert_eq!(face.family, "Liberation Sans");
    assert_eq!(
        warnings,
        vec![ExportWarning::FontSubstituted {
            requested: "Definitely Not A Font 42".to_string(),
            used: "Liberation Sans".to_string(),
        }]
    );
    // Style request still lands on the right slot — and still exactly one warning.
    let (bold_face, bold_warnings) = resolve(&resolver, "Definitely Not A Font 42", true, false);
    assert_eq!(bold_face.post_script_name, "LiberationSans-Bold");
    assert_eq!(bold_warnings.len(), 1);
}

#[test]
fn times_new_roman_substitutes_bundled_liberation_serif_on_all_platforms() {
    for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
        let resolver = FontResolver::with_platform(platform);
        let (face, warnings) = resolve(&resolver, "Times New Roman", true, false);
        assert_eq!(face.family, "Liberation Serif", "{platform:?}");
        assert_eq!(face.post_script_name, "LiberationSerif-Bold");
        assert_eq!(
            warnings,
            vec![ExportWarning::FontSubstituted {
                requested: "Times New Roman".to_string(),
                used: "Liberation Serif".to_string(),
            }],
            "{platform:?}"
        );
    }
}

#[test]
fn cjk_requests_fall_through_absent_candidates_to_the_bundled_fallback() {
    // Bundled-only database: no Songti/SimSun/Noto CJK anywhere, so 宋体 walks
    // its whole candidate list and terminates at Liberation Sans — with
    // exactly one FontSubstituted warning, never an error.
    for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
        let resolver = FontResolver::with_platform(platform);
        for requested in ["宋体", "SimSun", "微软雅黑", "Microsoft YaHei"] {
            let (face, warnings) = resolve(&resolver, requested, false, false);
            assert_eq!(face.family, "Liberation Sans", "{platform:?}/{requested}");
            assert_eq!(warnings.len(), 1, "{platform:?}/{requested}: {warnings:?}");
        }
    }
}

#[test]
fn user_substitutions_override_and_respect_candidate_order() {
    let mut resolver = FontResolver::with_platform(Platform::MacOs);
    resolver.add_substitution("MyCorpFont", &["No Such Family", "Liberation Mono"]);
    let (face, warnings) = resolve(&resolver, "MyCorpFont", false, false);
    assert_eq!(face.family, "Liberation Mono");
    assert_eq!(
        warnings,
        vec![ExportWarning::FontSubstituted {
            requested: "MyCorpFont".to_string(),
            used: "Liberation Mono".to_string(),
        }]
    );

    // Replacing the whole table drops the built-in rows.
    resolver.set_substitutions(Substitutions::empty());
    let (face, warnings) = resolve(&resolver, "Times New Roman", false, false);
    assert_eq!(face.family, "Liberation Sans");
    assert_eq!(warnings.len(), 1);
}

#[test]
fn missing_style_slot_degrades_to_nearest_face_with_style_warning() {
    // Noto Sans Math ships Regular only — bold must NOT be synthesized.
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (face, warnings) = resolve(&resolver, "Noto Sans Math", true, false);
    assert_eq!(face.family, "Noto Sans Math");
    assert_eq!(face.post_script_name, "NotoSansMath-Regular");
    assert_eq!(
        warnings,
        vec![ExportWarning::StyleApproximated {
            family: "Noto Sans Math".to_string(),
            bold: true,
            italic: false,
        }]
    );
}

#[test]
fn glyph_coverage_checks_resolve_through_the_cmap() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (latin, _) = resolve(&resolver, "Liberation Sans", false, false);
    assert!(resolver.has_glyph(&latin, 'A'));
    assert!(resolver.has_glyph(&latin, 'ä'));
    assert!(!resolver.has_glyph(&latin, '中'));
    assert!(!resolver.has_glyph(&latin, '𝔸'));
}

#[test]
fn per_char_fallback_walks_to_the_bundled_noto_tail() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (base, _) = resolve(&resolver, "Liberation Sans", false, false);

    // Covered char: same face back, no warning.
    let mut warnings = Vec::new();
    let same = resolver.resolve_char(&base, 'A', &mut warnings);
    assert_eq!(same.key(), base.key());
    assert!(warnings.is_empty());

    // Double-struck A lives in Noto Sans Math (bundled), not Liberation.
    let mut warnings = Vec::new();
    let math = resolver.resolve_char(&base, '𝔸', &mut warnings);
    assert_eq!(math.family, "Noto Sans Math");
    assert!(resolver.has_glyph(&math, '𝔸'));
    assert_eq!(
        warnings,
        vec![ExportWarning::GlyphFallback {
            ch: '𝔸',
            family: "Liberation Sans".to_string(),
        }]
    );
}

#[test]
fn exhausted_fallback_chain_degrades_to_notdef_with_warning() {
    // Nothing bundled covers CJK — the chain exhausts, the base face comes
    // back (drawn as .notdef downstream), and the miss is warned. Never an
    // error, never a panic.
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (base, _) = resolve(&resolver, "Liberation Sans", false, false);
    let mut warnings = Vec::new();
    let face = resolver.resolve_char(&base, '中', &mut warnings);
    assert_eq!(face.key(), base.key());
    assert_eq!(
        warnings,
        vec![ExportWarning::GlyphFallback {
            ch: '中',
            family: "Liberation Sans".to_string(),
        }]
    );
}

#[test]
fn face_data_hands_back_the_full_program_bytes() {
    let resolver = FontResolver::with_platform(Platform::MacOs);
    let (face, _) = resolve(&resolver, "Liberation Serif", false, false);
    let data = resolver.face_data(&face).expect("bundled face bytes");
    // Whole standalone TTF, sfnt magic 0x00010000.
    assert_eq!(&data[0..4], &[0x00, 0x01, 0x00, 0x00]);
    assert_eq!(
        data.len(),
        liberation_face(LiberationFamily::Serif, false, false).len()
    );
    // Second call serves the cache (same Arc).
    let again = resolver.face_data(&face).expect("cached bytes");
    assert!(std::sync::Arc::ptr_eq(&data, &again));
}

#[test]
fn resolution_is_deterministic_within_one_environment() {
    let resolver = FontResolver::with_platform(Platform::Linux);
    let (a, _) = resolve(&resolver, "Liberation Sans", true, true);
    let (b, _) = resolve(&resolver, "Liberation Sans", true, true);
    assert_eq!(a.key(), b.key());
    assert_eq!(a.post_script_name, b.post_script_name);
    let rebuilt = FontResolver::with_platform(Platform::Linux);
    let (c, _) = resolve(&rebuilt, "Liberation Sans", true, true);
    assert_eq!(a.post_script_name, c.post_script_name);
}

// --- synthetic TTC: fontdb enumeration + (bytes, face_index) plumbing --------

/// Replaces every occurrence of `needle` with the same-length `repl`.
fn replace_bytes(haystack: &mut [u8], needle: &[u8], repl: &[u8]) {
    assert_eq!(needle.len(), repl.len());
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if haystack[i..i + needle.len()] == *needle {
            haystack[i..i + needle.len()].copy_from_slice(repl);
            i += needle.len();
        } else {
            i += 1;
        }
    }
}

/// UTF-16BE bytes of `s` (name-table Windows-platform string encoding).
fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// Renames a family inside a font program by same-length byte substitution in
/// both name-table encodings (Latin-1/Mac-Roman and UTF-16BE).
fn rename_family(font: &[u8], from: &str, to: &str) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let mut out = font.to_vec();
    replace_bytes(&mut out, from.as_bytes(), to.as_bytes());
    replace_bytes(&mut out, &utf16be(from), &utf16be(to));
    out
}

/// Assembles a TrueType Collection: `ttcf` header + each font program
/// appended verbatim with its table-directory offsets rebased (sfnt table
/// offsets are absolute from the start of the file).
fn build_ttc(fonts: &[&[u8]]) -> Vec<u8> {
    let header_len = 12 + 4 * fonts.len();
    let mut offsets = Vec::with_capacity(fonts.len());
    let mut cursor = header_len;
    for font in fonts {
        offsets.push(u32::try_from(cursor).expect("fixture fits in u32"));
        cursor += font.len();
    }
    let mut out = Vec::with_capacity(cursor);
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&0x0001_0000_u32.to_be_bytes()); // version 1.0
    out.extend_from_slice(&u32::try_from(fonts.len()).expect("few fonts").to_be_bytes());
    for offset in &offsets {
        out.extend_from_slice(&offset.to_be_bytes());
    }
    for (font, base) in fonts.iter().zip(&offsets) {
        let mut blob = font.to_vec();
        let num_tables = usize::from(u16::from_be_bytes([blob[4], blob[5]]));
        for record in 0..num_tables {
            let pos = 12 + record * 16 + 8; // offset field of this table record
            let old = u32::from_be_bytes([blob[pos], blob[pos + 1], blob[pos + 2], blob[pos + 3]]);
            blob[pos..pos + 4].copy_from_slice(&(old + base).to_be_bytes());
        }
        out.extend_from_slice(&blob);
    }
    out
}

#[test]
fn ttc_faces_enumerate_with_distinct_indices_and_resolve_by_family() {
    // Two renamed Liberation programs → one synthetic collection (renaming
    // avoids colliding with the always-bundled Liberation families).
    let sans = rename_family(
        liberation_face(LiberationFamily::Sans, false, false),
        "Liberation Sans",
        "TypesetTest AAA",
    );
    let serif = rename_family(
        liberation_face(LiberationFamily::Serif, false, false),
        "Liberation Serif",
        "TypesetTest BBBB",
    );
    let ttc = build_ttc(&[&sans, &serif]);

    let mut resolver = FontResolver::with_platform(Platform::MacOs);
    let before = resolver.face_count();
    resolver.add_font_data(ttc.clone());
    assert_eq!(
        resolver.face_count(),
        before + 2,
        "both TTC faces enumerate"
    );

    let (first, warnings) = resolve(&resolver, "TypesetTest AAA", false, false);
    assert_eq!(first.family, "TypesetTest AAA");
    assert_eq!(first.index, 0);
    assert!(warnings.is_empty());

    // Folded lookup works against collection faces too.
    let (second, warnings) = resolve(&resolver, "typesettest  BBBB", false, false);
    assert_eq!(second.family, "TypesetTest BBBB");
    assert_eq!(second.index, 1, "second collection face carries index 1");
    assert!(warnings.is_empty());

    // Coverage checks parse the face at its collection index…
    assert!(resolver.has_glyph(&second, 'A'));
    assert!(!resolver.has_glyph(&second, '中'));
    // …and face_data returns the WHOLE collection (parse at `.index`, TS-3).
    let data = resolver.face_data(&second).expect("collection bytes");
    assert_eq!(data.len(), ttc.len());
    assert_eq!(&data[0..4], b"ttcf");
}
