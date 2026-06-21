//! Minimal Type1 font program (`/FontFile`, PFB/PFA) outliner.
//!
//! `ttf-parser` parses sfnt-wrapped TrueType (`FontFile2`) and bare CFF
//! (`FontFile3`), but **not** the eexec-encrypted Adobe Type1 format
//! (`FontFile`). This module fills that gap with a small, dependency-free
//! interpreter so an *embedded* Type1 program renders real glyph outlines
//! instead of staying blank (PRD-NEXT P4-2).
//!
//! Pipeline:
//! 1. [`unwrap_pfb`] strips the optional binary PFB segment framing (`0x80`
//!    markers) — a PFA / already-unwrapped program passes through unchanged.
//! 2. The cleartext portion yields `/Encoding` (code → glyph name).
//! 3. [`eexec_decrypt`] (R=55665) decrypts the private dict, exposing `/Subrs`
//!    and `/CharStrings`; each charstring is individually decrypted (R=4330,
//!    skipping `lenIV` leading bytes).
//! 4. [`Type1Font::outline`] interprets the Type1 charstring operators into an
//!    outline via any [`ttf_parser::OutlineBuilder`], so the result feeds the
//!    *same* `PathSink` → `tiny_skia::Path` path as the TrueType / CFF routes
//!    (no duplicated rasterization).
//!
//! Glyphs are keyed by PostScript name (`glyph_for_name`), the resolution path
//! the renderer already uses for CFF/Type1 simple fonts: the PDF `/Encoding`
//! (with `/Differences`) maps a code to a name, which maps here to an outline.
//! Synthetic GIDs are assigned in charstring declaration order.

use std::collections::HashMap;

use ttf_parser::OutlineBuilder;

/// The Type1 charstring decryption constant (Adobe Type1 spec §7).
const CHARSTRING_R: u16 = 4330;
/// The eexec (private dict) decryption constant.
const EEXEC_R: u16 = 55665;
const C1: u16 = 52845;
const C2: u16 = 22719;

/// A parsed, ready-to-outline Type1 font program.
pub struct Type1Font {
    /// Glyph name → decrypted charstring bytes.
    charstrings: HashMap<String, Vec<u8>>,
    /// `/Subrs[i]` → decrypted charstring bytes (a missing entry is empty).
    subrs: Vec<Vec<u8>>,
    /// Synthetic GID → glyph name, in `/CharStrings` declaration order. GID 0 is
    /// `.notdef` when present, else the first declared glyph.
    gid_names: Vec<String>,
    /// Glyph name → synthetic GID (inverse of `gid_names`).
    name_to_gid: HashMap<String, u16>,
    /// Builtin `/Encoding` (cleartext): code → glyph name. Sparse — only codes
    /// the font program assigns. `None` when the program omits a custom array
    /// (e.g. `/Encoding StandardEncoding def`, handled by the caller's AGL path).
    builtin_encoding: Option<Box<[Option<String>; 256]>>,
    /// `FontMatrix` x-scale → design grid size (`round(1/sx)`, default 1000).
    upem: u16,
}

impl Type1Font {
    /// Parses a Type1 program (`/FontFile` bytes, PFB or PFA / raw).
    ///
    /// Returns `None` when the bytes are not a recognizable Type1 program (no
    /// eexec section, or no decryptable `/CharStrings`).
    pub fn parse(data: &[u8]) -> Option<Self> {
        let flat = unwrap_pfb(data);
        let (clear, binary) = split_eexec(&flat)?;
        // The private portion is eexec-encrypted; decrypt then drop the 4 random
        // lead bytes (Type1 spec: the first 4 plaintext bytes are garbage).
        let priv_dict = eexec_decrypt(&binary, EEXEC_R);
        if priv_dict.len() <= 4 {
            return None;
        }
        let priv_dict = &priv_dict[4..];

        let len_iv = parse_len_iv(priv_dict);
        let subrs = parse_subrs(priv_dict, len_iv);
        let charstrings = parse_charstrings(priv_dict, len_iv);
        if charstrings.is_empty() {
            return None;
        }

        let upem = parse_font_matrix_upem(clear);
        let builtin_encoding = parse_builtin_encoding(clear);

        // Synthetic GID order: declaration order in /CharStrings, with .notdef
        // forced to GID 0 if present (mirrors sfnt/CFF convention).
        let mut gid_names: Vec<String> = Vec::with_capacity(charstrings.len());
        if charstrings.contains_key(".notdef") {
            gid_names.push(".notdef".to_owned());
        }
        let mut rest: Vec<&String> = charstrings.keys().filter(|n| *n != ".notdef").collect();
        rest.sort();
        gid_names.extend(rest.into_iter().cloned());
        let name_to_gid = gid_names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u16))
            .collect();

        Some(Self {
            charstrings,
            subrs,
            gid_names,
            name_to_gid,
            builtin_encoding,
            upem,
        })
    }

    /// The design grid size (`units_per_em`); never zero.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.upem.max(1)
    }

    /// The number of glyphs (declared charstrings).
    #[must_use]
    pub fn num_glyphs(&self) -> u16 {
        u16::try_from(self.gid_names.len()).unwrap_or(u16::MAX)
    }

    /// Looks up the synthetic GID for a PostScript glyph **name**.
    #[must_use]
    pub fn glyph_for_name(&self, name: &str) -> Option<u16> {
        self.name_to_gid.get(name).copied()
    }

    /// Looks up the synthetic GID for a 1-byte character `code` via the font's
    /// **builtin** `/Encoding` (P4-2r): the code → glyph-name array declared in
    /// the cleartext program. This is the authoritative code→glyph map for a
    /// Type1 font whose builtin encoding is non-standard (non-AGL) and that the
    /// PDF uses *without* its own `/Encoding` override. Returns `None` when the
    /// program has no custom encoding array or the code is unassigned / absent.
    #[must_use]
    pub fn glyph_for_code(&self, code: u8) -> Option<u16> {
        let name = self.builtin_encoding.as_ref()?[code as usize].as_deref()?;
        self.glyph_for_name(name)
    }

    /// Interprets glyph `gid`'s charstring into `builder` (font units, y-up).
    ///
    /// Returns `false` for an absent/empty glyph (the caller draws nothing).
    pub fn outline(&self, gid: u16, builder: &mut dyn OutlineBuilder) -> bool {
        let Some(name) = self.gid_names.get(gid as usize) else {
            return false;
        };
        let Some(cs) = self.charstrings.get(name) else {
            return false;
        };
        let mut exec = Interp::new(self, builder);
        exec.run(cs);
        exec.finish()
    }

    /// Resolves a `seac` accent composition component by StandardEncoding code
    /// (Type1 `seac` always names its base/accent via StandardEncoding).
    fn charstring_by_std_code(&self, code: u8) -> Option<&[u8]> {
        let name = pdf_fonts::BaseEncoding::Standard.glyph_name(code)?;
        self.charstrings.get(name).map(Vec::as_slice)
    }
}

