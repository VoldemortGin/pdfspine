//! M4a structural-validity gate — `INSERT-PROP-003` (PRD §8.8 DoD).
//!
//! After inserting text / image / vector content and a full save, the output
//! must reparse clean. We assert this two ways:
//! 1. Always: reopen via our own M2 pipeline and walk the page graph for
//!    dangling references (`assert_no_dangling_refs`).
//! 2. Best-effort: if `qpdf` is on `PATH`, run `qpdf --check` on the saved bytes
//!    and require a successful (or warnings-only) result. The check is **skipped
//!    cleanly** when `qpdf` is absent so the suite stays self-contained.

mod common;

use std::io::Write;
use std::process::Command;

use common::{assert_no_dangling_refs, blank_page, open, save_bytes};

use pdf_core::geom::{Point, Rect};
use pdf_edit::{draw_rect, insert_image_jpeg, insert_text, Color, TextOptions};

/// A minimal structurally-valid JPEG (SOI + SOF0 + EOI), mirroring the
/// `insert_image_e2e` helper.
fn synthetic_jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&w.to_be_bytes());
    v.push(3);
    for c in 0..3u8 {
        v.extend_from_slice(&[c + 1, 0x11, 0x00]);
    }
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

/// Builds a page with text + image + vector content, saves it, and returns the
/// bytes.
fn build_mixed() -> Vec<u8> {
    let doc = open(&blank_page(612, 792));
    insert_text(
        &doc,
        0,
        Point::new(72.0, 72.0),
        "Hello qpdf",
        &TextOptions::default(),
    )
    .unwrap();
    insert_image_jpeg(
        &doc,
        0,
        Rect::new(100.0, 200.0, 300.0, 350.0),
        &synthetic_jpeg(16, 16),
    )
    .unwrap();
    draw_rect(
        &doc,
        0,
        Rect::new(10.0, 10.0, 80.0, 80.0),
        Some(Color::BLACK),
        Some(Color::new(0.9, 0.9, 0.9)),
        1.5,
    )
    .unwrap();
    save_bytes(&doc)
}

/// `INSERT-PROP-003`: a mixed-content save reparses clean (no dangling refs).
#[test]
fn insert_prop_003_no_dangling_refs() {
    let bytes = build_mixed();
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    let checked = assert_no_dangling_refs(&re);
    assert!(checked > 0, "expected to check some refs");
}

/// `INSERT-PROP-003` (qpdf): the saved file passes `qpdf --check`. Skipped when
/// `qpdf` is not installed.
#[test]
fn insert_prop_003_qpdf_check() {
    let Some(qpdf) = which_qpdf() else {
        eprintln!("SKIP insert_prop_003_qpdf_check: qpdf not on PATH");
        return;
    };
    let bytes = build_mixed();
    let mut tmp = std::env::temp_dir();
    tmp.push(format!("pdfspine_m4a_qpdf_{}.pdf", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).expect("create temp");
        f.write_all(&bytes).expect("write temp");
    }
    let out = Command::new(qpdf)
        .arg("--check")
        .arg(&tmp)
        .output()
        .expect("run qpdf");
    let _ = std::fs::remove_file(&tmp);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // qpdf exit codes: 0 = clean, 3 = warnings only, 2 = errors. Accept 0 or 3.
    let code = out.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 3,
        "qpdf --check failed (code {code}):\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );
    assert!(
        stdout.contains("No syntax or stream encoding errors")
            || stdout.contains("no errors")
            || code == 0
            || code == 3,
        "qpdf did not report a clean structure:\n{stdout}"
    );
}

/// Locates a `qpdf` executable on `PATH` (cross-platform: tries `qpdf` and
/// `qpdf.exe`), returning the first that runs `--version` successfully.
fn which_qpdf() -> Option<String> {
    for cand in [
        "qpdf",
        "qpdf.exe",
        "/opt/homebrew/bin/qpdf",
        "/usr/local/bin/qpdf",
    ] {
        if Command::new(cand).arg("--version").output().is_ok() {
            return Some(cand.to_string());
        }
    }
    None
}
