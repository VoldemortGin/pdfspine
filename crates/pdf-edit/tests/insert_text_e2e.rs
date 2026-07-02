//! M4a text insertion — `INSERT-TEXT-*` / `INSERT-TTF-*` / `INSERT-TEXTBOX-*`
//! (PRD §8.8, §8.5.2).
//!
//! Reparse oracle throughout: build a blank page → insert → full save → reopen
//! → run the M2 interpreter (`page_glyphs`) and assert the inserted text is
//! extractable at the expected position / color, that existing content survives,
//! and that an embedded TTF maps back through its `/ToUnicode`.

mod common;

use common::{
    blank_page, open, page_content_bytes, page_fonts, page_glyphs, save_bytes, save_reopen,
    MultiPage,
};

use pdf_core::geom::Point;
use pdf_edit::{insert_text, insert_textbox, Align, Color, TextOptions};

/// Concatenated Unicode of all glyphs on page `index` (after reopen).
fn extracted(doc: &pdf_core::DocumentStore, index: usize) -> String {
    page_glyphs(doc, index)
        .iter()
        .map(|g| g.unicode.as_str())
        .collect::<String>()
}

/// Runs `search_for(needle)` over the page at `index` (via the M2 search
/// pipeline) and returns the number of hit quads.
fn search_hits(doc: &pdf_core::DocumentStore, index: usize, needle: &str) -> usize {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let res = pdf_text::interpret_page(doc, &page);
    let mb = pdf_core::pagetree::mediabox(doc, leaf);
    let rot = pdf_core::pagetree::rotation(doc, leaf);
    let tp = pdf_text::textpage_from_glyphs(&res.glyphs, &res.images, mb, rot);
    pdf_text::search(&tp, needle, pdf_text::SearchOptions::default()).len()
}

// === INSERT-TEXT-* ========================================================

/// `INSERT-TEXT-001`: insert_text on a blank page → save → reopen → the text is
/// extractable.
#[test]
fn insert_text_001_roundtrip() {
    let doc = open(&blank_page(612, 792));
    let opts = TextOptions::default();
    let n = insert_text(&doc, 0, Point::new(72.0, 72.0), "Hello", &opts).unwrap();
    assert_eq!(n, 1);

    let re = save_reopen(&doc);
    assert!(
        extracted(&re, 0).contains("Hello"),
        "got {:?}",
        extracted(&re, 0)
    );
    // The inserted text is also findable by `search_for` (the strongest oracle).
    assert_eq!(
        search_hits(&re, 0, "Hello"),
        1,
        "search_for did not find it"
    );
}

