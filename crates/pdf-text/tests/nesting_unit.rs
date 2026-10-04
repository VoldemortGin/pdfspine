//! Content-stream operand nesting cap: runs of `[` / `<<` must degrade (skip
//! the over-deep operand, record an issue, keep tokenizing) instead of
//! recursing once per byte until the stack overflows.
//!
//! A stack overflow aborts the whole process, so each overflow-shaped case runs
//! in a child copy of this test binary on a thread with an explicit 2 MiB stack
//! (Rust's default for spawned threads); the parent checks the exit status.

mod common;

use std::process::Command;

use common::{glyph_text, run_with_font, winansi_type1};
use pdf_core::{Limits, Object};
use pdf_text::tokenizer::{tokenize, tokenize_audited, Event};

const CHILD_ENV: &str = "PDFSPINE_NESTING_CHILD";
const CHILD_STACK: usize = 2 * 1024 * 1024;

fn in_child(test_name: &str, body: fn()) {
    if std::env::var(CHILD_ENV).as_deref() == Ok(test_name) {
        std::thread::Builder::new()
            .stack_size(CHILD_STACK)
            .spawn(body)
            .expect("spawn child thread")
            .join()
            .expect("child body panicked");
        return;
    }
    let out = Command::new(std::env::current_exe().expect("current exe"))
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, test_name)
        .output()
        .expect("spawn child test process");
    assert!(
        out.status.success(),
        "child `{test_name}` failed ({:?}):\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Nesting depth of the first operand (1 for a flat array).
fn operand_depth(o: &Object) -> usize {
    let mut depth = 0;
    let mut cur = Some(o);
    while let Some(o) = cur {
        cur = match o {
            Object::Array(a) => {
                depth += 1;
                a.first()
            }
            Object::Dictionary(d) => {
                depth += 1;
                d.iter().next().map(|(_, v)| v)
            }
            _ => None,
        };
    }
    depth
}

fn font() -> Object {
    winansi_type1("Helvetica", 32, &[500; 96])
}

#[test]
fn deep_operand_runs_do_not_overflow_the_tokenizer() {
    in_child("deep_operand_runs_do_not_overflow_the_tokenizer", || {
        for content in [
            "[".repeat(100_000),
            "<< /A ".repeat(100_000),
            "[<< /K ".repeat(50_000),
        ] {
            let events = tokenize(content.as_bytes());
            assert!(events.len() <= 1, "{}", events.len());
        }
    });
}

#[test]
fn deep_operand_run_does_not_overflow_page_interpretation() {
    in_child(
        "deep_operand_run_does_not_overflow_page_interpretation",
        || {
            let mut content = "[".repeat(100_000).into_bytes();
            content.extend_from_slice(b" BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
            let _ = run_with_font(font(), &content);
        },
    );
}

#[test]
fn operands_up_to_the_cap_keep_their_structure() {
    let max = Limits::DEFAULT.max_recursion_depth as usize;
    let mut content = "[".repeat(max).into_bytes();
    content.extend_from_slice("]".repeat(max).as_bytes());
    content.extend_from_slice(b" Tj");
    let events = tokenize(&content);
    assert_eq!(events.len(), 2, "{events:?}");
    let Event::Operand(o) = &events[0] else {
        panic!("first event is not an operand: {:?}", events[0]);
    };
    assert_eq!(operand_depth(o), max);
    assert!(tokenize_audited(&content).issues.is_empty());
}

#[test]
fn text_after_a_balanced_deep_operand_is_still_shown() {
    let mut content = "[".repeat(300).into_bytes();
    content.extend_from_slice("]".repeat(300).as_bytes());
    content.extend_from_slice(b" BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
    assert_eq!(glyph_text(&run_with_font(font(), &content)), "Hi");
}
