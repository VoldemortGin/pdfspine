//! Shared CMap parser (PRD §8.5; ISO 32000-1 §9.7.5 / §9.10.3).
//!
//! One parser serves two roles:
//!
//! - **ToUnicode** CMaps: `beginbfchar` / `beginbfrange` map a code →
//!   Unicode string (handling UTF-16BE values, surrogate pairs and 1-to-many
//!   ligature destinations).
//! - **CID encoding** CMaps: `begincidchar` / `begincidrange` map a code →
//!   CID; `begincodespacerange` declares the byte-length structure that drives
//!   variable-length code iteration.
//!
//! `usecmap` chaining is supported by merging a parent [`CMap`]'s ranges first.
//! The tokenizer is deliberately tolerant: unknown operators and malformed
//! operands are skipped, never panicking (PRD §8.5 defensive contract).

use smol_str::SmolStr;

/// A codespace range: byte strings of width `n_bytes` whose value lies in
/// `low..=high` are valid codes of that length (ISO 32000-1 §9.7.6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodespaceRange {
    /// Number of bytes for codes in this range (1–4).
    pub n_bytes: u8,
    /// Inclusive low value.
    pub low: u32,
    /// Inclusive high value.
    pub high: u32,
}

/// A `code → CID` single mapping or contiguous range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CidRange {
    lo: u32,
    hi: u32,
    cid: u32,
}

/// A `code → Unicode` mapping (single code; destination is a full string so
/// ligatures / astral characters are representable).
#[derive(Clone, Debug, PartialEq, Eq)]
struct BfEntry {
    lo: u32,
    hi: u32,
    /// For a single bf entry `lo == hi` and `dst` is that string; for a range
    /// with an incrementing base, `dst` is the base for `lo` and successive
    /// codes increment the *last* scalar. The array form is expanded into one
    /// `BfEntry` per code at parse time (so `lo == hi`).
    dst: SmolStr,
}

/// A parsed CMap: codespace structure + CID ranges + ToUnicode ranges.
///
/// A given CMap typically populates only the CID side **or** the bf side, but
/// the same struct holds both so the parser is single-pass and `usecmap` can
/// chain either flavor.
#[derive(Clone, Debug, Default)]
pub struct CMap {
    codespace: Vec<CodespaceRange>,
    cid_ranges: Vec<CidRange>,
    bf_entries: Vec<BfEntry>,
}

impl CMap {
    /// Parses a CMap program (a content-stream-like token sequence) from bytes.
    /// `resolve_use` is called for a `usecmap` name to fetch the parent CMap, if
    /// the caller can supply it (predefined CMaps / embedded `/UseCMap`).
    /// Returning `None` simply skips the chaining (tolerant).
    #[must_use]
    pub fn parse(bytes: &[u8], resolve_use: &mut dyn FnMut(&[u8]) -> Option<CMap>) -> CMap {
        let mut cmap = CMap::default();
        let toks = tokenize(bytes);
        let mut i = 0;
        while i < toks.len() {
            match &toks[i] {
                Tok::Op(op) if op == b"usecmap" => {
                    // The name precedes `usecmap`.
                    if i > 0 {
                        if let Tok::Name(name) = &toks[i - 1] {
                            if let Some(parent) = resolve_use(name) {
                                cmap.merge_parent(&parent);
                            }
                        }
                    }
                    i += 1;
                }
                Tok::Op(op) if op == b"begincodespacerange" => {
                    i = parse_codespace(&toks, i + 1, &mut cmap);
                }
                Tok::Op(op) if op == b"beginbfchar" => {
                    i = parse_bfchar(&toks, i + 1, &mut cmap);
                }
                Tok::Op(op) if op == b"beginbfrange" => {
                    i = parse_bfrange(&toks, i + 1, &mut cmap);
                }
                Tok::Op(op) if op == b"begincidchar" => {
                    i = parse_cidchar(&toks, i + 1, &mut cmap);
                }
                Tok::Op(op) if op == b"begincidrange" => {
                    i = parse_cidrange(&toks, i + 1, &mut cmap);
                }
                _ => i += 1,
            }
        }
        cmap
    }

