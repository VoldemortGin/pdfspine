//! `MD-NAV-*` — link annotations (`/Link` + URI / GoTo) and the `/Outlines`
//! bookmark tree, read back through `pdf-edit`'s own readers.

mod common;

use pdf_core::object::{Name, ObjRef, Object};
use pdf_core::{DocumentStore, Limits};
use pdf_edit::{get_links, get_outline_xrefs, get_toc, Link, LinkKind, TocEntry};
use pdf_markdown::{markdown_to_pdf, Options};

use common::{raw, render, render_with};

const A4_H: f64 = 841.92;
const EPS: f64 = 0.02;

fn store(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("output should reopen")
}

fn links(doc: &DocumentStore, page: usize) -> Vec<Link> {
    get_links(doc, page)
}

/// The `/XYZ` `top` of the `/Dest` array on object `num` (an annotation or
/// an outline item).
fn dest_top(doc: &DocumentStore, num: u32) -> f64 {
    let obj = doc
        .resolve(ObjRef::new(num, 0))
        .expect("object should resolve");
    let dict = obj.as_dict().expect("destination holder is a dict");
    let dest = dict
        .get(&Name::new("Dest"))
        .and_then(Object::as_array)
        .expect("/Dest array present");
    assert_eq!(dest.len(), 5, "explicit /XYZ destination has 5 elements");
    assert_eq!(dest[1].as_name().map(Name::as_bytes), Some(&b"XYZ"[..]));
    assert!(matches!(dest[2], Object::Null), "left is null (keep)");
    assert!(matches!(dest[4], Object::Null), "zoom is null (keep)");
    dest[3].as_f64().expect("top is numeric")
}

fn toc_rows(doc: &DocumentStore) -> Vec<(i32, String, i32)> {
    get_toc(doc)
        .into_iter()
        .map(|TocEntry { level, title, page }| (level, title, page))
        .collect()
}

fn uri(link: &Link) -> &str {
    match &link.kind {
        LinkKind::Uri(u) => u,
        other => panic!("expected a URI link, got {other:?}"),
    }
}

// --- MD-NAV-001: external links → /A /URI with a rect over the text --------

#[test]
fn external_link_gets_uri_annotation_over_its_text() {
    let bytes = render("See [the docs](https://example.com/a?b=1&c=2) now.");
    let doc = store(&bytes);
    let ls = links(&doc, 0);
    assert_eq!(ls.len(), 1);
    assert_eq!(uri(&ls[0]), "https://example.com/a?b=1&c=2");
    assert_eq!(ls[0].border, [0.0, 0.0, 0.0], "no visible border");
    let r = ls[0].from;
    // First body line: line-box top = the 72 pt margin, baseline 0.8 em below.
    assert!((r.y1 - (A4_H - 72.0)).abs() < EPS, "top edge {r:?}");
    assert!(
        (r.y0 - (A4_H - 72.0 - 11.0 * 0.8 - 11.0 * 0.25)).abs() < EPS,
        "bottom {r:?}"
    );
    assert!(r.x0 > 72.0 + 15.0, "starts after 'See ' {r:?}");
    assert!(r.x1 < 300.0 && r.x1 > r.x0 + 30.0, "spans 'the docs' {r:?}");
}

#[test]
fn autolinks_and_reference_links_are_annotated() {
    let bytes =
        render("Visit <https://x.test/p> or <me@x.test> or [ref][r].\n\n[r]: https://ref.test\n");
    let doc = store(&bytes);
    let ls = links(&doc, 0);
    let uris: Vec<&str> = ls.iter().map(uri).collect();
    assert_eq!(
        uris,
        ["https://x.test/p", "mailto:me@x.test", "https://ref.test"]
    );
    // Left-to-right on one line.
    assert!(ls[0].from.x1 <= ls[1].from.x0 + EPS && ls[1].from.x1 <= ls[2].from.x0 + EPS);
}

// --- MD-NAV-002: #anchor links → GoTo to the heading's page + top ------------

