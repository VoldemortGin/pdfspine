//! `META-*` — `/Info` write + XMP metadata round-trips (PRD §8.9).

mod common;

use common::{open, save_reopen, MultiPage};

use pdf_core::object::Name;
use pdf_core::{DocumentStore, Object};
use pdf_edit::metadata::{get_xml_metadata, set_metadata, set_xml_metadata};

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Reads an `/Info` key as a decoded string from the reopened document.
fn info_value(doc: &DocumentStore, pdf_key: &str) -> Option<String> {
    let info_ref = doc.effective_trailer_ref("Info")?;
    let info = doc.resolve(info_ref).ok()?;
    let s = info.as_dict()?.get(&Name::new(pdf_key))?.as_string()?;
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units))
    } else {
        Some(bytes.iter().map(|&b| b as char).collect())
    }
}

/// `META-INFO-001`: a doc with no `/Info` gets one; reopen reads it back.
#[test]
fn meta_info_001_creates_info() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    assert!(doc.effective_trailer_ref("Info").is_none());
    set_metadata(&doc, &fields(&[("title", "My Title"), ("author", "Jane")])).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(info_value(&re, "Title").as_deref(), Some("My Title"));
    assert_eq!(info_value(&re, "Author").as_deref(), Some("Jane"));
}

/// `META-INFO-002`: updates an existing `/Info`.
#[test]
fn meta_info_002_updates_existing() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(&doc, &fields(&[("title", "First")])).unwrap();
    set_metadata(&doc, &fields(&[("title", "Second")])).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(info_value(&re, "Title").as_deref(), Some("Second"));
}

/// `META-INFO-003`: all standard keys round-trip.
#[test]
fn meta_info_003_all_keys() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(
        &doc,
        &fields(&[
            ("title", "T"),
            ("author", "A"),
            ("subject", "S"),
            ("keywords", "K"),
            ("creator", "C"),
            ("producer", "P"),
            ("creationDate", "D:20260615120000"),
            ("modDate", "D:20260616120000"),
        ]),
    )
    .unwrap();
    let re = save_reopen(&doc);
    assert_eq!(info_value(&re, "Title").as_deref(), Some("T"));
    assert_eq!(info_value(&re, "Subject").as_deref(), Some("S"));
    assert_eq!(info_value(&re, "Keywords").as_deref(), Some("K"));
    assert_eq!(info_value(&re, "Creator").as_deref(), Some("C"));
    assert_eq!(info_value(&re, "Producer").as_deref(), Some("P"));
    assert_eq!(
        info_value(&re, "CreationDate").as_deref(),
        Some("D:20260615120000")
    );
}

/// `META-INFO-004`: clearing a key (empty value) removes it.
#[test]
fn meta_info_004_clear_key() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(&doc, &fields(&[("title", "T"), ("author", "A")])).unwrap();
    set_metadata(&doc, &fields(&[("author", "")])).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(info_value(&re, "Title").as_deref(), Some("T"));
    assert_eq!(info_value(&re, "Author"), None);
}

/// `META-INFO-005`: non-ASCII title is UTF-16BE, read back equal.
#[test]
fn meta_info_005_unicode() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(&doc, &fields(&[("title", "日本語タイトル")])).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(info_value(&re, "Title").as_deref(), Some("日本語タイトル"));
    // Verify the on-disk encoding is UTF-16BE (BOM present).
    let info_ref = re.effective_trailer_ref("Info").unwrap();
    let info = re.resolve(info_ref).unwrap();
    let bytes = info
        .as_dict()
        .unwrap()
        .get(&Name::new("Title"))
        .and_then(Object::as_string)
        .unwrap()
        .as_bytes()
        .to_vec();
    assert_eq!(&bytes[..2], &[0xFE, 0xFF]);
}

/// `META-INFO-006`: a PDF date string round-trips verbatim.
#[test]
fn meta_info_006_date_string() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(&doc, &fields(&[("creationDate", "D:20260615093000")])).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(
        info_value(&re, "CreationDate").as_deref(),
        Some("D:20260615093000")
    );
}

/// `META-XMP-001`: create an XMP `/Metadata` stream; read it back.
#[test]
fn meta_xmp_001_create() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    assert_eq!(get_xml_metadata(&doc), None);
    let xmp = "<?xpacket begin='\u{feff}'?><x:xmpmeta>hi</x:xmpmeta>";
    set_xml_metadata(&doc, xmp).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(get_xml_metadata(&re).as_deref(), Some(xmp));
}

/// `META-XMP-002`: replace an existing `/Metadata` stream.
#[test]
fn meta_xmp_002_replace() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_xml_metadata(&doc, "<a>one</a>").unwrap();
    set_xml_metadata(&doc, "<a>two</a>").unwrap();
    let re = save_reopen(&doc);
    assert_eq!(get_xml_metadata(&re).as_deref(), Some("<a>two</a>"));
}

/// `META-XMP-003`: no `/Metadata` → None.
#[test]
fn meta_xmp_003_absent() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    assert_eq!(get_xml_metadata(&doc), None);
}