/// Type1 charstring interpreter state. Borrows the [`OutlineBuilder`] for the
/// duration of one glyph and emits move/line/curve/close calls.
struct Interp<'a, 'b> {
    font: &'a Type1Font,
    builder: &'b mut dyn OutlineBuilder,
    stack: Vec<f32>,
    /// PostScript-style stack used by `callothersubl` argument passing.
    ps_stack: Vec<f32>,
    x: f32,
    y: f32,
    /// Left sidebearing x (from `hsbw`/`sbw`); the `seac` accent offset basis.
    sbx: f32,
    open: bool,
    any_contour: bool,
    /// `flex` point accumulator (`OtherSubrs` 0/1/2); collects the 7 rmoveto
    /// reference points so the body emits a single pair of curves.
    flex_pts: Vec<(f32, f32)>,
    in_flex: bool,
    /// Recursion / step guard against malformed subrs.
    steps: u32,
}

impl<'a, 'b> Interp<'a, 'b> {
    fn new(font: &'a Type1Font, builder: &'b mut dyn OutlineBuilder) -> Self {
        Self {
            font,
            builder,
            stack: Vec::with_capacity(32),
            ps_stack: Vec::with_capacity(8),
            x: 0.0,
            y: 0.0,
            sbx: 0.0,
            open: false,
            any_contour: false,
            flex_pts: Vec::with_capacity(8),
            in_flex: false,
            steps: 0,
        }
    }

    fn finish(&mut self) -> bool {
        if self.open {
            self.builder.close();
            self.open = false;
        }
        self.any_contour
    }