    /// Merges a parent (used by `usecmap`): the parent's ranges are the base,
    /// and `self`'s ranges (parsed later) take precedence on lookup because they
    /// are appended after and lookups scan in reverse.
    fn merge_parent(&mut self, parent: &CMap) {
        // Prepend parent ranges so child entries (appended later) win on the
        // reverse scan used in lookups.
        let mut codespace = parent.codespace.clone();
        codespace.append(&mut self.codespace);
        self.codespace = codespace;

        let mut cid = parent.cid_ranges.clone();
        cid.append(&mut self.cid_ranges);
        self.cid_ranges = cid;

        let mut bf = parent.bf_entries.clone();
        bf.append(&mut self.bf_entries);
        self.bf_entries = bf;
    }

    /// The declared codespace ranges (drives [`crate::mapper`] code iteration).
    #[must_use]
    pub fn codespace(&self) -> &[CodespaceRange] {
        &self.codespace
    }

    /// Maps a code to a CID via the CID ranges, if covered. Later entries win.
    #[must_use]
    pub fn cid(&self, code: u32) -> Option<u32> {
        for r in self.cid_ranges.iter().rev() {
            if code >= r.lo && code <= r.hi {
                return Some(r.cid + (code - r.lo));
            }
        }
        None
    }

    /// Maps a code to a Unicode string via the bf entries, if covered. Later
    /// entries win (matching Acrobat's last-wins behavior for overlaps).
    #[must_use]
    pub fn to_unicode(&self, code: u32) -> Option<SmolStr> {
        for e in self.bf_entries.iter().rev() {
            if code >= e.lo && code <= e.hi {
                if e.lo == e.hi {
                    return Some(e.dst.clone());
                }
                // Range with incrementing base: bump the last scalar by the
                // offset. Multi-scalar bases (ligature ranges) are rare; we
                // increment the final char which matches the spec intent.
                let offset = code - e.lo;
                return Some(increment_last(&e.dst, offset));
            }
        }
        None
    }

    /// The number of distinct code lengths that appear in the codespace. Used by
    /// callers to decide whether iteration is fixed- or variable-width.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.codespace.is_empty() && self.cid_ranges.is_empty() && self.bf_entries.is_empty()
    }
}

/// Bumps the final scalar of `base` by `offset` (for `bfrange` increment form).
fn increment_last(base: &str, offset: u32) -> SmolStr {
    let mut chars: Vec<char> = base.chars().collect();
    if let Some(last) = chars.last_mut() {
        let v = (*last as u32).wrapping_add(offset);
        if let Some(c) = char::from_u32(v) {
            *last = c;
        }
    }
    let s: String = chars.into_iter().collect();
    SmolStr::new(&s)
}

// --- low-level tokenizer --------------------------------------------------

