//! M3c page-tree editing — `PAGEOPS-*` (PRD §8.7).
//!
//! Reparse oracle throughout: open → op → save → reopen → assert. Page *order*
//! is asserted by each page's identifiable marker text; `/Pages /Count` is
//! asserted both via the page-tree walk and via the raw `/Count` key.

mod common;

use common::{all_page_text, open, page_text, pages_count_key, save_reopen, MultiPage};

use pdf_core::geom::Rect;
use pdf_edit::PageEditor;

// === PAGEOPS-NEW-* ========================================================

/// `PAGEOPS-NEW-001` / `PAGEOPS-NEW-003`: new_page adds a page with the given
/// MediaBox + empty Contents; count grows; survives reopen.
#[test]
fn pageops_new_001_adds_blank_page() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    assert_eq!(ed.page_count(), 2);
    let leaf = ed.new_page(2, 200.0, 300.0).unwrap();
    assert_eq!(ed.page_count(), 3);

    // The new leaf has the requested MediaBox and an empty Contents stream.
    let page = pdf_core::pagetree::page_dict(&doc, leaf).unwrap();
    assert!(page.contains_key(&pdf_core::Name::new("Contents")));
    assert_eq!(
        pdf_core::pagetree::mediabox(&doc, leaf),
        Rect::new(0.0, 0.0, 200.0, 300.0)
    );

    // After reopen the count key + walk agree, and the new page is blank.
    let re = save_reopen(&doc);
    assert_eq!(pdf_core::pagetree::page_count(&re), 3);
    assert_eq!(pages_count_key(&re), 3);
    assert_eq!(page_text(&re, 2), "");
    assert_eq!(
        pdf_core::pagetree::mediabox(&re, pdf_core::pagetree::page_refs(&re)[2]),
        Rect::new(0.0, 0.0, 200.0, 300.0)
    );
}

/// `PAGEOPS-NEW-002`: a new page is inserted at the requested index; the
/// surrounding order is preserved.
#[test]
fn pageops_new_002_insert_position() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.new_page(1, 100.0, 100.0).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["AAA", "", "BBB", "CCC"]);
}

/// `PAGEOPS-NEW-004`: a new page past the end appends.
#[test]
fn pageops_new_004_append_past_end() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.new_page(99, 100.0, 100.0).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["AAA", ""]);
}

// === PAGEOPS-INSERT-* =====================================================

/// `PAGEOPS-INSERT-001` / `PAGEOPS-INSERT-002`: insert an existing leaf at an
/// index; count grows; `/Parent` repointed; content appears at that index.
#[test]
fn pageops_insert_001_splices_existing_leaf() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    // Create a fresh leaf (via new_page at the end), then move it by re-inserting.
    let mut ed = PageEditor::new(&doc).unwrap();
    let leaf = ed.new_page(2, 50.0, 50.0).unwrap();
    // Remove it from the tail and re-insert at the front.
    ed.delete_page(2).unwrap();
    ed.insert_page(0, leaf).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["", "AAA", "BBB"]);
    // `/Parent` of the inserted leaf points at the root /Pages after reopen.
    let leaf0 = pdf_core::pagetree::page_refs(&re)[0];
    let parent = pdf_core::pagetree::page_dict(&re, leaf0)
        .unwrap()
        .get(&pdf_core::Name::new("Parent"))
        .and_then(pdf_core::Object::as_reference);
    assert!(parent.is_some());
}

// === PAGEOPS-DELETE-* =====================================================

/// `PAGEOPS-DELETE-001` / `PAGEOPS-DELETE-003`: delete a page; count drops; the
/// right page is removed; survives reopen.
#[test]
fn pageops_delete_001_removes_page() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.delete_page(1).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(pdf_core::pagetree::page_count(&re), 2);
    assert_eq!(pages_count_key(&re), 2);
    assert_eq!(all_page_text(&re), vec!["AAA", "CCC"]);
}

/// `PAGEOPS-DELETE-002`: delete first / last / middle each yield the correct
/// remaining order.
#[test]
fn pageops_delete_002_first_last_middle() {
    for (del, expect) in [
        (0usize, vec!["BBB", "CCC", "DDD"]),
        (3, vec!["AAA", "BBB", "CCC"]),
        (1, vec!["AAA", "CCC", "DDD"]),
    ] {
        let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC", "DDD"]).build());
        let mut ed = PageEditor::new(&doc).unwrap();
        ed.delete_page(del).unwrap();
        let re = save_reopen(&doc);
        assert_eq!(all_page_text(&re), expect, "deleting index {del}");
    }
}

/// `PAGEOPS-DELETE-004`: an out-of-range delete is a typed error, no mutation.
#[test]
fn pageops_delete_004_out_of_range() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    assert!(ed.delete_page(5).is_err());
    assert_eq!(ed.page_count(), 1);
}

