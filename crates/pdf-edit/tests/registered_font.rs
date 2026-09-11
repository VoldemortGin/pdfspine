//! Registration preserves Unicode/CID identity and reads saved resource mappings.
mod common;

use common::{blank_page, open, page_text, save_bytes};
use pdf_core::geom::Point;
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_edit::registered_font::{insert_font, FontRegistrationOptions};
use pdf_edit::{EmbeddedFont, TextOptions};

#[test]
fn full_registration_preserves_program_and_all_cmap_characters() {
    let program = common::testfont::build_test_ttf(&['A', 'Ω', 'Ж'], 700);
    let font = EmbeddedFont::for_registration(&program).unwrap();
    let characters = font.registration_map().unwrap();
    assert_eq!(
        characters.values().copied().collect::<Vec<_>>(),
        vec!['A', 'Ω', 'Ж']
    );
    let doc = open(&blank_page(300, 200));
    insert_font(
        &doc,
        0,
        "Registered",
        &FontRegistrationOptions {
            program: Some(&program),
            ..Default::default()
        },
    )
    .unwrap();
    pdf_edit::insert_text(
        &doc,
        0,
        Point::new(40.0, 70.0),
        "ЖΩA",
        &TextOptions {
            fontname: "Registered",
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page_text(&open(&save_bytes(&doc)), 0), "ЖΩA");
}

#[test]
fn reopened_writer_uses_actual_pdf_codes_and_widths() {
    let program = common::testfont::build_test_ttf(&['A', 'Z'], 700);
    let doc = open(&blank_page(300, 200));
    let xref = insert_font(
        &doc,
        0,
        "Registered",
        &FontRegistrationOptions {
            program: Some(&program),
            ..Default::default()
        },
    )
    .unwrap();
    let reference = ObjRef::new(xref, 0);
    let mut font = doc.resolve(reference).unwrap().as_dict().unwrap().clone();
    let unicode = doc.add_object(Object::Stream(StreamObj::new_encoded(Dict::new(), b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfchar <0007> <005A> endbfchar".to_vec()))).unwrap();
    font.insert(Name::new("ToUnicode"), Object::Reference(unicode));
    let descendants = font
        .get(&Name::new("DescendantFonts"))
        .unwrap()
        .as_array()
        .unwrap();
    let cid_ref = match descendants[0] {
        Object::Reference(r) => r,
        _ => panic!("reference"),
    };
    let mut cid = doc.resolve(cid_ref).unwrap().as_dict().unwrap().clone();
    let mut gids = vec![0; 16];
    gids[15] = 1; // CID 7 -> GID 1 (A); the actual ToUnicode intentionally maps it to Z.
    let mapping = doc
        .add_object(Object::Stream(StreamObj::new_encoded(Dict::new(), gids)))
        .unwrap();
    cid.insert(Name::new("CIDToGIDMap"), Object::Reference(mapping));
    cid.insert(
        Name::new("W"),
        Object::Array(vec![
            Object::Integer(7),
            Object::Array(vec![Object::Integer(900)]),
        ]),
    );
    doc.update_object(cid_ref, Object::Dictionary(cid)).unwrap();
    doc.update_object(reference, Object::Dictionary(font))
        .unwrap();
    let reopened = open(&save_bytes(&doc));
    pdf_edit::insert_text(
        &reopened,
        0,
        Point::new(40.0, 70.0),
        "ZZ",
        &TextOptions {
            fontname: "Registered",
            fontsize: 20.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page_text(&reopened, 0), "ZZ");
    let page =
        pdf_core::pagetree::page_dict(&reopened, pdf_core::pagetree::page_refs(&reopened)[0])
            .unwrap();
    let content = pdf_text::ContentInterpreter::new(&reopened).run_page(&page);
    assert_eq!(content.glyphs.len(), 2);
    assert!((content.glyphs[1].origin.x - content.glyphs[0].origin.x - 18.0).abs() < 1e-6);
}

#[test]
fn collections_and_empty_cmap_fail_before_allocating() {
    let ttf = common::testfont::build_test_ttf(&['A'], 700);
    let ttc = common::testfont::build_test_ttc(&[ttf]);
    let doc = open(&blank_page(300, 200));
    let before = save_bytes(&doc);
    assert!(insert_font(
        &doc,
        0,
        "Rejected",
        &FontRegistrationOptions {
            program: Some(&ttc),
            ..Default::default()
        }
    )
    .is_err());
    assert_eq!(save_bytes(&doc), before);
    let empty = common::testfont::build_test_ttf(&[], 700);
    assert!(insert_font(
        &doc,
        0,
        "Rejected",
        &FontRegistrationOptions {
            program: Some(&empty),
            ..Default::default()
        }
    )
    .is_err());
    assert_eq!(save_bytes(&doc), before);
}