#[test]
fn anchor_link_jumps_to_target_heading_page_and_top() {
    let filler = "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\n".repeat(80);
    let md =
        format!("# One\n\nJump to [chapter two](#chapter-two).\n\n{filler}# Chapter Two\n\nEnd.");
    let bytes = render(&md);
    let doc = store(&bytes);
    assert!(pdf_core::pagetree::page_count(&doc) >= 2);
    let ls = links(&doc, 0);
    assert_eq!(ls.len(), 1);
    let target_page = match ls[0].kind {
        LinkKind::Goto(p) => p,
        ref other => panic!("expected GoTo, got {other:?}"),
    };
    let toc = toc_rows(&doc);
    assert_eq!(toc[1].1, "Chapter Two");
    assert_eq!(target_page, toc[1].2, "link lands on the heading's page");
    assert!(target_page >= 1);
    // The annotation and the outline item share the heading's /XYZ top.
    let link_top = dest_top(&doc, ls[0].xref);
    let outline_top = dest_top(&doc, get_outline_xrefs(&doc)[1]);
    assert!((link_top - outline_top).abs() < EPS);
    assert!(
        link_top > 72.0 && link_top <= A4_H - 72.0 + EPS,
        "top inside the content area"
    );
}

#[test]
fn anchors_resolve_slugs_ids_percent_encoding_and_case() {
    let md = "# Intro\n\n# Intro\n\n# Custom Title {#my-id}\n\n# 中文\n\n\
              [a](#intro-1) [b](#my-id) [c](#Intro) [d](#%E4%B8%AD%E6%96%87) [e](#Custom%20Title)\n";
    let bytes = render(md);
    let doc = store(&bytes);
    let ls = links(&doc, 0);
    // `e` targets the natural slug of a heading whose explicit `{#my-id}`
    // replaced it — unresolved, so no annotation.
    assert_eq!(ls.len(), 4);
    let heading_tops: Vec<f64> = get_outline_xrefs(&doc)
        .iter()
        .map(|x| dest_top(&doc, *x))
        .collect();
    assert_eq!(heading_tops.len(), 4);
    assert!(
        heading_tops.windows(2).all(|w| w[0] > w[1]),
        "headings descend the page"
    );
    let hit = |i: usize| dest_top(&doc, ls[i].xref);
    assert!(
        (hit(0) - heading_tops[1]).abs() < EPS,
        "#intro-1 → second Intro"
    );
    assert!(
        (hit(1) - heading_tops[2]).abs() < EPS,
        "#my-id → explicit id"
    );
    assert!(
        (hit(2) - heading_tops[0]).abs() < EPS,
        "#Intro re-slugified → first Intro"
    );
    assert!(
        (hit(3) - heading_tops[3]).abs() < EPS,
        "percent-encoded CJK anchor"
    );
    // The `{#my-id}` attribute is not rendered as text.
    assert!(!common::full_text(&bytes).contains("{#my-id}"));
}

#[test]
fn unresolvable_anchor_and_empty_destination_get_no_annotation() {
    let bytes = render("# Real\n\n[nope](#missing) [empty]() [hash](#) plain");
    let doc = store(&bytes);
    assert!(links(&doc, 0).is_empty());
    assert_eq!(toc_rows(&doc).len(), 1);
}

// --- MD-NAV-003: heading hierarchy → /Outlines -------------------------------

#[test]
fn outline_mirrors_heading_hierarchy() {
    let bytes = render("# A\n\n## B\n\n### C\n\n## D\n\n# E\n\ntext");
    let doc = store(&bytes);
    assert_eq!(
        toc_rows(&doc),
        [
            (1, "A".to_string(), 0),
            (2, "B".to_string(), 0),
            (3, "C".to_string(), 0),
            (2, "D".to_string(), 0),
            (1, "E".to_string(), 0),
        ]
    );
    // Every item carries an explicit /XYZ destination with a numeric top.
    let tops: Vec<f64> = get_outline_xrefs(&doc)
        .iter()
        .map(|x| dest_top(&doc, *x))
        .collect();
    assert_eq!(tops.len(), 5);
    assert!(
        (tops[0] - (A4_H - 72.0)).abs() < EPS,
        "first heading sits at the top margin"
    );
    assert!(tops.windows(2).all(|w| w[0] > w[1]));
}

#[test]
fn outline_normalizes_level_jumps_and_flattens_titles() {
    let bytes =
        render("## Start\n\n#### Deep\n\n### Mid\n\n###### Deeper\n\n# **Bold** `code` 中文 end\n");
    let doc = store(&bytes);
    let rows = toc_rows(&doc);
    let levels: Vec<i32> = rows.iter().map(|r| r.0).collect();
    assert_eq!(levels, [1, 2, 2, 3, 1]);
    assert_eq!(
        rows[4].1, "Bold code 中文 end",
        "outline title keeps the real text"
    );
}

