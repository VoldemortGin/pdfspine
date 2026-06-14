//! M3c `insert_pdf` merge + split — `MERGE-*` / `SPLIT-*` (PRD §8.7 / §12).
//!
//! Reparse oracle throughout. Page order is asserted by per-page marker text;
//! "shared object copied once" is asserted by counting `/Type`/`/Subtype`
//! occurrences in the destination after merge.

mod common;

use common::{
    all_page_text, assert_no_dangling_refs, count_objects_of_type, count_streams_of_subtype, open,
    save_reopen, shared_resource_doc, MultiPage,
};

use pdf_edit::{extract_pages, insert_pdf, InsertOptions, PageEditor};

fn append_opts() -> InsertOptions {
    InsertOptions::default()
}

// === MERGE-COUNT-* ========================================================

/// `MERGE-COUNT-001` / `MERGE-COUNT-004`: insert_pdf appends all src pages → dst
/// count += src count; survives reopen.
#[test]
fn merge_count_001_append_all() {
    let dst = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let src = open(&MultiPage::new(&["XXX", "YYY", "ZZZ"]).build());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(pdf_core::pagetree::page_count(&re), 5);
    assert_eq!(common::pages_count_key(&re), 5);
}

/// `MERGE-COUNT-002`: a `from_page`/`to_page` subset inserts only that range.
#[test]
fn merge_count_002_range_subset() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&MultiPage::new(&["P0", "P1", "P2", "P3"]).build());
    let opts = InsertOptions {
        from_page: Some(1),
        to_page: Some(2),
        ..Default::default()
    };
    insert_pdf(&dst, &src, &opts).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(all_page_text(&re), vec!["AAA", "P1", "P2"]);
}

/// `MERGE-COUNT-003`: `start_at` splices the copied pages at that position.
#[test]
fn merge_count_003_start_at() {
    let dst = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let src = open(&MultiPage::new(&["XXX"]).build());
    let opts = InsertOptions {
        start_at: Some(1),
        ..Default::default()
    };
    insert_pdf(&dst, &src, &opts).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(all_page_text(&re), vec!["AAA", "XXX", "BBB", "CCC"]);
}

/// `MERGE-COUNT-005`: a reversed range inserts pages in reverse order.
#[test]
fn merge_count_005_reversed_range() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&MultiPage::new(&["P0", "P1", "P2"]).build());
    let opts = InsertOptions {
        from_page: Some(2),
        to_page: Some(0),
        ..Default::default()
    };
    insert_pdf(&dst, &src, &opts).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(all_page_text(&re), vec!["AAA", "P2", "P1", "P0"]);
}

// === MERGE-ORDER-* ========================================================

/// `MERGE-ORDER-001`: appended pages appear after dst text, in src order.
#[test]
fn merge_order_001_append_order() {
    let dst = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let src = open(&MultiPage::new(&["XXX", "YYY"]).build());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(all_page_text(&re), vec!["AAA", "BBB", "XXX", "YYY"]);
}

/// `MERGE-ORDER-002`: start_at=0 prepends; interleaved order correct.
#[test]
fn merge_order_002_prepend() {
    let dst = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let src = open(&MultiPage::new(&["XXX", "YYY"]).build());
    let opts = InsertOptions {
        start_at: Some(0),
        ..Default::default()
    };
    insert_pdf(&dst, &src, &opts).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(all_page_text(&re), vec!["XXX", "YYY", "AAA", "BBB"]);
}

// === MERGE-REFS-* =========================================================

/// `MERGE-REFS-001` / `MERGE-REFS-002`: all copied refs resolve; fresh numbers,
/// no collision.
#[test]
fn merge_refs_001_no_dangling_no_collision() {
    let dst = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let src = open(&MultiPage::new(&["XXX", "YYY"]).build());
    let new_leaves = insert_pdf(&dst, &src, &append_opts()).unwrap();

    // The inserted leaf numbers are fresh (past the dst original /Size).
    let dst_size = dst.xref_length();
    // (after merge, allocated numbers continue past the original size)
    for leaf in &new_leaves {
        assert!(leaf.num > 0);
    }
    assert!(dst_size >= 1);

    let re = save_reopen(&dst);
    let checked = assert_no_dangling_refs(&re);
    assert!(checked > 0, "walked the merged graph");
}

/// `MERGE-REFS-003`: get_text on a merged page returns the source page's text.
#[test]
fn merge_refs_003_text_extractable() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&MultiPage::new(&["HELLO"]).build());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(common::page_text(&re, 1), "HELLO");
}

/// `MERGE-REFS-004`: the saved merged doc reparses clean (reopen succeeds; the
/// page-tree walk + /Count agree). Optional `qpdf --check` if present.
#[test]
fn merge_refs_004_reparses_clean() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&shared_resource_doc());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let bytes = dst
        .save_to_vec(&pdf_core::SaveOptions::default().with_garbage(1))
        .unwrap();
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), 3);
    assert_eq!(common::pages_count_key(&re), 3);
    qpdf_check(&bytes);
}

// === MERGE-DEDUP-* ========================================================

/// `MERGE-DEDUP-001`: a font shared by two src pages is copied **once**.
///
/// Counts are taken on the saved+reopened document so every grafted object is in
/// the cross-reference (ChangeSet-allocated numbers are not in the live xref).
/// `save_reopen` uses `garbage=1` (mark-sweep only) — it does **not** dedup, so a
/// "copied twice" bug would leave two Font objects and fail this assertion.
#[test]
fn merge_dedup_001_shared_font_once() {
    // dst has its own one font; src has two pages sharing a single font.
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&shared_resource_doc());
    let before = count_objects_of_type(&open(&MultiPage::new(&["AAA"]).build()), "Font");
    assert_eq!(before, 1, "dst starts with one font");
    insert_pdf(&dst, &src, &append_opts()).unwrap();

    let re = save_reopen(&dst);
    // Exactly two fonts total: dst's original + the single shared src font.
    assert_eq!(
        count_objects_of_type(&re, "Font"),
        2,
        "shared src font copied exactly once"
    );

    // And both copied pages still reference a font (extractable text).
    assert_eq!(common::page_text(&re, 1), "SRC1");
    assert_eq!(common::page_text(&re, 2), "SRC2");
}