/// A CMap token. We only need names, hex/numeric operands, the bracket
/// delimiters for bf array destinations, and bare operators.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// A `/Name`.
    Name(Vec<u8>),
    /// A `<hex>` string (raw bytes).
    Hex(Vec<u8>),
    /// An integer operand.
    Int(i64),
    /// `[` array open.
    ArrayOpen,
    /// `]` array close.
    ArrayClose,
    /// A bare keyword/operator (`beginbfchar`, `endcidrange`, …).
    Op(Vec<u8>),
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Tokenizes a CMap program. Tolerant: unrecognized bytes advance by one.
fn tokenize(bytes: &[u8]) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        let b = bytes[i];
        if is_ws(b) {
            i += 1;
            continue;
        }
        match b {
            b'%' => {
                // Comment to end of line.
                while i < n && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
            }
            b'/' => {
                let mut j = i + 1;
                while j < n && !is_ws(bytes[j]) && !is_delim(bytes[j]) {
                    j += 1;
                }
                toks.push(Tok::Name(bytes[i + 1..j].to_vec()));
                i = j;
            }
            b'<' => {
                // `<<` dict open — skip both chars (we don't need dict bodies).
                if i + 1 < n && bytes[i + 1] == b'<' {
                    toks.push(Tok::Op(b"<<".to_vec()));
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                let mut hex = Vec::new();
                let mut hi: Option<u8> = None;
                while j < n && bytes[j] != b'>' {
                    let c = bytes[j];
                    if let Some(d) = hex_val(c) {
                        match hi {
                            None => hi = Some(d),
                            Some(h) => {
                                hex.push((h << 4) | d);
                                hi = None;
                            }
                        }
                    }
                    j += 1;
                }
                if let Some(h) = hi {
                    hex.push(h << 4); // odd nibble → pad low
                }
                toks.push(Tok::Hex(hex));
                i = if j < n { j + 1 } else { j };
            }
            b'>' => {
                if i + 1 < n && bytes[i + 1] == b'>' {
                    toks.push(Tok::Op(b">>".to_vec()));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'[' => {
                toks.push(Tok::ArrayOpen);
                i += 1;
            }
            b']' => {
                toks.push(Tok::ArrayClose);
                i += 1;
            }
            b'(' => {
                // Literal string: skip balanced parens (rare in CMaps).
                let mut depth = 1;
                let mut j = i + 1;
                while j < n && depth > 0 {
                    match bytes[j] {
                        b'\\' => j += 1,
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                i = j;
            }
            b'{' | b'}' | b')' => {
                i += 1;
            }
            _ => {
                // A number or a bare keyword.
                let mut j = i;
                while j < n && !is_ws(bytes[j]) && !is_delim(bytes[j]) {
                    j += 1;
                }
                let word = &bytes[i..j];
                if let Some(v) = parse_int(word) {
                    toks.push(Tok::Int(v));
                } else {
                    toks.push(Tok::Op(word.to_vec()));
                }
                i = j;
            }
        }
    }
    toks
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn parse_int(w: &[u8]) -> Option<i64> {
    if w.is_empty() {
        return None;
    }
    let (neg, digits) = match w[0] {
        b'-' => (true, &w[1..]),
        b'+' => (false, &w[1..]),
        _ => (false, w),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut v: i64 = 0;
    for &d in digits {
        v = v.checked_mul(10)?.checked_add(i64::from(d - b'0'))?;
    }
    Some(if neg { -v } else { v })
}

// --- operand decoders -----------------------------------------------------

/// A hex byte string interpreted as a big-endian integer code/CID.
fn hex_to_u32(h: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &b in h.iter().take(4) {
        v = (v << 8) | u32::from(b);
    }
    v
}

/// A hex byte string interpreted as UTF-16BE → Unicode string (the ToUnicode
/// destination form). Falls back to Latin-1 for an odd byte count.
fn hex_to_unicode(h: &[u8]) -> SmolStr {
    if h.len().is_multiple_of(2) && !h.is_empty() {
        let units: Vec<u16> = h
            .chunks_exact(2)
            .map(|c| (u16::from(c[0]) << 8) | u16::from(c[1]))
            .collect();
        // Decode UTF-16 (handles surrogate pairs); lossy on unpaired surrogates.
        let s: String = char::decode_utf16(units.iter().copied())
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect();
        return SmolStr::new(&s);
    }
    // Odd length: treat each byte as Latin-1.
    let s: String = h.iter().map(|&b| b as char).collect();
    SmolStr::new(&s)
}

// --- section parsers (return the index just past `end<...>`) ---------------

fn parse_codespace(toks: &[Tok], mut i: usize, cmap: &mut CMap) -> usize {
    while i < toks.len() {
        match &toks[i] {
            Tok::Op(op) if op == b"endcodespacerange" => return i + 1,
            Tok::Hex(lo) => {
                if let Some(Tok::Hex(hi)) = toks.get(i + 1) {
                    let n = lo.len().max(1) as u8;
                    cmap.codespace.push(CodespaceRange {
                        n_bytes: n.min(4),
                        low: hex_to_u32(lo),
                        high: hex_to_u32(hi),
                    });
                    i += 2;
                    continue;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

fn parse_bfchar(toks: &[Tok], mut i: usize, cmap: &mut CMap) -> usize {
    while i < toks.len() {
        match &toks[i] {
            Tok::Op(op) if op == b"endbfchar" => return i + 1,
            Tok::Hex(src) => {
                if let Some(Tok::Hex(dst)) = toks.get(i + 1) {
                    let code = hex_to_u32(src);
                    cmap.bf_entries.push(BfEntry {
                        lo: code,
                        hi: code,
                        dst: hex_to_unicode(dst),
                    });
                    i += 2;
                    continue;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

fn parse_bfrange(toks: &[Tok], mut i: usize, cmap: &mut CMap) -> usize {
    while i < toks.len() {
        match &toks[i] {
            Tok::Op(op) if op == b"endbfrange" => return i + 1,
            Tok::Hex(lo) => {
                let Some(Tok::Hex(hi)) = toks.get(i + 1) else {
                    i += 1;
                    continue;
                };
                let lo_v = hex_to_u32(lo);
                let hi_v = hex_to_u32(hi);
                match toks.get(i + 2) {
                    // Increment form: base destination string.
                    Some(Tok::Hex(base)) => {
                        cmap.bf_entries.push(BfEntry {
                            lo: lo_v,
                            hi: hi_v,
                            dst: hex_to_unicode(base),
                        });
                        i += 3;
                    }
                    // Array-of-destinations form: one dst per code.
                    Some(Tok::ArrayOpen) => {
                        let mut j = i + 3;
                        let mut code = lo_v;
                        while j < toks.len() {
                            match &toks[j] {
                                Tok::ArrayClose => {
                                    j += 1;
                                    break;
                                }
                                Tok::Hex(d) => {
                                    cmap.bf_entries.push(BfEntry {
                                        lo: code,
                                        hi: code,
                                        dst: hex_to_unicode(d),
                                    });
                                    code = code.wrapping_add(1);
                                    j += 1;
                                }
                                _ => j += 1,
                            }
                        }
                        i = j;
                    }
                    _ => i += 2,
                }
            }
            _ => i += 1,
        }
    }
    i
}

fn parse_cidchar(toks: &[Tok], mut i: usize, cmap: &mut CMap) -> usize {
    while i < toks.len() {
        match &toks[i] {
            Tok::Op(op) if op == b"endcidchar" => return i + 1,
            Tok::Hex(src) => match toks.get(i + 1) {
                Some(Tok::Int(cid)) if *cid >= 0 => {
                    let code = hex_to_u32(src);
                    cmap.cid_ranges.push(CidRange {
                        lo: code,
                        hi: code,
                        cid: *cid as u32,
                    });
                    i += 2;
                }
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
    i
}

fn parse_cidrange(toks: &[Tok], mut i: usize, cmap: &mut CMap) -> usize {
    while i < toks.len() {
        match &toks[i] {
            Tok::Op(op) if op == b"endcidrange" => return i + 1,
            Tok::Hex(lo) => {
                let (Some(Tok::Hex(hi)), Some(Tok::Int(cid))) = (toks.get(i + 1), toks.get(i + 2))
                else {
                    i += 1;
                    continue;
                };
                if *cid >= 0 {
                    cmap.cid_ranges.push(CidRange {
                        lo: hex_to_u32(lo),
                        hi: hex_to_u32(hi),
                        cid: *cid as u32,
                    });
                }
                i += 3;
            }
            _ => i += 1,
        }
    }
    i
}