#[test]
fn headings_inside_quotes_and_lists_are_in_the_outline_and_anchorable() {
    let bytes = render("> ## Quoted\n\n- ### Listed\n- item\n\n[q](#quoted) [l](#listed)");
    let doc = store(&bytes);
    let rows = toc_rows(&doc);
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].1.as_str()), (1, "Quoted"));
    assert_eq!((rows[1].0, rows[1].1.as_str()), (2, "Listed"));
    assert_eq!(links(&doc, 0).len(), 2);
}

// --- MD-NAV-004: geometry of link boxes -------------------------------------

#[test]
fn wrapped_link_yields_one_box_per_line() {
    let text = "a very long link text ".repeat(12);
    let bytes = render(&format!("[{text}](https://long.test)"));
    let doc = store(&bytes);
    let ls = links(&doc, 0);
    assert!(
        ls.len() >= 2,
        "expected one box per wrapped line, got {}",
        ls.len()
    );
    assert!(ls.iter().all(|l| uri(l) == "https://long.test"));
    assert!(
        ls.windows(2).all(|w| w[0].from.y0 > w[1].from.y0 + 1.0),
        "boxes descend line by line"
    );
    assert!(ls
        .iter()
        .all(|l| l.from.x0 >= 72.0 - EPS && l.from.x1 <= 595.32 - 72.0 + EPS));
}

#[test]
fn mixed_inline_styles_inside_one_link_share_one_box() {
    let bytes = render("[**bold** and `code` and *em*](https://mixed.test) tail");
    let doc = store(&bytes);
    let ls = links(&doc, 0);
    assert_eq!(ls.len(), 1, "one contiguous run → one annotation");
    assert_eq!(uri(&ls[0]), "https://mixed.test");
    let adjacent = render("[a](https://a.test)[b](https://b.test)");
    let doc = store(&adjacent);
    let ls = links(&doc, 0);
    assert_eq!(ls.len(), 2, "different destinations never merge");
}

#[test]
fn links_in_headings_and_table_cells_are_annotated() {
    let md =
        "# See the [spec](https://spec.test)\n\n| col |\n|---|\n| [cell](https://cell.test) |\n";
    let bytes = render(md);
    let doc = store(&bytes);
    let ls = links(&doc, 0);
    let uris: Vec<&str> = ls.iter().map(uri).collect();
    assert_eq!(uris, ["https://spec.test", "https://cell.test"]);
}

// --- MD-NAV-005: switches + byte stability -----------------------------------

#[test]
fn document_without_links_or_headings_is_byte_identical_with_navigation_off() {
    let md = "Just a paragraph with **bold** text.\n\n- a list\n\n| t |\n|---|\n| c |\n";
    let on = render(md);
    let mut opts = Options::default();
    opts.links = false;
    opts.toc = false;
    let off = render_with(md, &opts);
    assert_eq!(on, off);
    let text = raw(&on);
    assert!(!text.contains("/Annots") && !text.contains("/Outlines") && !text.contains("/Link"));
}

#[test]
fn links_and_toc_switches_are_independent() {
    let md = "# H\n\n[x](https://x.test) [h](#h)";
    let both = store(&render(md));
    assert_eq!(links(&both, 0).len(), 2);
    assert_eq!(toc_rows(&both).len(), 1);

    let mut opts = Options::default();
    opts.links = false;
    let no_links = store(&render_with(md, &opts));
    assert!(links(&no_links, 0).is_empty());
    assert_eq!(toc_rows(&no_links).len(), 1);

    let mut opts = Options::default();
    opts.toc = false;
    let no_toc = store(&render_with(md, &opts));
    assert_eq!(links(&no_toc, 0).len(), 2);
    assert!(toc_rows(&no_toc).is_empty());

    let mut opts = Options::default();
    opts.links = false;
    opts.toc = false;
    let none = raw(&render_with(md, &opts));
    assert!(!none.contains("/Annots") && !none.contains("/Outlines"));
}

#[test]
fn navigation_is_deterministic_and_never_panics_on_odd_input() {
    let md = "# A\n\n[x](#a) [y](https://y) <z@z.z>\n\n## A\n\n[w](#a-1) [%](#%) [q](#%zz%E4)";
    let a = markdown_to_pdf(md, &Options::default()).expect("render a");
    let b = markdown_to_pdf(md, &Options::default()).expect("render b");
    assert_eq!(a, b);
    let doc = store(&a);
    assert_eq!(
        links(&doc, 0).len(),
        4,
        "x, y, z, w resolve; % and %zz do not"
    );
}
