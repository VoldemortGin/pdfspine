//! INTERN-* — crate-wide Name interner de-dup pool. PRD §9.2.
//!
//! Covers [`pdf_core::NameInterner`]: empty construction, first-sight insertion,
//! byte-equal de-duplication, distinct-name growth, `intern`/`intern_bytes`
//! equivalence, canonical-clone return, the empty name, `Clone` state
//! preservation, and a batch never-panic count against a `std` `HashSet`.

use std::collections::HashSet;

use pdf_core::{Name, NameInterner};

#[test]
fn intern_001_new_and_default_start_empty() {
    // INTERN-001: both constructors yield an empty pool.
    let from_new = NameInterner::new();
    assert_eq!(from_new.len(), 0);
    assert!(from_new.is_empty());

    let from_default = NameInterner::default();
    assert_eq!(from_default.len(), 0);
    assert!(from_default.is_empty());
}

#[test]
fn intern_002_first_intern_inserts() {
    // INTERN-002: first intern returns an equal Name and grows the pool to 1.
    let mut interner = NameInterner::new();
    let result = interner.intern(&Name::new("Type"));

    assert_eq!(result, Name::new("Type"));
    assert_eq!(interner.len(), 1);
    assert!(!interner.is_empty());
}

#[test]
fn intern_003_dedup_same_bytes_keeps_len_one() {
    // INTERN-003: interning equal bytes twice does not grow the pool.
    let mut interner = NameInterner::new();
    let first = interner.intern(&Name::new("Type"));
    let second = interner.intern(&Name::new("Type"));

    assert_eq!(first, Name::new("Type"));
    assert_eq!(second, Name::new("Type"));
    assert_eq!(interner.len(), 1);
}

#[test]
fn intern_004_distinct_names_grow_pool() {
    // INTERN-004: three distinct names produce a pool of size 3.
    let mut interner = NameInterner::new();
    interner.intern(&Name::new("Type"));
    interner.intern(&Name::new("Pages"));
    interner.intern(&Name::new("Contents"));

    assert_eq!(interner.len(), 3);
}

#[test]
fn intern_005_intern_bytes_equivalent_to_intern() {
    // INTERN-005: intern_bytes(b"Type") matches intern(&Name::new("Type")) and
    // the two paths do not double-count.
    let mut interner = NameInterner::new();
    let from_bytes = interner.intern_bytes(b"Type");
    assert_eq!(from_bytes, Name::new("Type"));
    assert_eq!(interner.len(), 1);

    let from_name = interner.intern(&Name::new("Type"));
    assert_eq!(from_name, Name::new("Type"));
    assert_eq!(from_bytes, from_name);
    assert_eq!(interner.len(), 1);
}

#[test]
fn intern_006_second_call_returns_canonical_clone() {
    // INTERN-006: a separately-constructed equal Name returns the pooled entry
    // and does not grow the pool.
    let mut interner = NameInterner::new();
    let first = interner.intern(&Name::new("Type"));

    let separate = Name::from_decoded(b"Type".to_vec());
    let second = interner.intern(&separate);

    assert_eq!(second, first);
    assert_eq!(second, separate);
    assert_eq!(interner.len(), 1);
}

#[test]
fn intern_007_empty_name_is_internable() {
    // INTERN-007: the empty name interns to an empty Name and counts once.
    let mut interner = NameInterner::new();
    let empty = interner.intern_bytes(b"");

    assert!(empty.is_empty());
    assert_eq!(interner.len(), 1);
}

#[test]
fn intern_008_clone_preserves_pooled_state() {
    // INTERN-008: a cloned interner shares the pooled state; re-interning a
    // seen name in the clone does not grow it.
    let mut interner = NameInterner::new();
    interner.intern(&Name::new("Type"));
    interner.intern(&Name::new("Pages"));

    let mut cloned = interner.clone();
    assert_eq!(cloned.len(), interner.len());

    let again = cloned.intern(&Name::new("Type"));
    assert_eq!(again, Name::new("Type"));
    assert_eq!(cloned.len(), 2);
}

#[test]
fn intern_009_batch_len_equals_distinct_inputs() {
    // INTERN-009: interning a batch with duplicates and a non-utf8 entry never
    // panics and yields len() == number of distinct byte sequences.
    let inputs: Vec<Vec<u8>> = vec![
        b"Type".to_vec(),
        b"Pages".to_vec(),
        b"Type".to_vec(),
        b"Contents".to_vec(),
        b"Pages".to_vec(),
        b"".to_vec(),
        b"".to_vec(),
        vec![0xff, 0x00],
        vec![0xff, 0x00],
        b"Kids".to_vec(),
    ];

    let mut interner = NameInterner::new();
    for bytes in &inputs {
        interner.intern_bytes(bytes);
    }

    let expected: HashSet<Vec<u8>> = inputs.iter().cloned().collect();
    assert_eq!(interner.len(), expected.len());
}