/// `INSERT-TEXT-002`: the inserted baseline origin lands at the PyMuPDF
/// top-left `point` converted to PDF user space (y-up). For a 792-high page and
/// a top-left point (72, 100), the first glyph origin should be near
/// (72, 792-100 = 692) in user space.
#[test]
fn insert_text_002_origin_position() {
    let doc = open(&blank_page(612, 792));
    insert_text(
        &doc,
        0,
        Point::new(72.0, 100.0),
        "X",
        &TextOptions::default(),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let g = &page_glyphs(&re, 0)[0];
    assert!((g.origin.x - 72.0).abs() < 1.0, "x={}", g.origin.x);
    assert!((g.origin.y - 692.0).abs() < 1.0, "y={}", g.origin.y);
}

/// `INSERT-TEXT-003`: multi-line text splits on `\n`; each line drops by the
/// leading, so the second line's baseline is below the first.
#[test]
fn insert_text_003_multiline() {
    let doc = open(&blank_page(612, 792));
    let n = insert_text(
        &doc,
        0,
        Point::new(50.0, 50.0),
        "AAA\nBBB",
        &TextOptions::default(),
    )
    .unwrap();
    assert_eq!(n, 2);
    let re = save_reopen(&doc);
    let glyphs = page_glyphs(&re, 0);
    let text: String = glyphs.iter().map(|g| g.unicode.as_str()).collect();
    assert!(text.contains("AAA") && text.contains("BBB"), "got {text:?}");

    // The 'A' glyphs sit above the 'B' glyphs (larger user-space y).
    let a_y = glyphs.iter().find(|g| g.unicode == "A").unwrap().origin.y;
    let b_y = glyphs.iter().find(|g| g.unicode == "B").unwrap().origin.y;
    assert!(a_y > b_y, "first line should be higher: a={a_y} b={b_y}");
}

/// `INSERT-TEXT-004`: a non-default fill color is reflected on the extracted
/// glyph span color.
#[test]
fn insert_text_004_color() {
    let doc = open(&blank_page(612, 792));
    let opts = TextOptions {
        color: Color::new(1.0, 0.0, 0.0), // red
        ..Default::default()
    };
    insert_text(&doc, 0, Point::new(72.0, 72.0), "R", &opts).unwrap();
    let re = save_reopen(&doc);
    let g = &page_glyphs(&re, 0)[0];
    assert_eq!(g.color, 0xFF_0000, "expected red, got {:#08x}", g.color);
}

/// `INSERT-TEXT-005`: a Base-14 `/Type1 /BaseFont /Helvetica` resource is
/// registered (no embedding — no FontFile of any kind).
#[test]
fn insert_text_005_base14_resource() {
    let doc = open(&blank_page(612, 792));
    insert_text(
        &doc,
        0,
        Point::new(72.0, 72.0),
        "Hi",
        &TextOptions::default(),
    )
    .unwrap();
    let re = save_reopen(&doc);
    // The content selects a font and shows text.
    let content = String::from_utf8_lossy(&page_content_bytes(&re, 0)).to_string();
    assert!(content.contains("Tf"), "no Tf in {content}");
    assert!(content.contains("Tj"), "no Tj in {content}");

    // The registered font is a /Type1 /Helvetica with no embedded FontFile.
    let fonts = page_fonts(&re, 0);
    let helv = fonts
        .iter()
        .find(|d| {
            d.get(&pdf_core::Name::new("BaseFont"))
                .and_then(pdf_core::Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"Helvetica")
        })
        .expect("no Base-14 /Type1 /Helvetica font registered");
    assert!(
        helv.get(&pdf_core::Name::new("Subtype"))
            .and_then(pdf_core::Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"Type1"),
        "Base-14 font is not /Type1"
    );
    assert!(
        helv.get(&pdf_core::Name::new("FontDescriptor")).is_none(),
        "Base-14 font must not embed a FontDescriptor/FontFile"
    );
}

/// `INSERT-TEXT-006`: inserting onto a page with existing content leaves the
/// existing text extractable.
#[test]
fn insert_text_006_preserves_existing() {
    // MultiPage page 0 shows "AAA".
    let doc = open(&MultiPage::new(&["AAA"]).build());
    insert_text(
        &doc,
        0,
        Point::new(100.0, 100.0),
        "ZZZ",
        &TextOptions::default(),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let text = extracted(&re, 0);
    assert!(text.contains("AAA"), "existing lost: {text:?}");
    assert!(text.contains("ZZZ"), "inserted missing: {text:?}");
}

/// `INSERT-TEXT-007`: parentheses / backslashes are escaped and round-trip.
#[test]
fn insert_text_007_escaping() {
    let doc = open(&blank_page(612, 792));
    insert_text(
        &doc,
        0,
        Point::new(72.0, 72.0),
        "a(b)c\\d",
        &TextOptions::default(),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let text = extracted(&re, 0);
    assert!(text.contains("a(b)c\\d"), "got {text:?}");
}

/// `INSERT-TEXT-008`: font aliases register the right BaseFont (`tiro` → Times,
/// `cour` → Courier).
#[test]
fn insert_text_008_aliases() {
    for (alias, base) in [("tiro", "Times-Roman"), ("cour", "Courier")] {
        let doc = open(&blank_page(612, 792));
        let opts = TextOptions {
            fontname: alias,
            ..Default::default()
        };
        insert_text(&doc, 0, Point::new(72.0, 72.0), "T", &opts).unwrap();
        let re = save_reopen(&doc);
        let found = page_fonts(&re, 0).iter().any(|d| {
            d.get(&pdf_core::Name::new("BaseFont"))
                .and_then(pdf_core::Object::as_name)
                .is_some_and(|n| n.as_bytes() == base.as_bytes())
        });
        assert!(found, "alias {alias} did not register {base}");
    }
}

// === INSERT-TTF-* =========================================================

/// `INSERT-TTF-001` / `INSERT-TTF-004`: embedding a user TTF emits a `/Type0`
/// Identity-H font with a `/CIDFontType2` descendant + a FontFile2 holding a
/// usage-based glyph **subset** (PRD-NEXT §10 TS-3): `/Length1` equals the
/// embedded program length, the subset parses standalone, and the original
/// glyph IDs are preserved so the Identity-H codes stay valid.
#[test]
fn insert_ttf_001_type0_subset_embed() {
    let ttf = common::testfont::build_test_ttf(&['H', 'e', 'l', 'o'], 500);
    let doc = open(&blank_page(612, 792));
    let opts = TextOptions {
        fontfile: Some(&ttf),
        fontname: "OxipdfTest",
        ..Default::default()
    };
    insert_text(&doc, 0, Point::new(72.0, 72.0), "Hello", &opts).unwrap();
    let re = save_reopen(&doc);

    let mut saw_type0 = false;
    let mut saw_cidfont = false;
    let mut saw_identity_h = false;
    let mut fontfile_len1: Option<i64> = None;
    let mut fontfile_program: Option<Vec<u8>> = None;
    for num in re.xref().object_numbers() {
        if let Ok(o) = re.get_object(num, 0) {
            if let Some(d) = o.as_dict() {
                let sub = d
                    .get(&pdf_core::Name::new("Subtype"))
                    .and_then(pdf_core::Object::as_name);
                if sub.is_some_and(|n| n.as_bytes() == b"Type0") {
                    saw_type0 = true;
                    if d.get(&pdf_core::Name::new("Encoding"))
                        .and_then(pdf_core::Object::as_name)
                        .is_some_and(|n| n.as_bytes() == b"Identity-H")
                    {
                        saw_identity_h = true;
                    }
                }
                if sub.is_some_and(|n| n.as_bytes() == b"CIDFontType2") {
                    saw_cidfont = true;
                }
            }
            if let Some(s) = o.as_stream() {
                if let Some(l1) = s
                    .dict
                    .get(&pdf_core::Name::new("Length1"))
                    .and_then(pdf_core::Object::as_i64)
                {
                    fontfile_len1 = Some(l1);
                    fontfile_program = re.decode_stream(s).and_then(|o| o.into_decoded()).ok();
                }
            }
        }
    }
    assert!(saw_type0, "no /Type0 font");
    assert!(saw_identity_h, "no Identity-H encoding");
    assert!(saw_cidfont, "no /CIDFontType2 descendant");
    let program = fontfile_program.expect("no FontFile2 stream");
    assert_eq!(
        fontfile_len1,
        Some(program.len() as i64),
        "FontFile2 /Length1 must equal the embedded (subset) program length"
    );
    // The subset parses standalone and preserves the source glyph IDs, so the
    // Identity-H 2-byte codes in the content stream stay valid.
    let orig = ttf_parser::Face::parse(&ttf, 0).expect("source font parses");
    let sub = ttf_parser::Face::parse(&program, 0).expect("subset font parses");
    for ch in ['H', 'e', 'l', 'o'] {
        assert_eq!(
            sub.glyph_index(ch),
            orig.glyph_index(ch),
            "glyph id for {ch:?} must be preserved in the subset"
        );
    }
}

/// `INSERT-TTF-002`: the inserted TTF text is extractable via the written
/// `/ToUnicode` CMap (the round-trip oracle).
#[test]
fn insert_ttf_002_tounicode_roundtrip() {
    let ttf = common::testfont::build_test_ttf(&['H', 'e', 'l', 'o'], 500);
    let doc = open(&blank_page(612, 792));
    let opts = TextOptions {
        fontfile: Some(&ttf),
        fontname: "OxipdfTest",
        ..Default::default()
    };
    insert_text(&doc, 0, Point::new(72.0, 72.0), "Hello", &opts).unwrap();
    let re = save_reopen(&doc);
    let text = extracted(&re, 0);
    assert!(
        text.contains("Hello"),
        "ToUnicode round-trip failed: {text:?}"
    );
}

/// `INSERT-TTF-005`: a non-font byte blob is rejected with a typed error and
/// never panics.
#[test]
fn insert_ttf_005_bad_font_rejected() {
    let doc = open(&blank_page(612, 792));
    let junk = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let opts = TextOptions {
        fontfile: Some(&junk),
        ..Default::default()
    };
    let err = insert_text(&doc, 0, Point::new(72.0, 72.0), "x", &opts);
    assert!(err.is_err(), "bad font should be rejected");
}

// === INSERT-TEXTBOX-* =====================================================

/// `INSERT-TEXTBOX-001`: text wraps within the rect width; all words remain
/// extractable.
#[test]
fn insert_textbox_001_wrap() {
    let doc = open(&blank_page(612, 792));
    let rect = pdf_core::geom::Rect::new(72.0, 72.0, 200.0, 400.0);
    let text = "alpha beta gamma delta epsilon zeta eta theta";
    insert_textbox(&doc, 0, rect, text, &TextOptions::default()).unwrap();
    let re = save_reopen(&doc);
    let extracted = extracted(&re, 0);
    for word in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        assert!(extracted.contains(word), "missing {word} in {extracted:?}");
    }
}

/// `INSERT-TEXTBOX-002`: center vs right alignment shift the line origin (the
/// first glyph x differs from the left-aligned origin).
#[test]
fn insert_textbox_002_align() {
    let rect = pdf_core::geom::Rect::new(72.0, 72.0, 400.0, 200.0);
    let first_x = |align: Align| -> f64 {
        let doc = open(&blank_page(612, 792));
        let opts = TextOptions {
            align,
            ..Default::default()
        };
        insert_textbox(&doc, 0, rect, "word", &opts).unwrap();
        let re = save_reopen(&doc);
        page_glyphs(&re, 0)[0].origin.x
    };
    let left = first_x(Align::Left);
    let center = first_x(Align::Center);
    let right = first_x(Align::Right);
    assert!(center > left + 1.0, "center not shifted: {left} {center}");
    assert!(right > center + 1.0, "right not shifted: {center} {right}");
}

/// `INSERT-TEXTBOX-003`: a small amount of text in a tall box returns a positive
/// unused height.
#[test]
fn insert_textbox_003_unused_height_positive() {
    let doc = open(&blank_page(612, 792));
    let rect = pdf_core::geom::Rect::new(72.0, 72.0, 400.0, 600.0);
    let left = insert_textbox(&doc, 0, rect, "one line", &TextOptions::default()).unwrap();
    assert!(left > 0.0, "expected positive unused height, got {left}");
}

/// `INSERT-TEXTBOX-004`: more text than fits returns a negative overflow value.
#[test]
fn insert_textbox_004_overflow_negative() {
    let doc = open(&blank_page(612, 792));
    // A short box (height ~30pt at 11pt → ~2 lines) with many words.
    let rect = pdf_core::geom::Rect::new(72.0, 72.0, 160.0, 100.0);
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda \
                mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
    let left = insert_textbox(&doc, 0, rect, text, &TextOptions::default()).unwrap();
    assert!(left < 0.0, "expected negative overflow, got {left}");
}

/// `INSERT-TEXTBOX-005`: an explicit `\n` forces a line break (two lines from
/// short text that would otherwise fit on one).
#[test]
fn insert_textbox_005_explicit_break() {
    let doc = open(&blank_page(612, 792));
    let rect = pdf_core::geom::Rect::new(72.0, 72.0, 400.0, 400.0);
    insert_textbox(&doc, 0, rect, "AAA\nBBB", &TextOptions::default()).unwrap();
    let re = save_reopen(&doc);
    let glyphs = page_glyphs(&re, 0);
    let a_y = glyphs.iter().find(|g| g.unicode == "A").unwrap().origin.y;
    let b_y = glyphs.iter().find(|g| g.unicode == "B").unwrap().origin.y;
    assert!(a_y > b_y, "explicit break should put BBB below AAA");
}

// === INSERT-PROP-* (text path) ============================================

/// `INSERT-PROP-002`: inserting onto a page whose `/Contents` is an array
/// (multi-stream) works — existing content is wrapped and both survive.
#[test]
fn insert_prop_002_contents_array() {
    // MultiPage has a single `/Contents` ref; to get an array, insert twice and
    // confirm both the original marker and both inserts extract. The first
    // insert converts `/Contents` to an array; the second appends to the array.
    let doc = open(&MultiPage::new(&["AAA"]).build());
    insert_text(
        &doc,
        0,
        Point::new(50.0, 50.0),
        "BBB",
        &TextOptions::default(),
    )
    .unwrap();
    insert_text(
        &doc,
        0,
        Point::new(50.0, 200.0),
        "CCC",
        &TextOptions::default(),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let text = extracted(&re, 0);
    for marker in ["AAA", "BBB", "CCC"] {
        assert!(text.contains(marker), "missing {marker} in {text:?}");
    }
}

/// `INSERT-PROP-003`: a saved file with inserted content reparses clean — a
/// reopen with the full M2 pipeline succeeds and still sees the page + text.
#[test]
fn insert_prop_003_reparses_clean() {
    let doc = open(&blank_page(612, 792));
    insert_text(
        &doc,
        0,
        Point::new(72.0, 72.0),
        "clean",
        &TextOptions::default(),
    )
    .unwrap();
    let bytes = save_bytes(&doc);
    // Reparse from scratch must succeed and still see the page + text.
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    assert!(extracted(&re, 0).contains("clean"));
}

/// `INSERT-PROP-004`: repeated insertions on the same page accumulate and each
/// gets a distinct font resource name (no collision).
#[test]
fn insert_prop_004_repeated_accumulate() {
    let doc = open(&blank_page(612, 792));
    insert_text(
        &doc,
        0,
        Point::new(50.0, 50.0),
        "one",
        &TextOptions::default(),
    )
    .unwrap();
    insert_text(
        &doc,
        0,
        Point::new(50.0, 100.0),
        "two",
        &TextOptions::default(),
    )
    .unwrap();
    insert_text(
        &doc,
        0,
        Point::new(50.0, 150.0),
        "three",
        &TextOptions::default(),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let text = extracted(&re, 0);
    for marker in ["one", "two", "three"] {
        assert!(text.contains(marker), "missing {marker} in {text:?}");
    }
    // Three font registrations under /Resources /Font (F0, F1, F2).
    assert!(
        page_fonts(&re, 0).len() >= 3,
        "expected >=3 font resources, got {}",
        page_fonts(&re, 0).len()
    );
}
