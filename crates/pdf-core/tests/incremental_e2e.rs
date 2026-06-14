//! M3b incremental save — `INCR-*` (PRD §8.7, §8.7.1, §12 M3 exit gate).
//!
//! The correctness oracle is our own reparse plus the byte-exactness assertion
//! `out[..orig.len()] == orig`; an optional `qpdf --check` runs only when `qpdf`
//! is on `PATH`.

mod common;

use common::{dict, doc_with_signature_marker, last_startxref, name_obj, rref, simple_doc};

use pdf_core::object::Name;
use pdf_core::{DocumentStore, Limits, ObjRef, Object, OnRepaired, SaveOptions, XrefStyle};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

/// Builds a *repaired* document: prepend junk before the header so the parse is
/// tainted (`header_offset != 0` ⇒ `parse_was_repaired`), while still opening.
fn repaired_doc_bytes() -> Vec<u8> {
    let mut bytes = b"%junk-leading-bytes\n".to_vec();
    bytes.extend_from_slice(&simple_doc());
    bytes
}

// --- INCR-BYTES-* : byte exactness ---------------------------------------

#[test]
fn incr_bytes_001_update_prefix_exact() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(42))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    assert!(out.len() >= orig.len());
    assert_eq!(&out[..orig.len()], &orig[..], "prefix must be byte-exact");
}

#[test]
fn incr_bytes_002_add_prefix_exact() {
    let orig = simple_doc();
    let doc = open(&orig);
    let r = doc.add_object(Object::Integer(7)).unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    assert_eq!(&out[..orig.len()], &orig[..]);
    // The added object reopens.
    let re = open(&out);
    assert_eq!(re.resolve(r).unwrap().as_i64(), Some(7));
}

#[test]
fn incr_bytes_003_delete_prefix_exact() {
    let orig = simple_doc();
    let doc = open(&orig);
    // Delete an unreferenced-after-edit object (the font is fine to free for
    // the byte-exactness check; reachability isn't asserted here).
    doc.delete_object(ObjRef::new(5, 0)).unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    assert_eq!(&out[..orig.len()], &orig[..]);
    // The deleted object is now free on reopen: a direct fetch reports it missing,
    // and a lenient `resolve` of a reference to it yields Null.
    let re = open(&out);
    assert!(matches!(
        re.get_object(5, 0),
        Err(pdf_core::Error::MissingObject { .. })
    ));
    assert!(re.resolve(ObjRef::new(5, 0)).unwrap().is_null());
}

#[test]
fn incr_bytes_004_noop_prefix_exact() {
    let orig = simple_doc();
    let doc = open(&orig);
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    assert_eq!(&out[..orig.len()], &orig[..]);
    // Still a valid, reopenable doc.
    let re = open(&out);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
}

#[test]
fn incr_bytes_005_minimal_append() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(1))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    let appended = out.len() - orig.len();
    // A single tiny edit + a one-subsection xref + trailer is small. A full save
    // would re-emit every object; bound the append well under the original size.
    assert!(
        appended < orig.len(),
        "incremental append ({appended}) should be smaller than the original ({})",
        orig.len()
    );
}

#[test]
fn incr_bytes_006_xref_stream_prefix_exact() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(9))
        .unwrap();
    let out = doc
        .save_incremental(&SaveOptions::default().with_xref_style(XrefStyle::Stream))
        .unwrap();
    assert_eq!(&out[..orig.len()], &orig[..]);
    let re = open(&out);
    assert_eq!(re.resolve(ObjRef::new(5, 0)).unwrap().as_i64(), Some(9));
}

// --- INCR-PREV-* : /Prev chain + multi-revision --------------------------

#[test]
fn incr_prev_001_prev_equals_prior_startxref() {
    let orig = simple_doc();
    let prior = last_startxref(&orig);
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(1))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    // The appended trailer must carry /Prev == prior startxref.
    let needle = format!("/Prev {prior}");
    let hay = String::from_utf8_lossy(&out[orig.len()..]).into_owned();
    assert!(
        hay.contains(&needle),
        "appended trailer should contain `{needle}`; got:\n{hay}"
    );
}

#[test]
fn incr_prev_002_updated_resolves_new_value() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(777))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    let re = open(&out);
    assert_eq!(
        re.resolve(ObjRef::new(5, 0)).unwrap().as_i64(),
        Some(777),
        "newest revision wins on reopen"
    );
}

#[test]
fn incr_prev_003_new_startxref_points_at_section() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(1))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    let new_sx = last_startxref(&out);
    assert!(
        new_sx >= orig.len(),
        "the new startxref must point into the appended region"
    );
    // What it points at is a new cross-reference section (`xref` keyword for the
    // table style).
    assert_eq!(&out[new_sx..new_sx + 4], b"xref");
}

#[test]
fn incr_prev_004_trailer_keys() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(1))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    let re = open(&out);
    let trailer = re.trailer();
    assert!(trailer.get(&Name::new("Root")).is_some(), "/Root preserved");
    let id = trailer.get(&Name::new("ID")).unwrap().as_array().unwrap();
    assert_eq!(id.len(), 2, "/ID is a 2-element array");
}

