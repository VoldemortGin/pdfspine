//! TS-3 (PRD-NEXT §10) — TTC face selection + usage-based TrueType glyph
//! subsetting for `EmbeddedFont`.
//!
//! CI-safe tests use the repo Liberation TTFs and synthetic TTCs; the
//! system-CJK acceptance gates (subset < 5% of a ≥ 10 MB TTC face, subset
//! raster pixel-identical to the full embed, text read-back) run against
//! `/System/Library/Fonts/Supplemental/Songti.ttc` and are platform-guarded
//! (skipped when the file is absent).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{blank_page, open, page_text, save_bytes};
use pdf_core::object::Object;
use pdf_core::{DocumentStore, Limits};
use pdf_edit::{fonts_in_collection, EmbeddedFont, PageContent};

const SANS: &[u8] = include_bytes!("../../pdf-fonts/fonts/liberation/LiberationSans-Regular.ttf");

const SONGTI: &str = "/System/Library/Fonts/Supplemental/Songti.ttc";
const HIRAGINO_GB: &str = "/System/Library/Fonts/Hiragino Sans GB.ttc";

/// The (Length1, decoded program) of the single FontFile2 stream in `doc`.
fn fontfile2_program(doc: &DocumentStore) -> (i64, Vec<u8>) {
    for num in doc.xref().object_numbers() {
        if let Ok(o) = doc.get_object(num, 0) {
            if let Some(s) = o.as_stream() {
                if let Some(l1) = s
                    .dict
                    .get(&pdf_core::Name::new("Length1"))
                    .and_then(pdf_core::Object::as_i64)
                {
                    let data = doc
                        .decode_stream(s)
                        .and_then(|d| d.into_decoded())
                        .expect("FontFile2 decodes");
                    return (l1, data);
                }
            }
        }
    }
    panic!("no FontFile2 stream found");
}

/// The `/BaseFont` of the `/Type0` font in `doc`.
fn type0_base_font(doc: &DocumentStore) -> String {
    for num in doc.xref().object_numbers() {
        if let Ok(o) = doc.get_object(num, 0) {
            if let Some(d) = o.as_dict() {
                let is_type0 = d
                    .get(&pdf_core::Name::new("Subtype"))
                    .and_then(pdf_core::Object::as_name)
                    .is_some_and(|n| n.as_bytes() == b"Type0");
                if is_type0 {
                    if let Some(n) = d
                        .get(&pdf_core::Name::new("BaseFont"))
                        .and_then(pdf_core::Object::as_name)
                    {
                        return String::from_utf8_lossy(n.as_bytes()).into_owned();
                    }
                }
            }
        }
    }
    panic!("no /Type0 font found");
}

/// Builds a one-page PDF showing `text` at 24 pt via `font` (Identity-H hex
/// codes, the same emission `insert_text` uses) and returns the saved bytes.
fn build_pdf_with_font(font: &EmbeddedFont, text: &str) -> Vec<u8> {
    let doc = open(&blank_page(612, 792));
    let mut used = BTreeMap::new();
    let mut hex = String::from("<");
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        used.insert(gid, ch);
        hex.push_str(&format!("{gid:04X}"));
    }
    hex.push('>');
    let font_ref = font.write_type0(&doc, &used).expect("write_type0");
    let pc = PageContent::new(&doc, 0).expect("page content");
    let name = pc
        .add_resource("Font", "F", Object::Reference(font_ref))
        .expect("font resource");
    let chunk = format!("q\nBT\n/{name} 24 Tf\n1 0 0 1 72 700 Tm\n{hex} Tj\nET\nQ\n");
    pc.append_content(chunk.as_bytes()).expect("append content");
    save_bytes(&doc)
}

/// Renders page 0 of `bytes` at 144 dpi and returns the RGB samples.
fn render_samples(bytes: &[u8]) -> Vec<u8> {
    let doc = Arc::new(
        DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("reopen for render"),
    );
    let leaf = pdf_core::pagetree::page_refs(&doc)[0];
    let page = pdf_core::Page::new(Arc::clone(&doc), 0, leaf);
    let opts = pdf_render::RenderOptions {
        dpi: Some(144),
        ..Default::default()
    };
    let pix = pdf_render::render_page(&doc, &page, &opts).expect("render");
    pix.samples().to_vec()
}

