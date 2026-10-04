//! Syntactic nesting cap (PRD §9.6.2 `max_recursion_depth`): runs of `[` /
//! `<<` must yield a typed `LimitExceeded(RecursionDepth)` instead of
//! recursing once per byte until the stack overflows.
//!
//! A stack overflow aborts the whole process (no `catch_unwind` can contain
//! it), so every case runs in a child copy of this test binary on a thread
//! with an explicit 2 MiB stack (Rust's default for spawned threads). The
//! parent only inspects the child's exit status.

mod common;

use std::process::Command;

use common::{dict, name_obj, rref, Pdf};
use pdf_core::error::{Error, LimitKind};
use pdf_core::object::parse::Parser;
use pdf_core::{DocumentStore, Limits, Object, ParseMode};

const CHILD_ENV: &str = "PDFSPINE_NESTING_CHILD";
const CHILD_STACK: usize = 2 * 1024 * 1024;

/// Runs `body` in a child process (on a `CHILD_STACK` thread) and asserts the
/// child exited cleanly — a stack overflow shows up as a failed status here
/// rather than taking the test harness down.
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

fn is_depth_error(r: &pdf_core::Result<Object>) -> bool {
    matches!(r, Err(Error::LimitExceeded(LimitKind::RecursionDepth)))
}

fn nested_arrays(depth: usize) -> Vec<u8> {
    let mut v = "[".repeat(depth).into_bytes();
    v.extend_from_slice("]".repeat(depth).as_bytes());
    v
}

fn nested_dicts(depth: usize) -> Vec<u8> {
    let mut v = "<< /A ".repeat(depth).into_bytes();
    v.extend_from_slice(b"1");
    v.extend_from_slice(" >>".repeat(depth).as_bytes());
    v
}

/// The minimal page document whose object 4 body is `body` (unbalanced is fine).
fn doc_with_object_body(body: &[u8]) -> Vec<u8> {
    let mut bytes = Pdf::new()
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
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("Extra", rref(4, 0)),
            ])),
        )
        .obj(4, 0, Object::Null)
        .root(1, 0)
        .build();
    // Splice the raw body in place of object 4's `null`. Objects 1–4 keep their
    // offsets; only `startxref` goes stale, which the lenient open repairs.
    let marker = b"4 0 obj\nnull";
    let at = common::find_first(&bytes, marker).expect("object 4 marker");
    let start = at + b"4 0 obj\n".len();
    bytes.splice(start..start + 4, body.iter().copied());
    bytes
}

#[test]
fn deep_array_run_in_object_body_is_a_typed_error() {
    in_child("deep_array_run_in_object_body_is_a_typed_error", || {
        let bytes = doc_with_object_body(&"[".repeat(20_000).into_bytes());
        let doc = DocumentStore::from_bytes_with(bytes, ParseMode::Lenient, Limits::default())
            .expect("the rest of the document still opens");
        assert!(doc.get_object(4, 0).is_err());
        assert!(doc.get_object(3, 0).is_ok());
    });
}

#[test]
fn deep_runs_are_rejected_by_the_parser() {
    in_child("deep_runs_are_rejected_by_the_parser", || {
        let arrays = "[".repeat(20_000).into_bytes();
        assert!(is_depth_error(&Parser::new(&arrays).parse_object()));
        let dicts = "<< /A ".repeat(20_000).into_bytes();
        assert!(is_depth_error(&Parser::new(&dicts).parse_object()));
        let mixed = "[<< /K ".repeat(10_000).into_bytes();
        assert!(is_depth_error(&Parser::new(&mixed).parse_object()));
        // `N G obj` folded inside an object body recurses as well.
        let indirect = "1 0 obj ".repeat(50_000).into_bytes();
        assert!(is_depth_error(&Parser::new(&indirect).parse_object()));
        // Balanced but too deep is rejected the same way.
        assert!(is_depth_error(
            &Parser::new(&nested_arrays(5_000)).parse_object()
        ));
    });
}

#[test]
fn nesting_up_to_the_default_limit_still_parses() {
    in_child("nesting_up_to_the_default_limit_still_parses", || {
        let max = Limits::DEFAULT.max_recursion_depth as usize;
        let ok = Parser::new(&nested_arrays(max)).parse_object();
        assert!(matches!(ok, Ok(Object::Array(_))), "{ok:?}");
        let ok = Parser::new(&nested_dicts(max)).parse_object();
        assert!(matches!(ok, Ok(Object::Dictionary(_))), "{ok:?}");
        assert!(is_depth_error(
            &Parser::new(&nested_arrays(max + 1)).parse_object()
        ));
        assert!(is_depth_error(
            &Parser::new(&nested_dicts(max + 1)).parse_object()
        ));
        // Ordinary shallow structure is untouched.
        let d = Parser::new(b"<< /K [1 [2 << /X [3] >>]] >>").parse_object();
        assert!(d.is_ok());
    });
}