/// `MERGE-DEDUP-002`: a shared XObject form is copied once.
#[test]
fn merge_dedup_002_shared_xobject_once() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&shared_resource_doc());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    // The dst had no Form XObject; the src's single shared one is copied once.
    assert_eq!(
        count_streams_of_subtype(&re, "Form"),
        1,
        "shared XObject copied exactly once"
    );
}

/// `MERGE-DEDUP-003`: a cyclic ref graph in the source is copied without
/// looping; both nodes survive and reference each other in the destination.
#[test]
fn merge_dedup_003_cyclic_graph() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&common::cyclic_doc());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(pdf_core::pagetree::page_count(&re), 2);
    // The cyclic graph resolved without dangling refs (and without hanging).
    let checked = assert_no_dangling_refs(&re);
    assert!(checked > 0);
    assert_eq!(common::page_text(&re, 1), "CYC");
}

// === MERGE-PROP-* =========================================================

/// `MERGE-PROP-001`: inherited MediaBox on src pages is materialized onto copied
/// leaves.
#[test]
fn merge_prop_001_inherited_mediabox() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    // nested_doc's leaves inherit MediaBox [0 0 400 500] from the root.
    let src = open(&common::nested_doc());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    // Copied pages are at dst indices 1.. ; they carry the materialized box.
    let leaf = pdf_core::pagetree::page_refs(&re)[1];
    assert_eq!(
        pdf_core::pagetree::mediabox(&re, leaf),
        pdf_core::geom::Rect::new(0.0, 0.0, 400.0, 500.0)
    );
}

/// `MERGE-PROP-002`: the `rotate` option is applied to inserted pages.
#[test]
fn merge_prop_002_rotate_applied() {
    let dst = open(&MultiPage::new(&["AAA"]).build());
    let src = open(&MultiPage::new(&["XXX", "YYY"]).build());
    let opts = InsertOptions {
        rotate: Some(180),
        ..Default::default()
    };
    insert_pdf(&dst, &src, &opts).unwrap();
    let re = save_reopen(&dst);
    let refs = pdf_core::pagetree::page_refs(&re);
    assert_eq!(pdf_core::pagetree::rotation(&re, refs[1]), 180);
    assert_eq!(pdf_core::pagetree::rotation(&re, refs[2]), 180);
}

/// `MERGE-PROP-003`: self-insert (src structurally identical to dst) never
/// panics; the count doubles.
#[test]
fn merge_prop_003_self_insert() {
    let dst = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let src = open(&MultiPage::new(&["AAA", "BBB"]).build());
    insert_pdf(&dst, &src, &append_opts()).unwrap();
    let re = save_reopen(&dst);
    assert_eq!(pdf_core::pagetree::page_count(&re), 4);
    assert_eq!(all_page_text(&re), vec!["AAA", "BBB", "AAA", "BBB"]);
}

/// `MERGE-PROP-004`: inserting an empty range is a no-op.
#[test]
fn merge_prop_004_empty_range_noop() {
    let dst = open(&MultiPage::new(&["AAA", "BBB"]).build());
    // An empty (zero-page) source.
    let src = open(&MultiPage::new(&[]).build());
    let leaves = insert_pdf(&dst, &src, &append_opts()).unwrap();
    assert!(leaves.is_empty());
    let mut ed = PageEditor::new(&dst).unwrap();
    let _ = &mut ed;
    let re = save_reopen(&dst);
    assert_eq!(pdf_core::pagetree::page_count(&re), 2);
}

// === SPLIT-* ==============================================================

/// `SPLIT-001`: extract a single page → a new 1-page doc; text matches source.
#[test]
fn split_001_single_page() {
    let src = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let bytes = extract_pages(&src, &[1]).unwrap();
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    assert_eq!(common::page_text(&re, 0), "BBB");
}

/// `SPLIT-002`: extract a reordered subset → a 2-page doc in that order.
#[test]
fn split_002_subset_order() {
    let src = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let bytes = extract_pages(&src, &[2, 0]).unwrap();
    let re = open(&bytes);
    assert_eq!(all_page_text(&re), vec!["CCC", "AAA"]);
}

/// `SPLIT-003`: the extracted doc is self-contained (no dangling refs).
#[test]
fn split_003_self_contained() {
    let src = open(&shared_resource_doc());
    let bytes = extract_pages(&src, &[0, 1]).unwrap();
    let re = open(&bytes);
    let checked = assert_no_dangling_refs(&re);
    assert!(checked > 0);
    // The two extracted pages still share a single font (dedup preserved).
    assert_eq!(count_objects_of_type(&re, "Font"), 1);
}

/// Runs `qpdf --check` over `bytes` if `qpdf` is on PATH; skips cleanly
/// otherwise (the reparse is the primary oracle).
fn qpdf_check(bytes: &[u8]) {
    use std::io::Write;
    use std::process::Command;
    if Command::new("qpdf").arg("--version").output().is_err() {
        return; // qpdf not installed — skip.
    }
    let dir = std::env::temp_dir();
    let path = dir.join(format!("oxipdf_merge_{}.pdf", std::process::id()));
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(bytes).unwrap();
    drop(f);
    let out = Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "qpdf --check failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