/// An outline recorder: two glyphs draw identically iff the recorded op
/// strings match (the composite-closure oracle).
#[derive(Default)]
struct OutlineRec(String);

impl ttf_parser::OutlineBuilder for OutlineRec {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push_str(&format!("M{x},{y};"));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push_str(&format!("L{x},{y};"));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push_str(&format!("Q{x1},{y1},{x},{y};"));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push_str(&format!("C{x1},{y1},{x2},{y2},{x},{y};"));
    }
    fn close(&mut self) {
        self.0.push_str("Z;");
    }
}

fn outline_of(face: &ttf_parser::Face, gid: u16) -> String {
    let mut rec = OutlineRec::default();
    face.outline_glyph(ttf_parser::GlyphId(gid), &mut rec);
    rec.0
}

// === TTC face selection ====================================================

/// `TS3-TTC-001`: `parse_indexed` selects the requested collection face;
/// `parse` stays the face-0 shorthand; out-of-bounds indices are typed errors.
#[test]
fn ts3_ttc_001_parse_indexed_selects_face() {
    let font_a = common::testfont::build_test_ttf(&['A'], 500);
    let font_b = common::testfont::build_test_ttf(&['B'], 700);
    let ttc = common::testfont::build_test_ttc(&[font_a.clone(), font_b]);

    assert_eq!(fonts_in_collection(&ttc), Some(2));
    assert_eq!(fonts_in_collection(&font_a), None, "plain TTF, not a TTC");

    let face0 = EmbeddedFont::parse_indexed(&ttc, 0).expect("face 0");
    let face1 = EmbeddedFont::parse_indexed(&ttc, 1).expect("face 1");
    assert_eq!(face0.face_index(), 0);
    assert_eq!(face1.face_index(), 1);
    // Face 0 maps 'A' (advance 500); face 1 maps 'B' (advance 700). The
    // glyph_id re-parse threads the face index too (the PRD :108 trap).
    assert_eq!(face0.glyph_id('A'), 1);
    assert_eq!(face0.glyph_id('B'), 0, ".notdef on the wrong face");
    assert_eq!(face1.glyph_id('B'), 1);
    assert_eq!(face1.glyph_id('A'), 0, ".notdef on the wrong face");
    assert!((face0.advance(1) - 500.0).abs() < 1e-9);
    assert!((face1.advance(1) - 700.0).abs() < 1e-9);

    // Backwards compatibility: `parse` == face 0.
    let default = EmbeddedFont::parse(&ttc).expect("parse defaults to face 0");
    assert_eq!(default.glyph_id('A'), 1);
    assert_eq!(default.face_index(), 0);

    // Out-of-bounds face indices are typed errors, never panics.
    assert!(EmbeddedFont::parse_indexed(&ttc, 2).is_err());
    assert!(EmbeddedFont::parse_indexed(&font_a, 1).is_err());
}

/// `TS3-TTC-002`: a subset embedded from a TTC face is a standalone TTF (not
/// a collection) that preserves the face's glyph IDs.
#[test]
fn ts3_ttc_002_subset_from_ttc_face_is_standalone() {
    let font_a = common::testfont::build_test_ttf(&['A'], 500);
    let font_b = common::testfont::build_test_ttf(&['B'], 700);
    let ttc = common::testfont::build_test_ttc(&[font_a, font_b]);

    let font = EmbeddedFont::parse_indexed(&ttc, 1).expect("face 1");
    let bytes = build_pdf_with_font(&font, "B");
    let re = open(&bytes);

    let (len1, program) = fontfile2_program(&re);
    assert_eq!(len1 as usize, program.len());
    assert!(program.len() < ttc.len(), "subset must shrink the TTC");
    assert_eq!(
        fonts_in_collection(&program),
        None,
        "the embedded subset must be a standalone sfnt, not a TTC"
    );
    let sub = ttf_parser::Face::parse(&program, 0).expect("subset parses");
    assert_eq!(
        sub.glyph_index('B').map(|g| g.0),
        Some(1),
        "face-1 glyph ids preserved"
    );
}

// === Subsetting (Liberation Sans — CI-safe) ================================