#[test]
fn incr_prev_005_xref_stream_prev() {
    let orig = simple_doc();
    let prior = last_startxref(&orig);
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(5))
        .unwrap();
    let out = doc
        .save_incremental(&SaveOptions::default().with_xref_style(XrefStyle::Stream))
        .unwrap();
    let hay = String::from_utf8_lossy(&out[orig.len()..]).into_owned();
    assert!(
        hay.contains(&format!("/Prev {prior}")),
        "xref-stream trailer should carry /Prev == prior startxref"
    );
    let re = open(&out);
    assert_eq!(re.resolve(ObjRef::new(5, 0)).unwrap().as_i64(), Some(5));
}

#[test]
fn incr_prev_006_added_object_numbered_from_max() {
    let orig = simple_doc();
    let doc = open(&orig);
    // Original max object number is 5 → next is 6.
    let r = doc
        .add_object(Object::Dictionary(dict([("Type", name_obj("ExtGState"))])))
        .unwrap();
    assert_eq!(r.num, 6, "new object continues from existing max");
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    let re = open(&out);
    let obj = re.resolve(r).unwrap();
    assert!(obj.as_dict().is_some());
}

#[test]
fn incr_prev_007_two_successive_increments() {
    let orig = simple_doc();
    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(1))
        .unwrap();
    let rev1 = doc.save_incremental(&SaveOptions::default()).unwrap();
    let sx1 = last_startxref(&rev1);

    // Reopen revision 1, edit again, increment again.
    let doc2 = open(&rev1);
    doc2.update_object(ObjRef::new(5, 0), Object::Integer(2))
        .unwrap();
    let rev2 = doc2.save_incremental(&SaveOptions::default()).unwrap();

    assert_eq!(&rev2[..rev1.len()], &rev1[..], "rev2 prefixes rev1 exactly");
    let hay = String::from_utf8_lossy(&rev2[rev1.len()..]).into_owned();
    assert!(
        hay.contains(&format!("/Prev {sx1}")),
        "rev2 /Prev must chain to rev1's startxref"
    );
    let re = open(&rev2);
    assert_eq!(re.resolve(ObjRef::new(5, 0)).unwrap().as_i64(), Some(2));
}

// --- INCR-CLEAN-* : clean-parse precondition -----------------------------

#[test]
fn incr_clean_001_clean_can_and_succeeds() {
    let doc = open(&simple_doc());
    assert!(doc.can_save_incrementally());
    assert!(doc.save_incremental(&SaveOptions::default()).is_ok());
}

#[test]
fn incr_clean_002_repaired_cannot() {
    let doc = open(&repaired_doc_bytes());
    assert!(doc.parse_was_repaired());
    assert!(!doc.can_save_incrementally());
}

#[test]
fn incr_clean_003_repaired_reject_is_typed_error() {
    let doc = open(&repaired_doc_bytes());
    let err = doc
        .save_incremental(&SaveOptions::default().with_on_repaired(OnRepaired::Reject))
        .unwrap_err();
    assert_eq!(
        err.kind(),
        "incremental-requires-clean-parse",
        "repaired + Reject ⇒ typed precondition error"
    );
}

#[test]
fn incr_clean_004_repaired_upgrade_full_save() {
    let doc = open(&repaired_doc_bytes());
    let out = doc
        .save_incremental(&SaveOptions::default().with_on_repaired(OnRepaired::Upgrade))
        .unwrap();
    // The upgrade path produced a *full* save (not an append onto the junk
    // prefix), so it begins at a fresh `%PDF-` header and reopens cleanly.
    assert_eq!(&out[..5], b"%PDF-");
    let re = open(&out);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
}

// --- INCR-SIG-* : signature preservation ---------------------------------

#[test]
fn incr_sig_001_signed_range_unchanged() {
    let orig = doc_with_signature_marker();
    let doc = open(&orig);
    // Edit a *different* object incrementally (object 5, the font).
    doc.update_object(ObjRef::new(5, 0), Object::Integer(1))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    // The whole original file (the notional signed range) is untouched.
    assert_eq!(&out[..orig.len()], &orig[..]);
}

#[test]
fn incr_sig_002_byte_range_prefix_identical() {
    let orig = doc_with_signature_marker();
    // The /ByteRange-covered prefix here is the entire original file. Locate the
    // signature dict's /Contents placeholder bytes in the original and confirm
    // they are byte-identical in the incremental output.
    let marker = [0xABu8; 8];
    // The hex string serializes to uppercase ASCII "ABAB...".
    let needle = b"ABABABABABABABAB";
    let pos = orig
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("signature /Contents present in original");
    let _ = marker;

    let doc = open(&orig);
    doc.update_object(ObjRef::new(5, 0), Object::Integer(2))
        .unwrap();
    let out = doc.save_incremental(&SaveOptions::default()).unwrap();
    assert_eq!(
        &out[pos..pos + needle.len()],
        needle,
        "the signed /Contents bytes survive the incremental edit verbatim"
    );
}

/// A reachable-object value-stays-old check for the older revision: the original
/// `startxref` region still parses the OLD value. (Multi-revision history.)
#[test]
fn incr_prev_002b_old_revision_value_via_original() {
    // Open the ORIGINAL bytes (revision 0) and confirm the old value resolves.
    let orig = simple_doc();
    let doc0 = open(&orig);
    let old = doc0.resolve(ObjRef::new(5, 0)).unwrap();
    assert!(old.as_dict().is_some(), "rev 0 sees the font dict");

    let _ = rref; // keep the import used across helper variants
}
