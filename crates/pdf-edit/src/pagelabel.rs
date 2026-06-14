//! Page labels — `/Root /PageLabels` number tree → per-page label (PRD §8.9 §3.5).
//!
//! PyMuPDF's `get_page_labels`/label scheme: each range entry in the number tree
//! gives a start physical page index, a numbering style (`D`/`r`/`R`/`a`/`A`), an
//! optional `/P` prefix, and an optional `/St` start value. The label for a
//! physical page is `prefix + format(style, start + offset_within_range)`.

use pdf_core::object::Name;
use pdf_core::{DocumentStore, ObjRef, Object};

/// One `/PageLabels` range: the first physical page it applies to plus its
/// numbering dictionary fields.
#[derive(Clone, Debug)]
struct LabelRange {
    start_page: usize,
    style: Option<u8>, // b'D' | b'r' | b'R' | b'a' | b'A'
    prefix: String,
    first_value: i64,
}

/// The label of physical page `index` (0-based), or the empty string when the
/// document has no `/PageLabels` (matching PyMuPDF, which returns `""`).
#[must_use]
pub fn get_label(doc: &DocumentStore, index: usize) -> String {
    let ranges = read_ranges(doc);
    if ranges.is_empty() {
        return String::new();
    }
    // Find the range with the greatest start_page <= index.
    let mut chosen: Option<&LabelRange> = None;
    for r in &ranges {
        if r.start_page <= index {
            match chosen {
                Some(c) if c.start_page >= r.start_page => {}
                _ => chosen = Some(r),
            }
        }
    }
    let Some(r) = chosen else {
        return String::new();
    };
    let offset = (index - r.start_page) as i64;
    let value = r.first_value + offset;
    let mut out = r.prefix.clone();
    if let Some(style) = r.style {
        out.push_str(&format_value(style, value));
    }
    out
}

/// Reads + sorts the `/PageLabels` number-tree ranges. Empty when absent.
fn read_ranges(doc: &DocumentStore) -> Vec<LabelRange> {
    let mut out = Vec::new();
    let Some(catalog) = catalog_dict(doc) else {
        return out;
    };
    let Some(pl) = catalog.get(&Name::new("PageLabels")) else {
        return out;
    };
    let pl = deref(doc, pl);
    collect_nums(doc, &pl, &mut out, 0);
    out.sort_by_key(|r| r.start_page);
    out
}

/// Walks a number tree (`/Nums` leaf pairs or `/Kids` branches), pushing label
/// ranges. Depth-guarded.
fn collect_nums(doc: &DocumentStore, node: &Object, out: &mut Vec<LabelRange>, depth: usize) {
    if depth > 50 {
        return;
    }
    let Some(d) = node.as_dict() else {
        return;
    };

    if let Some(nums) = d.get(&Name::new("Nums")) {
        let nums = deref(doc, nums);
        if let Some(arr) = nums.as_array() {
            let mut i = 0;
            while i + 1 < arr.len() {
                if let Some(start) = arr[i].as_i64() {
                    let label_dict = deref(doc, &arr[i + 1]);
                    if let Some(ld) = label_dict.as_dict() {
                        out.push(parse_label_dict(start as usize, ld));
                    }
                }
                i += 2;
            }
        }
    }

    if let Some(kids) = d.get(&Name::new("Kids")) {
        let kids = deref(doc, kids);
        if let Some(arr) = kids.as_array() {
            for kid in arr {
                let kid = deref(doc, kid);
                collect_nums(doc, &kid, out, depth + 1);
            }
        }
    }
}

/// Parses a `/PageLabels` entry dict into a [`LabelRange`].
fn parse_label_dict(start_page: usize, d: &pdf_core::Dict) -> LabelRange {
    let style = d
        .get(&Name::new("S"))
        .and_then(Object::as_name)
        .and_then(|n| n.as_bytes().first().copied());
    let prefix = d
        .get(&Name::new("P"))
        .and_then(Object::as_string)
        .map(|s| decode_text(s.as_bytes()))
        .unwrap_or_default();
    let first_value = d
        .get(&Name::new("St"))
        .and_then(Object::as_i64)
        .unwrap_or(1);
    LabelRange {
        start_page,
        style,
        prefix,
        first_value,
    }
}

/// Formats `value` under a numbering style byte (`D`/`r`/`R`/`a`/`A`).
fn format_value(style: u8, value: i64) -> String {
    match style {
        b'D' => value.to_string(),
        b'r' => to_roman(value, false),
        b'R' => to_roman(value, true),
        b'a' => to_alpha(value, false),
        b'A' => to_alpha(value, true),
        _ => value.to_string(),
    }
}

/// Roman numerals (lowercase or uppercase). Non-positive values format as
/// decimal (PDF spec leaves this undefined; PyMuPDF falls back to the number).
fn to_roman(mut n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    const VALUES: [(i64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (v, sym) in VALUES {
        while n >= v {
            s.push_str(sym);
            n -= v;
        }
    }
    if upper {
        s.to_uppercase()
    } else {
        s
    }
}

/// Spreadsheet-style alphabetic labels: 1→a, 26→z, 27→aa, 28→bb, … (PDF §12.4.2
/// "A"/"a" style: the letter repeats, `((n-1) mod 26)` cycled `((n-1) div 26)+1`
/// times).
fn to_alpha(n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let zero = if upper { b'A' } else { b'a' };
    let idx = ((n - 1) % 26) as u8;
    let count = ((n - 1) / 26) as usize + 1;
    let ch = (zero + idx) as char;
    std::iter::repeat_n(ch, count).collect()
}

/// Minimal PDF text-string decode (UTF-16BE BOM, else Latin-1) for the prefix.
fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn deref(doc: &DocumentStore, obj: &Object) -> Object {
    match obj {
        Object::Reference(r) => doc
            .resolve(*r)
            .map(|a| (*a).clone())
            .unwrap_or(Object::Null),
        other => other.clone(),
    }
}

fn catalog_dict(doc: &DocumentStore) -> Option<pdf_core::Dict> {
    let root: ObjRef = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}