    fn move_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.builder.close();
        }
        self.x = x;
        self.y = y;
        self.builder.move_to(x, y);
        self.open = true;
        self.any_contour = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.builder.line_to(x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.builder.curve_to(x1, y1, x2, y2, x, y);
    }

    /// Interprets a charstring byte sequence. Recurses through `callsubr`.
    fn run(&mut self, cs: &[u8]) -> Flow {
        let mut i = 0;
        while i < cs.len() {
            self.steps += 1;
            if self.steps > 200_000 {
                return Flow::Stop; // runaway guard.
            }
            let b = cs[i];
            i += 1;
            if b >= 32 {
                // Operand encoding.
                let v = if b <= 246 {
                    i32::from(b) - 139
                } else if b <= 250 {
                    let w = cs.get(i).copied().unwrap_or(0);
                    i += 1;
                    (i32::from(b) - 247) * 256 + i32::from(w) + 108
                } else if b <= 254 {
                    let w = cs.get(i).copied().unwrap_or(0);
                    i += 1;
                    -((i32::from(b) - 251) * 256) - i32::from(w) - 108
                } else {
                    // 255: a 32-bit signed integer follows.
                    let mut buf = [0u8; 4];
                    for slot in &mut buf {
                        *slot = cs.get(i).copied().unwrap_or(0);
                        i += 1;
                    }
                    i32::from_be_bytes(buf)
                };
                self.stack.push(v as f32);
                continue;
            }
            // Operator.
            if b == 12 {
                let b2 = cs.get(i).copied().unwrap_or(0);
                i += 1;
                match self.op_escape(b2) {
                    Flow::Continue => {}
                    Flow::Return => return Flow::Return,
                    Flow::Stop => return Flow::Stop,
                }
            } else {
                match self.op(b) {
                    Flow::Continue => {}
                    Flow::Return => return Flow::Return,
                    Flow::Stop => return Flow::Stop,
                }
            }
        }
        Flow::Continue
    }

    /// One-byte operators.
    fn op(&mut self, op: u8) -> Flow {
        match op {
            1 | 3 => {
                // hstem / vstem: hints — ignored for outlines.
                self.stack.clear();
            }
            4 => {
                // vmoveto: dy
                let dy = self.arg_last(1)[0];
                if self.in_flex {
                    self.flex_pts.push((self.x, self.y + dy));
                    self.y += dy;
                } else {
                    self.move_to(self.x, self.y + dy);
                }
                self.stack.clear();
            }
            5 => {
                // rlineto: dx dy
                let a = self.arg_last(2);
                self.line_to(self.x + a[0], self.y + a[1]);
                self.stack.clear();
            }
            6 => {
                // hlineto: dx
                let dx = self.arg_last(1)[0];
                self.line_to(self.x + dx, self.y);
                self.stack.clear();
            }
            7 => {
                // vlineto: dy
                let dy = self.arg_last(1)[0];
                self.line_to(self.x, self.y + dy);
                self.stack.clear();
            }
            8 => {
                // rrcurveto: dx1 dy1 dx2 dy2 dx3 dy3
                let a = self.arg_last(6);
                self.rel_curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                self.stack.clear();
            }
            9 => {
                // closepath
                if self.open {
                    self.builder.close();
                    self.open = false;
                    // A subsequent draw op reopens at the current point.
                }
                self.stack.clear();
            }
            10 => {
                // callsubr: subr#
                let idx = self.stack.pop().unwrap_or(0.0) as i32;
                if let Some(sub) = usize::try_from(idx)
                    .ok()
                    .and_then(|u| self.font.subrs.get(u))
                {
                    let sub = sub.clone();
                    if matches!(self.run(&sub), Flow::Stop) {
                        return Flow::Stop;
                    }
                }
            }
            11 => return Flow::Return, // return
            13 => {
                // hsbw: sbx wx — set the left sidebearing as the start point.
                let a = self.arg_last(2);
                self.sbx = a[0];
                self.x = a[0];
                self.y = 0.0;
                self.stack.clear();
            }
            14 => {
                // endchar
                return Flow::Stop;
            }
            21 => {
                // rmoveto: dx dy
                let a = self.arg_last(2);
                if self.in_flex {
                    self.flex_pts.push((self.x + a[0], self.y + a[1]));
                    self.x += a[0];
                    self.y += a[1];
                } else {
                    self.move_to(self.x + a[0], self.y + a[1]);
                }
                self.stack.clear();
            }
            22 => {
                // hmoveto: dx
                let dx = self.arg_last(1)[0];
                if self.in_flex {
                    self.flex_pts.push((self.x + dx, self.y));
                    self.x += dx;
                } else {
                    self.move_to(self.x + dx, self.y);
                }
                self.stack.clear();
            }
            30 => {
                // vhcurveto: dy1 dx2 dy2 dx3
                let a = self.arg_last(4);
                self.rel_curve(0.0, a[0], a[1], a[2], a[3], 0.0);
                self.stack.clear();
            }
            31 => {
                // hvcurveto: dx1 dx2 dy2 dy3
                let a = self.arg_last(4);
                self.rel_curve(a[0], 0.0, a[1], a[2], 0.0, a[3]);
                self.stack.clear();
            }
            _ => {
                // Unknown / unsupported single-byte op: drop operands, continue.
                self.stack.clear();
            }
        }
        Flow::Continue
    }

    /// Two-byte (escape, `12 x`) operators.
    fn op_escape(&mut self, op: u8) -> Flow {
        match op {
            0 => {
                // dotsection: deprecated hint — ignore.
                self.stack.clear();
            }
            1 | 2 => {
                // vstem3 / hstem3: hints — ignore.
                self.stack.clear();
            }
            6 => {
                // seac: asb adx ady bchar achar (accented composite).
                let a = self.arg_last(5);
                self.seac(a[0], a[1], a[2], a[3] as u8, a[4] as u8);
                return Flow::Stop;
            }
            7 => {
                // sbw: sbx sby wx wy — set the 2D left sidebearing start point.
                let a = self.arg_last(4);
                self.sbx = a[0];
                self.x = a[0];
                self.y = a[1];
                self.stack.clear();
            }
            12 => {
                // div: num1 num2 div
                let b = self.stack.pop().unwrap_or(1.0);
                let a = self.stack.pop().unwrap_or(0.0);
                self.stack.push(if b != 0.0 { a / b } else { 0.0 });
            }
            16 => {
                // callothersubr: arg1..argn n othersubr#
                self.call_othersubr();
            }
            17 => {
                // pop: push a value from the PS stack back to the operand stack.
                let v = self.ps_stack.pop().unwrap_or(0.0);
                self.stack.push(v);
            }
            33 => {
                // setcurrentpoint: x y
                let a = self.arg_last(2);
                self.x = a[0];
                self.y = a[1];
                self.stack.clear();
            }
            _ => {
                self.stack.clear();
            }
        }
        Flow::Continue
    }

    /// Emits a relative cubic from the current point with the 3 delta pairs.
    fn rel_curve(&mut self, dx1: f32, dy1: f32, dx2: f32, dy2: f32, dx3: f32, dy3: f32) {
        let x1 = self.x + dx1;
        let y1 = self.y + dy1;
        let x2 = x1 + dx2;
        let y2 = y1 + dy2;
        let x3 = x2 + dx3;
        let y3 = y2 + dy3;
        self.curve_to(x1, y1, x2, y2, x3, y3);
    }

    /// The Type1 `OtherSubrs` protocol (`callothersubr`): the only entries that
    /// affect the outline are 0/1/2 (flex) and 3 (hint replacement). Others are
    /// no-ops here; their declared return values are pushed to the PS stack so a
    /// following `pop` reads them back (we echo the arguments through).
    fn call_othersubr(&mut self) {
        let othersubr = self.stack.pop().unwrap_or(0.0) as i32;
        let n = self.stack.pop().unwrap_or(0.0).max(0.0) as usize;
        let mut args = Vec::with_capacity(n);
        for _ in 0..n {
            args.push(self.stack.pop().unwrap_or(0.0));
        }
        // `args` is now in reverse (last-pushed first); restore call order.
        args.reverse();

        match othersubr {
            1 => {
                // Start flex: collect the next 7 rmoveto reference points.
                self.in_flex = true;
                self.flex_pts.clear();
            }
            0 => {
                // End flex: emit two curves through the 7 collected points.
                self.end_flex();
                // OtherSubr 0 returns the final (x, y) for two following `pop`s.
                self.ps_stack.push(self.y);
                self.ps_stack.push(self.x);
            }
            2 => {
                // Flex point collection step: handled by the rmoveto capture.
            }
            3 => {
                // Hint replacement: returns the subr# (arg) for a following pop.
                let subr = args.first().copied().unwrap_or(3.0);
                self.ps_stack.push(subr);
            }
            _ => {
                // Unknown OtherSubr: echo args back for any following pops.
                for v in args.into_iter().rev() {
                    self.ps_stack.push(v);
                }
            }
        }
    }

    /// Completes a flex sequence: the 7 collected points are a reference point
    /// plus two Bézier control triples. Emit them as two cubic curves.
    fn end_flex(&mut self) {
        self.in_flex = false;
        // flex_pts[0] is the reference point; [1..7] are the two curves' points.
        if self.flex_pts.len() >= 7 {
            let p = self.flex_pts.clone();
            // Curve 1: control p1,p2 end p3; Curve 2: control p4,p5 end p6.
            self.curve_to(p[1].0, p[1].1, p[2].0, p[2].1, p[3].0, p[3].1);
            self.curve_to(p[4].0, p[4].1, p[5].0, p[5].1, p[6].0, p[6].1);
        } else if let Some(last) = self.flex_pts.last().copied() {
            // Degenerate flex: just line to the last reference point.
            self.line_to(last.0, last.1);
        }
        self.flex_pts.clear();
    }

    /// `seac`: render a base glyph + an accent glyph offset by `(adx-asb+sbx, ady)`.
    fn seac(&mut self, asb: f32, adx: f32, ady: f32, bchar: u8, achar: u8) {
        if self.open {
            self.builder.close();
            self.open = false;
        }
        let base_sbx = self.sbx;
        if let Some(base) = self.font.charstring_by_std_code(bchar) {
            let base = base.to_vec();
            let mut sub = Interp::new(self.font, self.builder);
            sub.run(&base);
            if sub.open {
                sub.builder.close();
            }
            if sub.any_contour {
                self.any_contour = true;
            }
        }
        if let Some(acc) = self.font.charstring_by_std_code(achar) {
            let acc = acc.to_vec();
            let mut sub = OffsetBuilder {
                inner: self.builder,
                dx: base_sbx - asb + adx,
                dy: ady,
            };
            let mut exec = Interp::new(self.font, &mut sub);
            exec.run(&acc);
            if exec.open {
                exec.builder.close();
            }
            if exec.any_contour {
                self.any_contour = true;
            }
        }
    }

    /// Returns the last `n` stack operands (front-padded with 0 if too few),
    /// reading them in push order.
    fn arg_last(&self, n: usize) -> Vec<f32> {
        let len = self.stack.len();
        let mut out = vec![0.0; n];
        let take = n.min(len);
        for k in 0..take {
            out[n - take + k] = self.stack[len - take + k];
        }
        out
    }
}