// === PAGEOPS-COPY-* =======================================================

/// `PAGEOPS-COPY-001` / `PAGEOPS-COPY-002`: copy_page duplicates a page; the
/// copy shows the same content; count grows.
#[test]
fn pageops_copy_001_duplicates_page() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.copy_page(0, 2).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(pdf_core::pagetree::page_count(&re), 3);
    assert_eq!(all_page_text(&re), vec!["AAA", "BBB", "AAA"]);
}

// === PAGEOPS-MOVE-* =======================================================

/// `PAGEOPS-MOVE-001` / `PAGEOPS-MOVE-003`: move a page forward/backward.
#[test]
fn pageops_move_001_reorders() {
    // Move page 0 ("AAA") to the end.
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.move_page(0, 2).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["BBB", "CCC", "AAA"]);

    // Move page 2 ("CCC") to the front.
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.move_page(2, 0).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["CCC", "AAA", "BBB"]);
}

/// `PAGEOPS-MOVE-002`: move with from == to is a no-op.
#[test]
fn pageops_move_002_noop() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.move_page(1, 1).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["AAA", "BBB"]);
}

// === PAGEOPS-SELECT-* =====================================================

/// `PAGEOPS-SELECT-001`: select a reordered subset.
#[test]
fn pageops_select_001_subset_reorder() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.select(&[2, 0]).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(pdf_core::pagetree::page_count(&re), 2);
    assert_eq!(all_page_text(&re), vec!["CCC", "AAA"]);
}

/// `PAGEOPS-SELECT-002`: duplicate indices duplicate the page.
#[test]
fn pageops_select_002_duplicates() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.select(&[0, 0, 1]).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["AAA", "AAA", "BBB"]);
}

/// `PAGEOPS-SELECT-003`: empty select yields a zero-page document.
#[test]
fn pageops_select_003_empty() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.select(&[]).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(pdf_core::pagetree::page_count(&re), 0);
    assert_eq!(pages_count_key(&re), 0);
}

/// `PAGEOPS-SELECT-004`: identity select preserves order + content.
#[test]
fn pageops_select_004_identity() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.select(&[0, 1, 2]).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["AAA", "BBB", "CCC"]);
}

/// `PAGEOPS-SELECT-005`: an out-of-range index is a typed error.
#[test]
fn pageops_select_005_out_of_range() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    assert!(ed.select(&[0, 9]).is_err());
}

// === PAGEOPS-BOX-* / PAGEOPS-ROTATE-* =====================================

/// `PAGEOPS-BOX-001`: set_mediabox is reflected after reopen.
#[test]
fn pageops_box_001_mediabox() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.set_mediabox(0, &Rect::new(10.0, 20.0, 110.0, 220.0))
        .unwrap();
    let re = save_reopen(&doc);
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    assert_eq!(
        pdf_core::pagetree::mediabox(&re, leaf),
        Rect::new(10.0, 20.0, 110.0, 220.0)
    );
}

/// `PAGEOPS-BOX-002`: set_cropbox is clipped to the media box; reflected after
/// reopen.
#[test]
fn pageops_box_002_cropbox_clipped() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    // MediaBox is [0 0 612 792]; a cropbox larger than it clips to it.
    ed.set_cropbox(0, &Rect::new(-50.0, -50.0, 5000.0, 5000.0))
        .unwrap();
    let re = save_reopen(&doc);
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    assert_eq!(
        pdf_core::pagetree::cropbox(&re, leaf),
        Rect::new(0.0, 0.0, 612.0, 792.0)
    );
}

/// `PAGEOPS-ROTATE-001`: set_rotation reflected after reopen.
#[test]
fn pageops_rotate_001_reflected() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let mut ed = PageEditor::new(&doc).unwrap();
    ed.set_rotation(0, 90).unwrap();
    let re = save_reopen(&doc);
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    assert_eq!(pdf_core::pagetree::rotation(&re, leaf), 90);
}

/// `PAGEOPS-ROTATE-002`: rotation normalized to {0,90,180,270}.
#[test]
fn pageops_rotate_002_normalized() {
    for (deg, expect) in [(450i64, 90i32), (-90, 270), (360, 0), (180, 180)] {
        let doc = open(&MultiPage::new(&["AAA"]).build());
        let mut ed = PageEditor::new(&doc).unwrap();
        ed.set_rotation(0, deg).unwrap();
        let re = save_reopen(&doc);
        let leaf = pdf_core::pagetree::page_refs(&re)[0];
        assert_eq!(
            pdf_core::pagetree::rotation(&re, leaf),
            expect,
            "rotation {deg}"
        );
    }
}