/// `TS3-SUB-001`: the default `write_type0` embeds a subset — smaller than
/// the source, `/Length1` exact, glyph IDs + advances preserved for the used
/// set, `/BaseFont` carrying the `ABCDEF+` subset tag, and the text still
/// extractable via ToUnicode (the read-back gate).
#[test]
fn ts3_sub_001_subset_preserves_gids_and_advances() {
    let text = "Hello, World!";
    let font = EmbeddedFont::parse(SANS).expect("liberation parses");
    let bytes = build_pdf_with_font(&font, text);
    let re = open(&bytes);

    let (len1, program) = fontfile2_program(&re);
    assert_eq!(len1 as usize, program.len(), "/Length1 must be exact");
    assert!(
        program.len() < SANS.len() / 4,
        "subset ({} B) must be far smaller than the source ({} B)",
        program.len(),
        SANS.len()
    );

    let base_font = type0_base_font(&re);
    assert_eq!(base_font.len(), 7 + font.base_name().len());
    assert_eq!(&base_font[6..7], "+", "subset tag required: {base_font}");
    assert!(base_font[..6].chars().all(|c| c.is_ascii_uppercase()));
    assert!(base_font.ends_with(font.base_name()));

    let orig = ttf_parser::Face::parse(SANS, 0).expect("source parses");
    let sub = ttf_parser::Face::parse(&program, 0).expect("subset parses");
    assert!(sub.number_of_glyphs() <= orig.number_of_glyphs());
    for ch in text.chars() {
        let gid = orig.glyph_index(ch).expect("source maps char");
        assert_eq!(sub.glyph_index(ch), Some(gid), "gid preserved for {ch:?}");
        assert_eq!(
            sub.glyph_hor_advance(gid),
            orig.glyph_hor_advance(gid),
            "advance preserved for {ch:?}"
        );
    }

    // Read-back oracle: the M2 interpreter recovers the text via ToUnicode.
    assert!(
        page_text(&re, 0).contains(text),
        "subset text must round-trip: {:?}",
        page_text(&re, 0)
    );
}

/// `TS3-SUB-002`: composite glyphs pull their component glyphs into the
/// subset recursively — the accented glyph draws the identical outline from
/// the subset (the classic subsetter failure mode).
#[test]
fn ts3_sub_002_composite_closure() {
    let font = EmbeddedFont::parse(SANS).expect("liberation parses");
    // 'é' / 'Å' are composites (base letter + accent) in Liberation Sans.
    let text = "éÅ";
    let bytes = build_pdf_with_font(&font, text);
    let re = open(&bytes);
    let (_, program) = fontfile2_program(&re);

    let orig = ttf_parser::Face::parse(SANS, 0).expect("source parses");
    let sub = ttf_parser::Face::parse(&program, 0).expect("subset parses");
    for ch in text.chars() {
        let gid = orig.glyph_index(ch).expect("source maps char").0;
        let reference = outline_of(&orig, gid);
        assert!(!reference.is_empty(), "{ch:?} must have an outline");
        assert_eq!(
            outline_of(&sub, gid),
            reference,
            "{ch:?} must draw identically from the subset (composite closure)"
        );
    }
}

/// `TS3-SUB-003`: the raster of the subset embed is pixel-identical to the
/// full embed (same glyph bytes, same gids, same widths).
#[test]
fn ts3_sub_003_subset_raster_matches_full_embed() {
    let text = "Pixel parity!";
    let subset_font = EmbeddedFont::parse(SANS).expect("parse");
    let mut full_font = EmbeddedFont::parse(SANS).expect("parse");
    full_font.set_full_embed(true);

    let subset_pdf = build_pdf_with_font(&subset_font, text);
    let full_pdf = build_pdf_with_font(&full_font, text);
    assert_eq!(
        render_samples(&subset_pdf),
        render_samples(&full_pdf),
        "subset raster must be pixel-identical to the full embed"
    );
}

/// `TS3-SUB-004`: the debug flag restores the whole-program embed, untagged.
#[test]
fn ts3_sub_004_full_embed_debug_flag() {
    let mut font = EmbeddedFont::parse(SANS).expect("parse");
    assert!(!font.full_embed());
    font.set_full_embed(true);
    assert!(font.full_embed());

    let bytes = build_pdf_with_font(&font, "debug");
    let re = open(&bytes);
    let (len1, program) = fontfile2_program(&re);
    assert_eq!(
        len1 as usize,
        SANS.len(),
        "full embed writes the whole program"
    );
    assert_eq!(program.len(), SANS.len());
    assert_eq!(
        type0_base_font(&re),
        font.base_name(),
        "no subset tag on a full embed"
    );
}

