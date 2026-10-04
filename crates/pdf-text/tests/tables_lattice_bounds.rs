//! Lattice (`Lines` strategy) table detection on rulings that never close a
//! cell: N long vertical rules plus N short horizontal rules off to the side.
//! The exhaustive corner × bottom × right × edge search was O(N⁵) (N = 240:
//! ~2 minutes in release); the indexed search is near-linear.
//!
//! The pre-fix search does not finish in any reasonable time, so the case runs
//! in a child copy of this test binary under a wall-clock deadline.

mod common;

use std::process::Command;
use std::time::{Duration, Instant};

use common::run_content;
use pdf_core::geom::{Matrix, Rect};
use pdf_core::Dict;
use pdf_text::layout::{page_transform, textpage_from_glyphs};
use pdf_text::tables::{drawings_to_device, find_tables, Strategy, TableOptions};
use pdf_text::words;

const CHILD_ENV: &str = "PDFSPINE_LATTICE_CHILD";
/// Far above the indexed search's cost (milliseconds even in debug builds),
/// far below the exhaustive search's (minutes for N = 240).
const DEADLINE: Duration = Duration::from_secs(60);

fn within_deadline(test_name: &str, body: fn()) {
    if std::env::var(CHILD_ENV).as_deref() == Ok(test_name) {
        body();
        return;
    }
    let mut child = Command::new(std::env::current_exe().expect("current exe"))
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, test_name)
        .spawn()
        .expect("spawn child test process");
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            assert!(status.success(), "child `{test_name}` failed: {status:?}");
            return;
        }
        if start.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child `{test_name}` exceeded {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The probe's ruling set: `n` vertical rules 4.5 pt apart spanning every row,
/// and `n` horizontal rules 4.5 pt apart lying entirely right of them.
fn unclosed_grid(n: usize) -> Vec<u8> {
    let mut out = String::from("0.5 w\n");
    for i in 0..n {
        let x = 20.0 + 4.5 * i as f64;
        out.push_str(&format!("{x:.1} 20 m {x:.1} 2000 l S\n"));
    }
    for i in 0..n {
        let y = 20.0 + 4.5 * i as f64;
        out.push_str(&format!("1500 {y:.1} m 1560 {y:.1} l S\n"));
    }
    out.into_bytes()
}

#[test]
fn unclosed_rulings_are_searched_in_near_linear_time() {
    within_deadline("unclosed_rulings_are_searched_in_near_linear_time", || {
        let page = Rect::new(0.0, 0.0, 612.0, 792.0);
        for n in [60, 120, 240] {
            let res = run_content(&unclosed_grid(n), Dict::new(), Matrix::IDENTITY);
            let tp = textpage_from_glyphs(&res.glyphs, &res.images, page, 0);
            let w = words(&tp);
            let dr = drawings_to_device(&res.drawings, &page_transform(page, 0));
            assert_eq!(dr.len(), 2 * n);
            let tf = find_tables(&tp, &w, &dr, &TableOptions::with_strategy(Strategy::Lines));
            assert!(tf.tables.is_empty(), "n = {n}: {} tables", tf.tables.len());
        }
    });
}
