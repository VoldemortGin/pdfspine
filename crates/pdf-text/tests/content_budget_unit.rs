//! Per-page content work budget (`Limits::max_page_content_ops` /
//! `max_page_content_items`): Form XObject fan-out — each level re-running a
//! shared form 10× — must be cut off by the budget instead of growing 10× per
//! level, while a page the budget covers is interpreted unchanged.

mod common;

use common::{dict, name_obj, raw_stream, rref, winansi_type1, Pdf};
use pdf_core::{Dict, DocumentStore, Limits, Object};
use pdf_text::{interpret_page, interpret_page_render, InterpretResult};

/// Tokens of the leaf form's content (`BT /F1 12 Tf 72 700 Td (x) Tj ET`).
const LEAF_TOKENS: u64 = 10;
/// Tokens of each intermediate form's content (`/N Do` × 10).
const LEVEL_TOKENS: u64 = 20;
/// Tokens of each page's content (`/X0 Do`).
const PAGE_TOKENS: u64 = 2;

/// A document of `pages` pages, each drawing form 10, where forms 10.. form a
/// `levels`-deep chain whose every form calls the next one 10 times, ending in
/// a leaf that shows one glyph: 10^levels glyphs per page.
fn fan_doc(levels: u32, pages: u32, limits: Limits) -> (DocumentStore, Vec<Dict>) {
    let leaf = 10 + levels;
    let mut pdf = Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                (
                    "Kids",
                    Object::Array((0..pages).map(|p| rref(100 + p, 0)).collect()),
                ),
                ("Count", Object::Integer(i64::from(pages))),
            ])),
        )
        .obj(4, 0, winansi_type1("Helvetica", 32, &[500; 96]))
        .obj(5, 0, raw_stream([], b"/X0 Do"));
    let mut page_dicts = Vec::new();
    for p in 0..pages {
        let page = dict([
            ("Type", name_obj("Page")),
            ("Parent", rref(2, 0)),
            (
                "Resources",
                Object::Dictionary(dict([
                    ("Font", Object::Dictionary(dict([("F1", rref(4, 0))]))),
                    ("XObject", Object::Dictionary(dict([("X0", rref(10, 0))]))),
                ])),
            ),
            ("Contents", rref(5, 0)),
        ]);
        page_dicts.push(page.clone());
        pdf = pdf.obj(100 + p, 0, Object::Dictionary(page));
    }
    let form = |resources: Dict, body: &[u8]| {
        raw_stream(
            [
                ("Type", name_obj("XObject")),
                ("Subtype", name_obj("Form")),
                (
                    "BBox",
                    Object::Array(
                        vec![0, 0, 612, 792]
                            .into_iter()
                            .map(Object::Integer)
                            .collect(),
                    ),
                ),
                ("Resources", Object::Dictionary(resources)),
            ],
            body,
        )
    };
    for n in 10..leaf {
        let res = dict([("XObject", Object::Dictionary(dict([("N", rref(n + 1, 0))])))]);
        pdf = pdf.obj(n, 0, form(res, "/N Do ".repeat(10).as_bytes()));
    }
    let res = dict([("Font", Object::Dictionary(dict([("F1", rref(4, 0))])))]);
    pdf = pdf.obj(leaf, 0, form(res, b"BT /F1 12 Tf 72 700 Td (x) Tj ET"));
    let doc = DocumentStore::from_bytes(pdf.root(1, 0).build(), limits).expect("open fan doc");
    (doc, page_dicts)
}

/// The exact token + invocation cost of one page of a `levels`-deep fan.
fn page_cost(levels: u32) -> u64 {
    let mut total = PAGE_TOKENS;
    let mut invocations = 1u64;
    for _ in 0..levels {
        total += invocations * (Limits::CONTENT_INVOCATION_COST + LEVEL_TOKENS);
        invocations *= 10;
    }
    total + invocations * (Limits::CONTENT_INVOCATION_COST + LEAF_TOKENS)
}

fn run(doc: &DocumentStore, page: &Dict) -> InterpretResult {
    interpret_page(doc, page)
}

#[test]
fn deep_fan_out_is_cut_off_by_the_op_budget() {
    let budget = 100_000;
    for levels in [5, 6, 7] {
        let limits = Limits::default().with_max_page_content_ops(budget);
        let (doc, pages) = fan_doc(levels, 1, limits);
        let res = run(&doc, &pages[0]);
        assert!(res.truncated, "levels {levels}");
        // Every shown glyph cost at least one leaf invocation.
        let max_glyphs = budget / (Limits::CONTENT_INVOCATION_COST + LEAF_TOKENS);
        assert!(
            res.glyphs.len() as u64 <= max_glyphs,
            "{}",
            res.glyphs.len()
        );
        assert!(!res.glyphs.is_empty());
        let ops = interpret_page_render(&doc, &pages[0]);
        assert!(ops.len() as u64 <= budget, "{}", ops.len());
    }
}

#[test]
fn a_budget_that_covers_the_page_leaves_it_unchanged() {
    let exact = page_cost(2);
    let (full_doc, pages) = fan_doc(2, 1, Limits::default());
    let full = run(&full_doc, &pages[0]);
    assert!(!full.truncated);
    assert_eq!(full.glyphs.len(), 100);

    let (doc, pages) = fan_doc(2, 1, Limits::default().with_max_page_content_ops(exact));
    let res = run(&doc, &pages[0]);
    assert!(!res.truncated);
    assert_eq!(res, full);
    assert_eq!(
        format!("{:?}", interpret_page_render(&doc, &pages[0])),
        format!("{:?}", interpret_page_render(&full_doc, &pages[0]))
    );

    // One token short: truncated (the last leaf runs only a prefix of its
    // content); half the cost: about half the glyphs. Either way the output is
    // a prefix of the full page.
    for (budget, max_glyphs) in [(exact - 1, 100), (exact / 2, 99)] {
        let (doc, pages) = fan_doc(2, 1, Limits::default().with_max_page_content_ops(budget));
        let res = run(&doc, &pages[0]);
        assert!(res.truncated, "{budget}");
        assert!(
            res.glyphs.len() <= max_glyphs,
            "{budget}: {}",
            res.glyphs.len()
        );
        assert_eq!(res.glyphs[..], full.glyphs[..res.glyphs.len()]);
    }
}

#[test]
fn the_item_budget_caps_emitted_output() {
    let (doc, pages) = fan_doc(3, 1, Limits::default().with_max_page_content_items(50));
    let res = run(&doc, &pages[0]);
    assert!(res.truncated);
    assert_eq!(res.glyphs.len(), 50);
}

#[test]
fn the_budget_is_per_page() {
    let exact = page_cost(2);
    let (doc, pages) = fan_doc(2, 3, Limits::default().with_max_page_content_ops(exact));
    for page in &pages {
        let res = run(&doc, page);
        assert!(!res.truncated);
        assert_eq!(res.glyphs.len(), 100);
    }
}