/// `TS3-SUB-005`: subsetting is deterministic — same font + same usage ⇒
/// byte-identical PDFs (the pdf-markdown determinism contract must survive).
#[test]
fn ts3_sub_005_deterministic() {
    let font = EmbeddedFont::parse(SANS).expect("parse");
    let a = build_pdf_with_font(&font, "determinism");
    let b = build_pdf_with_font(&font, "determinism");
    assert_eq!(a, b, "same input must produce byte-identical output");
}

// === System-CJK acceptance gates (platform-guarded, PRD TS-3) ==============

/// `TS3-GATE-001`: on a ≥ 10 MB system TTC face (Songti.ttc, 8 TrueType
/// faces), embedding ≤ 100 glyphs yields a FontFile2 **< 5 % of the source
/// size**, the subset raster is **pixel-identical** to the full embed, and
/// the text reads back exactly (extraction oracle; the fitz oracle runs in
/// `python/tests/test_fontfile_subset.py`).
#[test]
fn ts3_gate_001_songti_ttc_subset() {
    let Ok(data) = std::fs::read(SONGTI) else {
        eprintln!("skipping: {SONGTI} not available on this platform");
        return;
    };
    assert!(data.len() > 10 << 20, "gate needs a >= 10 MB source TTC");
    let faces = fonts_in_collection(&data).expect("Songti.ttc is a collection");
    assert!(faces >= 2);
    for i in 0..faces {
        EmbeddedFont::parse_indexed(&data, i).expect("every TTC face parses");
    }

    let text = "永和九年，岁在癸丑，暮春之初，会于会稽山阴之兰亭。";
    assert!(text.chars().count() <= 100);
    let subset_font = EmbeddedFont::parse_indexed(&data, 0).expect("face 0");
    let mut full_font = EmbeddedFont::parse_indexed(&data, 0).expect("face 0");
    full_font.set_full_embed(true);

    // Gate: subset embed < 5 % of the source collection.
    let subset_pdf = build_pdf_with_font(&subset_font, text);
    let re = open(&subset_pdf);
    let (len1, program) = fontfile2_program(&re);
    assert_eq!(len1 as usize, program.len());
    assert!(
        program.len() * 20 < data.len(),
        "subset ({} B) must be < 5% of the source TTC ({} B)",
        program.len(),
        data.len()
    );
    eprintln!(
        "TS3-GATE-001: source {} B, subset FontFile2 {} B ({:.3}%)",
        data.len(),
        program.len(),
        program.len() as f64 * 100.0 / data.len() as f64
    );

    // Gate: pixel-identical to the full embed under the repo render stack.
    let full_pdf = build_pdf_with_font(&full_font, text);
    assert!(subset_pdf.len() < full_pdf.len() / 10);
    assert_eq!(
        render_samples(&subset_pdf),
        render_samples(&full_pdf),
        "subset raster must be pixel-identical to the full embed"
    );

    // Gate: exact text read-back via ToUnicode.
    assert!(
        page_text(&re, 0).contains(text),
        "CJK subset text must round-trip: {:?}",
        page_text(&re, 0)
    );
}

/// `TS3-GATE-002`: a CFF-flavored OpenType collection (Hiragino Sans GB —
/// `OTTO` faces, no `glyf`) degrades to the documented v1 full embed, still
/// extractable via ToUnicode.
#[test]
fn ts3_gate_002_cff_flavored_falls_back_to_full_embed() {
    let Ok(data) = std::fs::read(HIRAGINO_GB) else {
        eprintln!("skipping: {HIRAGINO_GB} not available on this platform");
        return;
    };
    let font = EmbeddedFont::parse_indexed(&data, 0).expect("face 0");
    let text = "你好";
    let bytes = build_pdf_with_font(&font, text);
    let re = open(&bytes);
    let (len1, _) = fontfile2_program(&re);
    assert_eq!(
        len1 as usize,
        font.program_len(),
        "CFF-flavored face must fall back to the whole-program embed (v1)"
    );
    assert_eq!(type0_base_font(&re), font.base_name(), "no subset tag");
    assert!(
        page_text(&re, 0).contains(text),
        "fallback text must round-trip"
    );
}
