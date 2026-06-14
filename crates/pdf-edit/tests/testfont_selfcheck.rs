//! Self-check that the synthetic TrueType generator produces a font which
//! `ttf_parser` accepts and reads back exactly as configured.

mod common;

use common::testfont::build_test_ttf;
use ttf_parser::{Face, GlyphId};

#[test]
fn synthetic_ttf_parses_and_reads_back() {
    let chars = ['A', 'B', 'H', 'e', 'l', 'o', ' '];
    let advance: u16 = 500;
    let data = build_test_ttf(&chars, advance);

    let face = Face::parse(&data, 0).expect("ttf_parser should parse the synthetic font");

    // 7 mapped chars + .notdef.
    assert_eq!(face.number_of_glyphs(), 8);

    assert_eq!(face.units_per_em(), 1000);

    // cmap: chars map to glyph IDs 1.. in input order.
    assert_eq!(face.glyph_index('A'), Some(GlyphId(1)));
    assert_eq!(face.glyph_index('B'), Some(GlyphId(2)));
    assert_eq!(face.glyph_index('H'), Some(GlyphId(3)));
    assert_eq!(face.glyph_index('e'), Some(GlyphId(4)));
    assert_eq!(face.glyph_index('l'), Some(GlyphId(5)));
    assert_eq!(face.glyph_index('o'), Some(GlyphId(6)));
    assert_eq!(face.glyph_index(' '), Some(GlyphId(7)));
    // Unmapped char -> no glyph.
    assert_eq!(face.glyph_index('Z'), None);

    // hmtx advance.
    assert_eq!(face.glyph_hor_advance(GlyphId(1)), Some(advance));
    assert_eq!(face.glyph_hor_advance(GlyphId(3)), Some(advance));

    // hhea metrics.
    assert!(face.ascender() > 0);
    assert!(face.descender() < 0);

    // head bounding box.
    let bbox = face.global_bounding_box();
    assert!(bbox.y_max > bbox.y_min);

    // name table: a PostScript name must be readable.
    let ps_name = face
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .and_then(|n| n.to_string())
        .expect("PostScript name should be present");
    assert_eq!(ps_name, "OxipdfTest");
}