#[test]
fn nesting_cap_is_configurable_per_parser() {
    let p = Parser::new(&nested_arrays(5))
        .with_max_depth(5)
        .parse_object();
    assert!(matches!(p, Ok(Object::Array(_))), "{p:?}");
    let p = Parser::new(&nested_arrays(6))
        .with_max_depth(5)
        .parse_object();
    assert!(is_depth_error(&p));
}

/// A document whose objects are `objs` (plus a minimal catalog/page tree).
fn doc_with(objs: Vec<(u32, Object)>) -> DocumentStore {
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
                ("Kids", Object::Array(vec![])),
                ("Count", Object::Integer(0)),
            ])),
        );
    for (num, o) in objs {
        pdf = pdf.obj(num, 0, o);
    }
    DocumentStore::from_bytes(pdf.root(1, 0).build(), Limits::default()).expect("open")
}

fn real_array(vals: &[f64]) -> Object {
    Object::Array(vals.iter().map(|&v| Object::Real(v)).collect())
}

fn stitching(children: Vec<Object>) -> Object {
    let k = children.len();
    let bounds: Vec<f64> = (1..k).map(|i| i as f64 / k as f64).collect();
    let encode: Vec<f64> = (0..k).flat_map(|_| [0.0, 1.0]).collect();
    Object::Dictionary(dict([
        ("FunctionType", Object::Integer(3)),
        ("Domain", real_array(&[0.0, 1.0])),
        ("Functions", Object::Array(children)),
        ("Bounds", real_array(&bounds)),
        ("Encode", real_array(&encode)),
    ]))
}

fn exponential() -> Object {
    Object::Dictionary(dict([
        ("FunctionType", Object::Integer(2)),
        ("Domain", real_array(&[0.0, 1.0])),
        ("C0", real_array(&[0.0])),
        ("C1", real_array(&[1.0])),
        ("N", Object::Integer(1)),
    ]))
}

fn function_nodes(f: &pdf_core::PdfFunction) -> usize {
    match f {
        pdf_core::PdfFunction::Stitching { functions, .. } => {
            1 + functions.iter().map(function_nodes).sum::<usize>()
        }
        _ => 1,
    }
}

/// A stitching function listing itself in `/Functions` must not recurse until
/// the stack overflows.
#[test]
fn self_referencing_stitching_function_terminates() {
    in_child("self_referencing_stitching_function_terminates", || {
        let doc = doc_with(vec![(5, stitching(vec![rref(5, 0)]))]);
        let _ = pdf_core::parse_function(&doc, &rref(5, 0));
    });
}

/// A function shared through references (a DAG, fan-out 4, 9 levels = 4⁹
/// leaves) is bounded instead of being expanded exponentially; an ordinary
/// two-level gradient parses unchanged.
#[test]
fn shared_function_dag_is_bounded() {
    let levels = 9u32;
    let mut objs = Vec::new();
    for l in 0..levels {
        objs.push((10 + l, stitching(vec![rref(11 + l, 0); 4])));
    }
    objs.push((10 + levels, exponential()));
    objs.push((
        40,
        stitching(vec![exponential(), stitching(vec![exponential(); 3])]),
    ));
    let doc = doc_with(objs);
    let f = pdf_core::parse_function(&doc, &rref(10, 0)).expect("bounded parse");
    assert!(function_nodes(&f) <= 4096, "{}", function_nodes(&f));
    let g = pdf_core::parse_function(&doc, &rref(40, 0)).expect("gradient");
    assert_eq!(function_nodes(&g), 6);
}

/// A `/VE` expression whose operands share one sub-expression per level (a
/// DAG, 2²⁰ paths) parses into a bounded tree; a self-referencing one ends.
#[test]
fn shared_visibility_expression_is_bounded() {
    let levels = 20u32;
    let mut objs = vec![
        (
            7,
            Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", Object::Null)])),
        ),
        (
            8,
            Object::Dictionary(dict([("Type", name_obj("OCMD")), ("VE", rref(50, 0))])),
        ),
        (
            9,
            Object::Dictionary(dict([("Type", name_obj("OCMD")), ("VE", rref(90, 0))])),
        ),
        (90, Object::Array(vec![name_obj("Not"), rref(90, 0)])),
    ];
    for l in 0..levels {
        objs.push((
            50 + l,
            Object::Array(vec![name_obj("And"), rref(51 + l, 0), rref(51 + l, 0)]),
        ));
    }
    objs.push((50 + levels, Object::Array(vec![name_obj("Or"), rref(7, 0)])));
    let doc = doc_with(objs);

    fn ve_nodes(e: &pdf_core::ocg::VeExpr) -> usize {
        match e {
            pdf_core::ocg::VeExpr::Op { args, .. } => 1 + args.iter().map(ve_nodes).sum::<usize>(),
            pdf_core::ocg::VeExpr::Ocg(_) => 1,
        }
    }
    let info = pdf_core::ocg::get_ocmd(&doc, 8).expect("ocmd");
    let ve = info.ve.expect("parsed ve");
    assert!(ve_nodes(&ve) <= 2 * 1024 + 1, "{}", ve_nodes(&ve));
    let info = pdf_core::ocg::get_ocmd(&doc, 9).expect("self-referencing ocmd");
    assert!(info.ve.is_some());
}