/// Adapts an [`OutlineBuilder`] to translate every emitted point by `(dx, dy)` —
/// used to place a `seac` accent component over the base glyph.
struct OffsetBuilder<'b> {
    inner: &'b mut dyn OutlineBuilder,
    dx: f32,
    dy: f32,
}

impl OutlineBuilder for OffsetBuilder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.inner.move_to(x + self.dx, y + self.dy);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.inner.line_to(x + self.dx, y + self.dy);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.inner
            .quad_to(x1 + self.dx, y1 + self.dy, x + self.dx, y + self.dy);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.inner.curve_to(
            x1 + self.dx,
            y1 + self.dy,
            x2 + self.dx,
            y2 + self.dy,
            x + self.dx,
            y + self.dy,
        );
    }
    fn close(&mut self) {
        self.inner.close();
    }
}

/// Control flow result of interpreting a charstring fragment.
enum Flow {
    /// Keep interpreting the caller's remaining bytes.
    Continue,
    /// `return` — pop one level of `callsubr` recursion.
    Return,
    /// `endchar` / `seac` / guard trip — stop the whole glyph.
    Stop,
}

// ----- container / decryption parsing ------------------------------------

/// Strips PFB segment framing if present. PFB wraps the program in records, each
/// `0x80 <type> <len:u32-le> <data>`; type 1 = ASCII, type 2 = binary, type 3 =
/// EOF. A PFA / already-flat program (no `0x80` lead byte) is returned as-is.
fn unwrap_pfb(data: &[u8]) -> Vec<u8> {
    if data.first() != Some(&0x80) {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i + 1 < data.len() && data[i] == 0x80 {
        let kind = data[i + 1];
        if kind == 3 {
            break; // EOF record.
        }
        if i + 6 > data.len() {
            break;
        }
        let len = u32::from_le_bytes([data[i + 2], data[i + 3], data[i + 4], data[i + 5]]) as usize;
        let start = i + 6;
        let end = start.saturating_add(len).min(data.len());
        out.extend_from_slice(&data[start..end]);
        i = end;
    }
    out
}

/// Splits a Type1 program at `eexec`: returns the cleartext prefix and the bytes
/// of the encrypted private portion (binary, with any ASCII-hex form decoded).
fn split_eexec(data: &[u8]) -> Option<(&[u8], Vec<u8>)> {
    let pos = find(data, b"eexec")?;
    let clear = &data[..pos];
    let mut j = pos + 5;
    // Skip the whitespace separating `eexec` from the encrypted data.
    while j < data.len() && matches!(data[j], b' ' | b'\r' | b'\n' | b'\t') {
        j += 1;
    }
    let enc = &data[j..];
    // The encrypted block is either raw binary or ASCII-hex. Detect hex: the
    // first 4 bytes are all hex digits.
    let is_hex = enc.len() >= 4 && enc[..4].iter().all(|b| b.is_ascii_hexdigit());
    let bytes = if is_hex {
        hex_decode(enc)
    } else {
        enc.to_vec()
    };
    Some((clear, bytes))
}

/// eexec / charstring decryption (Adobe Type1 spec §7): a simple stream cipher.
fn eexec_decrypt(cipher: &[u8], r0: u16) -> Vec<u8> {
    let mut r = r0;
    let mut out = Vec::with_capacity(cipher.len());
    for &c in cipher {
        let p = c ^ (r >> 8) as u8;
        r = (u16::from(c).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        out.push(p);
    }
    out
}

/// Decrypts a single charstring (R=4330) and drops the `len_iv` lead bytes.
fn decrypt_charstring(cipher: &[u8], len_iv: usize) -> Vec<u8> {
    let dec = eexec_decrypt(cipher, CHARSTRING_R);
    if dec.len() > len_iv {
        dec[len_iv..].to_vec()
    } else {
        Vec::new()
    }
}

/// Reads `/lenIV` from the private dict (default 4).
fn parse_len_iv(priv_dict: &[u8]) -> usize {
    if let Some(p) = find(priv_dict, b"/lenIV") {
        let rest = &priv_dict[p + 6..];
        if let Some(n) = read_int(rest) {
            if n >= 0 {
                return n as usize;
            }
        }
    }
    4
}

/// Parses `/Subrs N array_def` then the `dup i len RD <bytes> NP` entries.
fn parse_subrs(priv_dict: &[u8], len_iv: usize) -> Vec<Vec<u8>> {
    let Some(start) = find(priv_dict, b"/Subrs") else {
        return Vec::new();
    };
    let after = &priv_dict[start + 6..];
    let count = read_int(after).unwrap_or(0).max(0) as usize;
    if count == 0 {
        return Vec::new();
    }
    let mut subrs: Vec<Vec<u8>> = vec![Vec::new(); count];
    // Walk `dup <i> <len> <RD|-|> <binary>` entries.
    let mut data = after;
    let mut found = 0usize;
    while found < count {
        let Some(dp) = find(data, b"dup ") else { break };
        let entry = &data[dp + 4..];
        let Some((idx, rest)) = read_int_adv(entry) else {
            break;
        };
        let Some((len, rest)) = read_int_adv(rest) else {
            break;
        };
        let Some(bin_start) = rd_binary_start(rest) else {
            break;
        };
        let len = len.max(0) as usize;
        if bin_start + len > rest.len() {
            break;
        }
        let cipher = &rest[bin_start..bin_start + len];
        if let (Ok(i), true) = (usize::try_from(idx), idx >= 0) {
            if i < subrs.len() {
                subrs[i] = decrypt_charstring(cipher, len_iv);
                found += 1;
            }
        }
        // Advance past this entry's binary payload.
        let consumed = (rest.as_ptr() as usize - data.as_ptr() as usize) + bin_start + len;
        if consumed >= data.len() {
            break;
        }
        data = &data[consumed..];
    }
    subrs
}

/// Parses the `/CharStrings N dict dup begin` block into name → charstring.
fn parse_charstrings(priv_dict: &[u8], len_iv: usize) -> HashMap<String, Vec<u8>> {
    let mut map = HashMap::new();
    let Some(start) = find(priv_dict, b"/CharStrings") else {
        return map;
    };
    // Entries look like `/name len RD <binary> ND`. Scan from the first `begin`.
    let begin = find(&priv_dict[start..], b"begin").map(|b| start + b + 5);
    let mut data = &priv_dict[begin.unwrap_or(start)..];

    // Find the next `/name`; stop when no further glyph entry remains.
    while let Some(slash) = data.iter().position(|&b| b == b'/') {
        let name_start = slash + 1;
        // Read the glyph name token (until whitespace).
        let mut k = name_start;
        while k < data.len() && !is_ps_delim(data[k]) {
            k += 1;
        }
        if k == name_start {
            data = &data[name_start..];
            continue;
        }
        let name = String::from_utf8_lossy(&data[name_start..k]).into_owned();
        let rest = &data[k..];
        // Read `len RD`.
        let Some((len, rest2)) = read_int_adv(rest) else {
            data = &data[k..];
            continue;
        };
        let Some(bin_start) = rd_binary_start(rest2) else {
            data = &data[k..];
            continue;
        };
        let len = len.max(0) as usize;
        if bin_start + len > rest2.len() {
            break;
        }
        let cipher = &rest2[bin_start..bin_start + len];
        let cs = decrypt_charstring(cipher, len_iv);
        if !cs.is_empty() {
            map.insert(name, cs);
        }
        // Advance past the binary payload.
        let consumed = (rest2.as_ptr() as usize - data.as_ptr() as usize) + bin_start + len;
        if consumed >= data.len() {
            break;
        }
        data = &data[consumed..];
    }
    map
}

/// Recovers the design grid size from the cleartext `/FontMatrix [sx ...]`.
fn parse_font_matrix_upem(clear: &[u8]) -> u16 {
    let Some(p) = find(clear, b"/FontMatrix") else {
        return 1000;
    };
    let rest = &clear[p..];
    let Some(open) = rest.iter().position(|&b| b == b'[') else {
        return 1000;
    };
    let after = &rest[open + 1..];
    if let Some(sx) = read_float(after) {
        if sx.is_finite() && sx.abs() > f32::EPSILON {
            let upem = (1.0 / sx.abs()).round();
            if (1.0..=f32::from(u16::MAX)).contains(&upem) {
                return upem as u16;
            }
        }
    }
    1000
}

/// Parses the cleartext builtin `/Encoding` array (P4-2r): the entries are
/// `dup <code> /<name> put` statements between `/Encoding` and the following
/// `readonly def` / `def`. Returns the dense code → glyph-name table, or `None`
/// when the program declares a named predefined encoding (e.g.
/// `/Encoding StandardEncoding def`) with no `dup ... put` overrides — those are
/// covered by the caller's StandardEncoding + AGL path, so there is nothing
/// font-specific to record.
fn parse_builtin_encoding(clear: &[u8]) -> Option<Box<[Option<String>; 256]>> {
    let start = find(clear, b"/Encoding")?;
    // Bound the scan to this /Encoding statement: stop at the terminating `def`
    // so we never wander into unrelated `dup`s elsewhere in the cleartext.
    let region = &clear[start..];
    let end = find(region, b" def").unwrap_or(region.len());
    let region = &region[..end];

    let mut table: [Option<String>; 256] = std::array::from_fn(|_| None);
    let mut any = false;
    let mut data = region;
    // Each override is `dup <code> /<name> put`.
    while let Some(dp) = find(data, b"dup ") {
        let after = &data[dp + 4..];
        let Some((code, rest)) = read_int_adv(after) else {
            data = &data[dp + 4..];
            continue;
        };
        // The glyph name token: a `/name` literal.
        let rest = skip_ws(rest);
        if rest.first() != Some(&b'/') {
            data = rest;
            continue;
        }
        let name_bytes = &rest[1..];
        let n = name_bytes
            .iter()
            .position(|&b| is_ps_delim(b))
            .unwrap_or(name_bytes.len());
        let advance = 1 + n;
        if n > 0 && (0..=255).contains(&code) {
            let name = String::from_utf8_lossy(&name_bytes[..n]).into_owned();
            table[code as usize] = Some(name);
            any = true;
        }
        data = &rest[advance..];
    }
    any.then(|| Box::new(table))
}

/// Skips leading ASCII whitespace.
fn skip_ws(data: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    &data[i..]
}

// ----- tiny byte-scanning helpers ----------------------------------------

/// Whether `b` is a Type1 / PostScript token delimiter.
fn is_ps_delim(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'(' | b')' | b'[' | b']' | b'{' | b'}'
    )
}

/// Finds the first occurrence of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads the first signed integer in `data` (skipping leading whitespace),
/// returning its value only.
fn read_int(data: &[u8]) -> Option<i64> {
    read_int_adv(data).map(|(v, _)| v)
}

/// Reads the first signed integer in `data` (skipping leading whitespace),
/// returning the value and the slice *after* the integer.
fn read_int_adv(data: &[u8]) -> Option<(i64, &[u8])> {
    let mut i = 0;
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    if i < data.len() && (data[i] == b'-' || data[i] == b'+') {
        i += 1;
    }
    let digits = i;
    while i < data.len() && data[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits {
        return None;
    }
    let s = std::str::from_utf8(&data[start..i]).ok()?;
    let v: i64 = s.parse().ok()?;
    Some((v, &data[i..]))
}

/// Reads the first float in `data` (skipping leading whitespace).
fn read_float(data: &[u8]) -> Option<f32> {
    let mut i = 0;
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    if i < data.len() && (data[i] == b'-' || data[i] == b'+') {
        i += 1;
    }
    while i < data.len()
        && (data[i].is_ascii_digit()
            || data[i] == b'.'
            || data[i] == b'e'
            || data[i] == b'E'
            || data[i] == b'-'
            || data[i] == b'+')
    {
        i += 1;
    }
    let s = std::str::from_utf8(&data[start..i]).ok()?;
    s.parse().ok()
}

/// After an integer length, an entry uses a binary-introducer token
/// (`RD`, `-|`, or similar) followed by exactly one space, then the binary
/// bytes. Returns the offset of the first binary byte within `data`.
fn rd_binary_start(data: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    // The introducer token: `RD` or `-|` (procedure-defined); read until space.
    let tok_start = i;
    while i < data.len() && data[i] != b' ' {
        i += 1;
    }
    if i == tok_start || i >= data.len() {
        return None;
    }
    // Exactly one space separates the token from the binary data.
    Some(i + 1)
}

/// Decodes an ASCII-hex eexec block until a non-hex byte (e.g. the trailing
/// zeros / `cleartomark`).
fn hex_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut hi: Option<u8> = None;
    for &b in data {
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => break,
        };
        match hi.take() {
            None => hi = Some(v),
            Some(h) => out.push((h << 4) | v),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects the emitted outline into counts so a test can assert a glyph
    /// produced real contours (not a blank), plus the bounding box.
    #[derive(Default)]
    struct CountBuilder {
        moves: u32,
        lines: u32,
        curves: u32,
        closes: u32,
        min: (f32, f32),
        max: (f32, f32),
        seen: bool,
    }

    impl CountBuilder {
        fn track(&mut self, x: f32, y: f32) {
            if !self.seen {
                self.min = (x, y);
                self.max = (x, y);
                self.seen = true;
            } else {
                self.min.0 = self.min.0.min(x);
                self.min.1 = self.min.1.min(y);
                self.max.0 = self.max.0.max(x);
                self.max.1 = self.max.1.max(y);
            }
        }
        fn drawn(&self) -> bool {
            self.moves > 0 && (self.lines + self.curves) > 0
        }
    }

    impl OutlineBuilder for CountBuilder {
        fn move_to(&mut self, x: f32, y: f32) {
            self.moves += 1;
            self.track(x, y);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.lines += 1;
            self.track(x, y);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            self.curves += 1;
            self.track(x1, y1);
            self.track(x, y);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.curves += 1;
            self.track(x1, y1);
            self.track(x2, y2);
            self.track(x, y);
        }
        fn close(&mut self) {
            self.closes += 1;
        }
    }

    /// Encodes a Type1 charstring integer using the operand encoding the
    /// interpreter decodes (the inverse of [`Interp::run`]'s number path).
    fn enc_int(out: &mut Vec<u8>, v: i32) {
        if (-107..=107).contains(&v) {
            out.push((v + 139) as u8);
        } else if (108..=1131).contains(&v) {
            let v = v - 108;
            out.push((v / 256 + 247) as u8);
            out.push((v % 256) as u8);
        } else if (-1131..=-108).contains(&v) {
            let v = -v - 108;
            out.push((v / 256 + 251) as u8);
            out.push((v % 256) as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }

    /// Encrypts a charstring (R=4330): prepend `len_iv` lead bytes, then encrypt.
    fn encrypt_cs(plain: &[u8], len_iv: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len_iv];
        buf.extend_from_slice(plain);
        encrypt(&buf, CHARSTRING_R)
    }

    /// The encryption inverse of [`eexec_decrypt`] (same cipher, plaintext in).
    fn encrypt(plain: &[u8], r0: u16) -> Vec<u8> {
        let mut r = r0;
        let mut out = Vec::with_capacity(plain.len());
        for &p in plain {
            let c = p ^ (r >> 8) as u8;
            r = (u16::from(c).wrapping_add(r))
                .wrapping_mul(C1)
                .wrapping_add(C2);
            out.push(c);
        }
        out
    }

    /// Builds a minimal, self-contained Type1 program (PFA / flat, no PFB frame)
    /// embedding the given `(name, charstring-plaintext)` glyphs at `upem=1000`.
    /// The private dict is eexec-encrypted exactly as a real `/FontFile`.
    fn build_type1(glyphs: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let len_iv = 4usize;
        // Private-dict cleartext: /lenIV, an empty /Subrs, then /CharStrings.
        let mut priv_clear = Vec::new();
        // 4 random lead bytes (decoder drops them) — any bytes work.
        priv_clear.extend_from_slice(b"0000");
        priv_clear.extend_from_slice(b"dup /Private 1 dict dup begin\n");
        priv_clear.extend_from_slice(b"/lenIV 4 def\n");
        priv_clear.extend_from_slice(b"/Subrs 0 array\n");
        priv_clear.extend_from_slice(
            format!("/CharStrings {} dict dup begin\n", glyphs.len()).as_bytes(),
        );
        for (name, cs) in glyphs {
            let enc = encrypt_cs(cs, len_iv);
            priv_clear.extend_from_slice(format!("/{name} {} RD ", enc.len()).as_bytes());
            priv_clear.extend_from_slice(&enc);
            priv_clear.extend_from_slice(b" ND\n");
        }
        priv_clear.extend_from_slice(b"end\nend\n");

        let enc_priv = encrypt(&priv_clear, EEXEC_R);

        let mut out = Vec::new();
        out.extend_from_slice(b"%!FontType1-1.0: Synthetic\n");
        out.extend_from_slice(b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n");
        out.extend_from_slice(b"currentfile eexec\n");
        out.extend_from_slice(&enc_priv);
        out.extend_from_slice(b"\n0000000000000000\ncleartomark\n");
        out
    }

    /// Like [`build_type1`] but emits a custom builtin `/Encoding` array of
    /// `(code, glyph-name)` overrides in the cleartext header (P4-2r).
    fn build_type1_with_encoding(glyphs: &[(&str, Vec<u8>)], encoding: &[(u8, &str)]) -> Vec<u8> {
        let base = build_type1(glyphs);
        // Splice the encoding array into the cleartext, before `currentfile eexec`.
        let mut enc = Vec::new();
        enc.extend_from_slice(b"/Encoding 256 array\n");
        enc.extend_from_slice(b"0 1 255 {1 index exch /.notdef put} for\n");
        for (code, name) in encoding {
            enc.extend_from_slice(format!("dup {code} /{name} put\n").as_bytes());
        }
        enc.extend_from_slice(b"readonly def\n");

        let marker = b"currentfile eexec\n";
        let pos = super::find(&base, marker).expect("eexec marker present");
        let mut out = Vec::with_capacity(base.len() + enc.len());
        out.extend_from_slice(&base[..pos]);
        out.extend_from_slice(&enc);
        out.extend_from_slice(&base[pos..]);
        out
    }

    /// A box glyph charstring: `hsbw`, `rmoveto`, 3 `rlineto`, `closepath`,
    /// `endchar`. Draws a `w`×`h` box at sidebearing `sb`.
    fn box_charstring(sb: i32, w: i32, h: i32) -> Vec<u8> {
        let mut cs = Vec::new();
        enc_int(&mut cs, sb); // sbx
        enc_int(&mut cs, w + 2 * sb); // wx
        cs.push(13); // hsbw
        enc_int(&mut cs, 0); // dx (already at sbx from hsbw)
        enc_int(&mut cs, 0); // dy
        cs.push(21); // rmoveto → start at (sb, 0)
        enc_int(&mut cs, w);
        enc_int(&mut cs, 0);
        cs.push(5); // rlineto → (sb+w, 0)
        enc_int(&mut cs, 0);
        enc_int(&mut cs, h);
        cs.push(5); // rlineto → (sb+w, h)
        enc_int(&mut cs, -w);
        enc_int(&mut cs, 0);
        cs.push(5); // rlineto → (sb, h)
        cs.push(9); // closepath
        cs.push(14); // endchar
        cs
    }

    /// TYPE1-PARSE-001: a synthetic `/FontFile` parses, exposes its named glyph,
    /// and its charstring outlines into a real, non-empty contour at the right
    /// box extent — proving eexec + per-charstring decrypt + the interpreter's
    /// hsbw / rmoveto / rlineto / closepath / endchar path all work end-to-end.
    #[test]
    fn type1_parse_001_box_glyph_outlines_nonempty() {
        let prog = build_type1(&[("A", box_charstring(50, 600, 700))]);
        let font = Type1Font::parse(&prog).expect("synthetic Type1 must parse");
        assert_eq!(font.units_per_em(), 1000);
        let gid = font.glyph_for_name("A").expect("glyph A resolves by name");

        let mut b = CountBuilder::default();
        assert!(font.outline(gid, &mut b), "outline reports drawn contour");
        assert!(b.drawn(), "box glyph emits a move + lines (not blank)");
        assert_eq!(b.closes, 1, "one closed contour");
        // Box spans x∈[50,650], y∈[0,700] in font units.
        assert!((b.min.0 - 50.0).abs() < 1.0, "left = sidebearing 50");
        assert!((b.max.0 - 650.0).abs() < 1.0, "right = 50+600");
        assert!((b.max.1 - 700.0).abs() < 1.0, "top = 700");
    }

    /// TYPE1-PARSE-002: the `seac` accent composite renders BOTH the base and the
    /// (offset) accent glyph by StandardEncoding code, so the composed glyph inks
    /// more than either component alone.
    #[test]
    fn type1_parse_002_seac_composes_base_and_accent() {
        // StandardEncoding: code 0x41='A', 0xC2=acute ('acute' accent).
        let base = box_charstring(50, 400, 700);
        let accent = box_charstring(0, 200, 150);
        // seac charstring: hsbw, then `asb adx ady bchar achar seac`.
        let mut comp = Vec::new();
        enc_int(&mut comp, 0); // sbx
        enc_int(&mut comp, 500); // wx
        comp.push(13); // hsbw
        enc_int(&mut comp, 50); // asb (accent sidebearing basis)
        enc_int(&mut comp, 100); // adx
        enc_int(&mut comp, 600); // ady (place accent high)
        enc_int(&mut comp, 0x41); // bchar = 'A'
        enc_int(&mut comp, 0xC2); // achar = 'acute'
        comp.push(12);
        comp.push(6); // seac

        let prog = build_type1(&[("A", base), ("acute", accent), ("Aacute", comp)]);
        let font = Type1Font::parse(&prog).expect("parse");
        let gid = font.glyph_for_name("Aacute").expect("Aacute resolves");
        let mut b = CountBuilder::default();
        assert!(font.outline(gid, &mut b), "composite outlines");
        // Base (1 move) + accent (1 move) → 2 contours.
        assert_eq!(b.moves, 2, "seac draws base + accent contours");
        assert_eq!(b.closes, 2, "both components closed");
        // The accent sits high (ady=600) so the composite is taller than the base
        // box alone (top 700 + accent height 150 region above the base).
        assert!(b.max.1 > 700.0, "accent extends above the base box");
    }

    /// TYPE1-ENC-001 (P4-2r): a Type1 program whose builtin `/Encoding` maps a
    /// code to a *non-AGL* glyph name resolves that code → glyph via the parsed
    /// builtin encoding (the name is not derivable from AGL / Unicode).
    #[test]
    fn type1_builtin_encoding_resolves_code() {
        // Glyph "ornament" is not an AGL name; only the builtin /Encoding (code
        // 0x61 → /ornament) connects code 0x61 to its outline.
        let prog = build_type1_with_encoding(
            &[("ornament", box_charstring(0, 500, 500))],
            &[(0x61, "ornament")],
        );
        let font = Type1Font::parse(&prog).expect("parse with custom encoding");

        // The builtin encoding resolves code 0x61 → the ornament GID.
        let gid = font
            .glyph_for_code(0x61)
            .expect("code 0x61 maps via builtin");
        assert_eq!(font.glyph_for_name("ornament"), Some(gid));
        // An unassigned code yields nothing.
        assert_eq!(font.glyph_for_code(0x62), None);

        // The resolved glyph outlines a real contour.
        let mut b = CountBuilder::default();
        assert!(font.outline(gid, &mut b), "ornament outlines");
        assert!(b.drawn(), "builtin-encoded glyph is not blank");
    }

    /// A program with no custom `/Encoding` array (named predefined encoding)
    /// records no builtin table — the caller's StandardEncoding+AGL path covers
    /// it, so `glyph_for_code` returns `None` rather than a spurious match.
    #[test]
    fn type1_named_encoding_has_no_builtin_table() {
        let prog = build_type1(&[("A", box_charstring(0, 400, 700))]);
        let font = Type1Font::parse(&prog).expect("parse");
        assert_eq!(font.glyph_for_code(0x41), None);
        // The glyph is still reachable by name (the normal path).
        assert!(font.glyph_for_name("A").is_some());
    }
}
