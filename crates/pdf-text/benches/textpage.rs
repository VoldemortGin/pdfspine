//! Criterion benchmarks for the `get_text` pipeline so perf regressions in the
//! interpreter / layout / serializers are catchable.
//!
//! Times, over real corpus pages:
//! - `build_textpage` — interpret + device-transform + line/block grouping;
//! - `to_text` / `to_words` / `to_dict` — serializing a pre-built `TextPage`.
//!
//! The corpus PDFs live in `fixtures/corpus/` (tracked). If a fixture is absent
//! the benchmark is skipped rather than failing, so the suite still runs in a
//! checkout without the corpus.

use std::path::PathBuf;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};

use pdf_core::document::DocumentStore;
use pdf_core::page::Page;
use pdf_core::source::MmapMode;
use pdf_core::{pagetree, Limits};
use pdf_text::{build_textpage, defaults, to_dict, to_text, to_words, TextPage};

/// Resolves a corpus fixture path from the workspace root.
fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/corpus")
        .join(name)
}

/// Opens a corpus doc, returning `None` (with a note) when the fixture is absent.
fn open_doc(name: &str) -> Option<Arc<DocumentStore>> {
    let path = corpus(name);
    if !path.exists() {
        eprintln!("skipping bench: fixture not found: {}", path.display());
        return None;
    }
    DocumentStore::open(&path, MmapMode::Never, Limits::default())
        .ok()
        .map(Arc::new)
}

/// Builds every page's [`TextPage`] for `doc` (the build-side workload).
fn build_all(doc: &Arc<DocumentStore>) -> usize {
    let refs = pagetree::page_refs(doc);
    let mut n = 0;
    for (i, r) in refs.iter().enumerate() {
        let page = Page::new(Arc::clone(doc), i, *r);
        let tp = build_textpage(doc, &page, &Limits::default());
        n += tp.blocks.len();
    }
    n
}

/// Pre-builds every page's [`TextPage`] once (serialize-side input).
fn build_pages(doc: &Arc<DocumentStore>) -> Vec<TextPage> {
    let refs = pagetree::page_refs(doc);
    refs.iter()
        .enumerate()
        .map(|(i, r)| {
            let page = Page::new(Arc::clone(doc), i, *r);
            build_textpage(doc, &page, &Limits::default())
        })
        .collect()
}

fn bench_get_text(c: &mut Criterion) {
    // A mid-size multi-page government doc — representative of the corpus.
    let name = "govinfo-hr2.pdf";
    let Some(doc) = open_doc(name) else { return };

    let mut group = c.benchmark_group("get_text");
    group.sample_size(20);

    group.bench_function("build_textpage", |b| {
        b.iter(|| std::hint::black_box(build_all(&doc)));
    });

    let pages = build_pages(&doc);
    group.bench_function("to_text", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for tp in &pages {
                total += to_text(tp, defaults::TEXT).len();
            }
            std::hint::black_box(total)
        });
    });
    group.bench_function("to_words", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for tp in &pages {
                total += to_words(tp, defaults::WORDS).len();
            }
            std::hint::black_box(total)
        });
    });
    group.bench_function("to_dict", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for tp in &pages {
                total += to_dict(tp, false, defaults::DICT).blocks.len();
            }
            std::hint::black_box(total)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_get_text);
criterion_main!(benches);
