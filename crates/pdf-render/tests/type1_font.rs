//! `TYPE1-*` — first-party Adobe Type1 (`/FontFile`) outliner tests.
//!
//! These drive the public `pdf_render::type1::Type1Font` (`parse` / `outline` /
//! `units_per_em` / `glyph_for_name` / `glyph_for_code`) through a small in-code
//! Type1 program generator that mirrors the private encrypt/build helpers of the
//! module's inline `#[cfg(test)]` module (`enc_int` / `encrypt` / `encrypt_cs` /
//! `build_type1`) and of `tests/synth::type1()`. Every charstring is authored as
//! raw operator bytes so the tests exercise the interpreter operators (hsbw / sbw
//! / rlineto / hlineto / vlineto / rrcurveto / vh/hv-curveto / rmoveto / hmoveto /
//! vmoveto / closepath / callsubr / return / callothersubr flex + hint-replace /
//! div / setcurrentpoint / seac) plus the container plumbing (PFB unwrap, ASCII-hex
//! eexec, `/Subrs`, `/lenIV`, `/FontMatrix`, builtin `/Encoding`) and the failure
//! branches (truncated eexec, empty CharStrings, garbage input, out-of-range gid).
//!
//! The generator is license-clean (authored in-code, no font asset, no network).

use ttf_parser::OutlineBuilder;

use pdf_render::type1::Type1Font;

// === cipher / operand encoders (inverse of the module's decrypt path) =======

const C1: u16 = 52845;
const C2: u16 = 22719;
const CHARSTRING_R: u16 = 4330;
const EEXEC_R: u16 = 55665;

/// The encryption inverse of the module's `eexec_decrypt` (same stream cipher).
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

/// Encrypts a charstring (R=4330): prepend `len_iv` lead bytes, then encrypt.
fn encrypt_cs(plain: &[u8], len_iv: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len_iv];
    buf.extend_from_slice(plain);
    encrypt(&buf, CHARSTRING_R)
}

/// Encodes a Type1 charstring integer using the operand encoding the interpreter
/// decodes (the inverse of the `run` number path, incl. the 255 32-bit form).
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

/// Finds the first occurrence of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// === a flexible Type1 program builder =======================================

/// A synthetic, self-contained Type1 (`/FontFile`) program builder. Defaults to a
/// flat/PFA program with `/FontMatrix [0.001 …]` (upem 1000) and `/lenIV 4`; the
/// setters let each test override the pieces it needs to exercise.
struct T1 {
    /// The `/FontMatrix` array literal (with brackets), or a bracket-less / absent
    /// variant to exercise the upem-parse fallbacks. `None` omits `/FontMatrix`.
    font_matrix: Option<String>,
    /// A verbatim cleartext `/Encoding … def` block (inserted in the header).
    encoding_block: Option<String>,
    /// The `/lenIV` declaration literal (e.g. `"4"`, `"-1"`); `None` omits the line.
    len_iv_decl: Option<String>,
    /// The `len_iv` actually used to encrypt charstrings (the value the parser
    /// resolves — 4 when the declaration is absent / rejected).
    len_iv_actual: usize,
    /// Plaintext `/Subrs` charstrings (index = position).
    subrs: Vec<Vec<u8>>,
    /// Plaintext `/CharStrings` glyphs (name, charstring).
    glyphs: Vec<(String, Vec<u8>)>,
    /// Emit the eexec block as ASCII-hex rather than raw binary.
    hex: bool,
    /// Wrap the flat program in PFB (`0x80`) segment framing.
    pfb: bool,
}

impl T1 {
    fn new() -> Self {
        T1 {
            font_matrix: Some("[0.001 0 0 0.001 0 0]".to_owned()),
            encoding_block: None,
            len_iv_decl: Some("4".to_owned()),
            len_iv_actual: 4,
            subrs: Vec::new(),
            glyphs: Vec::new(),
            hex: false,
            pfb: false,
        }
    }

    fn glyph(mut self, name: &str, cs: Vec<u8>) -> Self {
        self.glyphs.push((name.to_owned(), cs));
        self
    }

    fn subr(mut self, cs: Vec<u8>) -> Self {
        self.subrs.push(cs);
        self
    }

    fn font_matrix(mut self, fm: &str) -> Self {
        self.font_matrix = Some(fm.to_owned());
        self
    }

    fn no_font_matrix(mut self) -> Self {
        self.font_matrix = None;
        self
    }

    fn encoding_block(mut self, block: String) -> Self {
        self.encoding_block = Some(block);
        self
    }

    fn len_iv_decl(mut self, decl: Option<&str>) -> Self {
        self.len_iv_decl = decl.map(str::to_owned);
        self
    }

    fn hex(mut self) -> Self {
        self.hex = true;
        self
    }

    fn pfb(mut self) -> Self {
        self.pfb = true;
        self
    }

    fn build(&self) -> Vec<u8> {
        let liv = self.len_iv_actual;
        // Private-dict cleartext: 4 lead bytes, then /lenIV, /Subrs, /CharStrings.
        let mut pc = Vec::new();
        pc.extend_from_slice(b"0000");
        pc.extend_from_slice(b"dup /Private 1 dict dup begin\n");
        if let Some(decl) = &self.len_iv_decl {
            pc.extend_from_slice(format!("/lenIV {decl} def\n").as_bytes());
        }
        pc.extend_from_slice(format!("/Subrs {} array\n", self.subrs.len()).as_bytes());
        for (i, s) in self.subrs.iter().enumerate() {
            let enc = encrypt_cs(s, liv);
            pc.extend_from_slice(format!("dup {i} {} RD ", enc.len()).as_bytes());
            pc.extend_from_slice(&enc);
            pc.extend_from_slice(b" NP\n");
        }
        pc.extend_from_slice(
            format!("/CharStrings {} dict dup begin\n", self.glyphs.len()).as_bytes(),
        );
        for (name, cs) in &self.glyphs {
            let enc = encrypt_cs(cs, liv);
            pc.extend_from_slice(format!("/{name} {} RD ", enc.len()).as_bytes());
            pc.extend_from_slice(&enc);
            pc.extend_from_slice(b" ND\n");
        }
        pc.extend_from_slice(b"end\nend\n");
        let enc_priv = encrypt(&pc, EEXEC_R);

        // Cleartext header.
        let mut header = Vec::new();
        header.extend_from_slice(b"%!FontType1-1.0: Synthetic\n");
        if let Some(fm) = &self.font_matrix {
            header.extend_from_slice(format!("/FontMatrix {fm} readonly def\n").as_bytes());
        }
        if let Some(block) = &self.encoding_block {
            header.extend_from_slice(block.as_bytes());
        }

        // Assemble the flat program.
        let mut flat = Vec::new();
        flat.extend_from_slice(&header);
        flat.extend_from_slice(b"currentfile eexec\n");
        if self.hex {
            for b in &enc_priv {
                flat.extend_from_slice(format!("{b:02x}").as_bytes());
            }
        } else {
            flat.extend_from_slice(&enc_priv);
        }
        flat.extend_from_slice(b"\n0000000000000000\ncleartomark\n");

        if self.pfb {
            pfb_wrap(&flat)
        } else {
            flat
        }
    }
}

/// Wraps a flat program in PFB framing: an ASCII record (type 1) up to and
/// including `currentfile eexec\n`, a binary record (type 2) for the rest, then
/// an EOF record (type 3). `unwrap_pfb` concatenates the record payloads back.
fn pfb_wrap(flat: &[u8]) -> Vec<u8> {
    let marker = b"currentfile eexec\n";
    let split = find(flat, marker).map_or(flat.len(), |p| p + marker.len());
    let (seg1, seg2) = flat.split_at(split);
    let mut out = Vec::new();
    for (kind, seg) in [(1u8, seg1), (2u8, seg2)] {
        out.push(0x80);
        out.push(kind);
        out.extend_from_slice(&(seg.len() as u32).to_le_bytes());
        out.extend_from_slice(seg);
    }
    out.push(0x80);
    out.push(3); // EOF record.
    out
}

/// A cleartext builtin `/Encoding` array of `(code, name)` overrides.
fn encoding_block(entries: &[(u8, &str)]) -> String {
    let mut s = String::from("/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\n");
    for (c, n) in entries {
        s.push_str(&format!("dup {c} /{n} put\n"));
    }
    s.push_str("readonly def\n");
    s
}

// === charstring authors =====================================================

/// A closed box (`hsbw rmoveto rlineto×3 closepath endchar`) `w`×`h` at sb.
fn box_cs(sb: i32, w: i32, h: i32) -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, sb);
    enc_int(&mut cs, w + 2 * sb);
    cs.push(13); // hsbw
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 0);
    cs.push(21); // rmoveto → (sb, 0)
    enc_int(&mut cs, w);
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto
    enc_int(&mut cs, 0);
    enc_int(&mut cs, h);
    cs.push(5); // rlineto
    enc_int(&mut cs, -w);
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto
    cs.push(9); // closepath
    cs.push(14); // endchar
    cs
}

/// An open box (no closepath): leaves the contour open so `finish`/`seac` close it.
fn open_box_cs(sb: i32, w: i32, h: i32) -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, sb);
    enc_int(&mut cs, w + 2 * sb);
    cs.push(13);
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 0);
    cs.push(21);
    enc_int(&mut cs, w);
    enc_int(&mut cs, 0);
    cs.push(5);
    enc_int(&mut cs, 0);
    enc_int(&mut cs, h);
    cs.push(5);
    enc_int(&mut cs, -w);
    enc_int(&mut cs, 0);
    cs.push(5);
    cs.push(14); // endchar, contour left open
    cs
}

/// An open accent glyph that draws a curve (so a `seac` offset applies to the
/// `OffsetBuilder`'s `curve_to`, not only its `line_to`).
fn accent_curve_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 120);
    cs.push(13); // hsbw
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 0);
    cs.push(21); // rmoveto → (0,0)
    for d in [30, 60, 30, 0, 30, -60] {
        enc_int(&mut cs, d);
    }
    cs.push(8); // rrcurveto (peak at y=60)
    cs.push(14); // endchar (open)
    cs
}

/// A glyph exercising the full line/curve/move operator repertoire.
fn rich_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 700);
    cs.push(13); // hsbw
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 100);
    cs.push(1); // hstem
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 100);
    cs.push(3); // vstem
    enc_int(&mut cs, 100);
    enc_int(&mut cs, 100);
    cs.push(21); // rmoveto → (100,100)
    enc_int(&mut cs, 200);
    cs.push(6); // hlineto → (300,100)
    enc_int(&mut cs, 150);
    cs.push(7); // vlineto → (300,250)
    enc_int(&mut cs, -50);
    enc_int(&mut cs, 50);
    cs.push(5); // rlineto → (250,300)
    for d in [10, 20, 30, 40, 50, 60] {
        enc_int(&mut cs, d);
    }
    cs.push(8); // rrcurveto
    for d in [10, 20, 30, 40] {
        enc_int(&mut cs, d);
    }
    cs.push(30); // vhcurveto
    for d in [10, 20, 30, 40] {
        enc_int(&mut cs, d);
    }
    cs.push(31); // hvcurveto
    cs.push(9); // closepath
    enc_int(&mut cs, 50);
    cs.push(22); // hmoveto → new contour
    enc_int(&mut cs, 40);
    cs.push(7); // vlineto
    cs.push(9); // closepath
    enc_int(&mut cs, 50);
    cs.push(4); // vmoveto → new contour
    enc_int(&mut cs, 40);
    cs.push(6); // hlineto
    cs.push(9); // closepath
    cs.push(14); // endchar
    cs
}

/// Two open contours with no endchar: the second rmoveto closes the first (the
/// `move_to` re-open path), and falling off the end returns `Flow::Continue`; a
/// 2000 coordinate exercises the 255 32-bit operand form. `finish` closes the tail.
fn two_open_contours_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 2500);
    cs.push(13); // hsbw
    enc_int(&mut cs, 50);
    enc_int(&mut cs, 50);
    cs.push(21); // rmoveto (opens)
    enc_int(&mut cs, 2000); // 255 32-bit operand form
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 100);
    cs.push(21); // rmoveto again while open → move_to closes prior contour
    enc_int(&mut cs, 100);
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto (no endchar: run falls off the end)
    cs
}

/// A subr drawing one rlineto then `return`.
fn subr_line() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 100);
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto
    cs.push(11); // return
    cs
}

/// Calls `/Subrs[0]` for its middle segment (covers callsubr + nested return).
fn callsubr_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 500);
    cs.push(13); // hsbw
    enc_int(&mut cs, 50);
    enc_int(&mut cs, 50);
    cs.push(21); // rmoveto
    enc_int(&mut cs, 0);
    cs.push(10); // callsubr 0 → rlineto(100,0)
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 100);
    cs.push(5); // rlineto
    cs.push(9);
    cs.push(14);
    cs
}

/// Hint replacement: `0 1 3 callothersubr pop callsubr` then draw.
fn hint_replace_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 500);
    cs.push(13); // hsbw
    enc_int(&mut cs, 50);
    enc_int(&mut cs, 50);
    cs.push(21); // rmoveto
    enc_int(&mut cs, 0); // subr# arg
    enc_int(&mut cs, 1); // n = 1
    enc_int(&mut cs, 3); // othersubr 3
    cs.push(12);
    cs.push(16); // callothersubr (hint replacement)
    cs.push(12);
    cs.push(17); // pop → subr# to operand stack
    cs.push(10); // callsubr 0
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 100);
    cs.push(5);
    cs.push(9);
    cs.push(14);
    cs
}

/// A flex sequence via OtherSubrs 1 (start) / 2 (collect) / 0 (end) + `pop`s +
/// `setcurrentpoint`. Emits two cubics on the open contour.
fn flex_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 700);
    cs.push(13); // hsbw
    enc_int(&mut cs, 50);
    enc_int(&mut cs, 50);
    cs.push(21); // rmoveto (opens the contour)
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 1);
    cs.push(12);
    cs.push(16); // 0 1 callothersubr → start flex
    for (dx, dy) in [
        (10, 10),
        (10, 10),
        (10, 0),
        (10, -10),
        (10, 10),
        (10, 0),
        (10, -10),
    ] {
        enc_int(&mut cs, dx);
        enc_int(&mut cs, dy);
        cs.push(21); // rmoveto (captured as a flex point)
        enc_int(&mut cs, 0);
        enc_int(&mut cs, 2);
        cs.push(12);
        cs.push(16); // 0 2 callothersubr → collect step (no-op)
    }
    enc_int(&mut cs, 0); // flex height
    enc_int(&mut cs, 120); // end x
    enc_int(&mut cs, 60); // end y
    enc_int(&mut cs, 3); // n = 3
    enc_int(&mut cs, 0);
    cs.push(12);
    cs.push(16); // 3 args, 0 callothersubr → end flex
    cs.push(12);
    cs.push(17); // pop x
    cs.push(12);
    cs.push(17); // pop y
    cs.push(12);
    cs.push(33); // setcurrentpoint
    cs.push(9); // closepath
    cs.push(14); // endchar
    cs
}

/// A malformed (degenerate) flex: only three reference points, captured via
/// `vmoveto` / `hmoveto` / `rmoveto` while in flex — the end-flex step falls back
/// to a straight `line_to` to the last point instead of two cubics.
fn degenerate_flex_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 500);
    cs.push(13); // hsbw
    enc_int(&mut cs, 50);
    enc_int(&mut cs, 50);
    cs.push(21); // rmoveto (opens)
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 1);
    cs.push(12);
    cs.push(16); // start flex
    enc_int(&mut cs, 10);
    cs.push(4); // vmoveto (in-flex capture)
    enc_int(&mut cs, 10);
    cs.push(22); // hmoveto (in-flex capture)
    enc_int(&mut cs, 10);
    enc_int(&mut cs, 10);
    cs.push(21); // rmoveto (in-flex capture) — only 3 points total
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 80);
    enc_int(&mut cs, 80);
    enc_int(&mut cs, 3);
    enc_int(&mut cs, 0);
    cs.push(12);
    cs.push(16); // end flex (degenerate)
    cs.push(12);
    cs.push(17);
    cs.push(12);
    cs.push(17); // pop pop
    cs.push(14); // endchar
    cs
}

/// Misc operators: `sbw`, `div`, `dotsection`, `vstem3`, `hstem3`, an unknown
/// escape op, and an unknown one-byte op — all around a drawn segment.
fn misc_ops_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 500);
    enc_int(&mut cs, 0);
    cs.push(12);
    cs.push(7); // sbw
    enc_int(&mut cs, 50);
    enc_int(&mut cs, 50);
    cs.push(21); // rmoveto
    enc_int(&mut cs, 200);
    enc_int(&mut cs, 2);
    cs.push(12);
    cs.push(12); // div → 100
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto (dx from div)
    cs.push(12);
    cs.push(0); // dotsection
    for d in [0, 10, 0, 10, 0, 10] {
        enc_int(&mut cs, d);
    }
    cs.push(12);
    cs.push(1); // vstem3
    for d in [0, 10, 0, 10, 0, 10] {
        enc_int(&mut cs, d);
    }
    cs.push(12);
    cs.push(2); // hstem3
    enc_int(&mut cs, 5);
    enc_int(&mut cs, 1);
    enc_int(&mut cs, 99);
    cs.push(12);
    cs.push(16); // callothersubr 99 (unknown → args echoed to the PS stack)
    cs.push(12);
    cs.push(17); // pop (reads the echoed arg back)
    cs.push(12);
    cs.push(99); // unknown escape op
    cs.push(25); // unknown one-byte op
    cs.push(9);
    cs.push(14);
    cs
}

/// A `seac` composite whose own contour is still open at the `seac` op (so the
/// interpreter closes it), composing a base + accent that both leave their
/// contour open (so the sub-interpreters close them too).
fn seac_open_cs() -> Vec<u8> {
    let mut cs = Vec::new();
    enc_int(&mut cs, 0);
    enc_int(&mut cs, 500);
    cs.push(13); // hsbw
    enc_int(&mut cs, 10);
    enc_int(&mut cs, 10);
    cs.push(21); // rmoveto (opens a contour)
    enc_int(&mut cs, 20);
    enc_int(&mut cs, 0);
    cs.push(5); // rlineto (open contour at seac entry)
    enc_int(&mut cs, 0); // asb
    enc_int(&mut cs, 100); // adx
    enc_int(&mut cs, 300); // ady
    enc_int(&mut cs, 0x41); // bchar 'A'
    enc_int(&mut cs, 0xC2); // achar 'acute'
    cs.push(12);
    cs.push(6); // seac
    cs
}

// === recording OutlineBuilder ===============================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Move(i32, i32),
    Line(i32, i32),
    Curve(i32, i32, i32, i32, i32, i32),
    Close,
}

#[derive(Default)]
struct Rec {
    segs: Vec<Seg>,
    min: (f32, f32),
    max: (f32, f32),
    seen: bool,
}

impl Rec {
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
    fn count(&self, f: impl Fn(&Seg) -> bool) -> usize {
        self.segs.iter().filter(|s| f(s)).count()
    }
    fn moves(&self) -> usize {
        self.count(|s| matches!(s, Seg::Move(..)))
    }
    fn lines(&self) -> usize {
        self.count(|s| matches!(s, Seg::Line(..)))
    }
    fn curves(&self) -> usize {
        self.count(|s| matches!(s, Seg::Curve(..)))
    }
    fn closes(&self) -> usize {
        self.count(|s| matches!(s, Seg::Close))
    }
    fn drawn(&self) -> bool {
        self.moves() > 0 && (self.lines() + self.curves()) > 0
    }
}

fn r(v: f32) -> i32 {
    v.round() as i32
}

impl OutlineBuilder for Rec {
    fn move_to(&mut self, x: f32, y: f32) {
        self.segs.push(Seg::Move(r(x), r(y)));
        self.track(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.segs.push(Seg::Line(r(x), r(y)));
        self.track(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.segs
            .push(Seg::Curve(r(x1), r(y1), r(x1), r(y1), r(x), r(y)));
        self.track(x1, y1);
        self.track(x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.segs
            .push(Seg::Curve(r(x1), r(y1), r(x2), r(y2), r(x), r(y)));
        self.track(x1, y1);
        self.track(x2, y2);
        self.track(x, y);
    }
    fn close(&mut self) {
        self.segs.push(Seg::Close);
    }
}

/// Parses `data` and outlines glyph `name` into a fresh [`Rec`], returning
/// `(font, rec, drawn)`.
fn outline_named(data: &[u8], name: &str) -> (Type1Font, Rec) {
    let font = Type1Font::parse(data).expect("synthetic Type1 must parse");
    let gid = font.glyph_for_name(name).expect("glyph resolves by name");
    let mut rec = Rec::default();
    let drawn = font.outline(gid, &mut rec);
    assert!(drawn, "glyph `{name}` must report a drawn contour");
    (font, rec)
}

// === tests ==================================================================

/// TYPE1-010: the exact `hsbw/rmoveto/rlineto×3/closepath` geometry of a box glyph
/// — proves the interpreter emits the precise segment sequence + sidebearing.
#[test]
fn type1_010_box_exact_geometry() {
    let (font, rec) = outline_named(&T1::new().glyph("A", box_cs(50, 600, 700)).build(), "A");
    assert_eq!(font.units_per_em(), 1000);
    assert_eq!(
        rec.segs,
        vec![
            Seg::Move(50, 0),
            Seg::Line(650, 0),
            Seg::Line(650, 700),
            Seg::Line(50, 700),
            Seg::Close,
        ],
        "box glyph draws the exact hsbw+rlineto sequence"
    );
}

/// TYPE1-011: the full one-byte + escape operator repertoire (stems, all move /
/// line / curve variants) yields the expected number of contours and curves.
#[test]
fn type1_011_rich_operator_repertoire() {
    let (_font, rec) = outline_named(&T1::new().glyph("P", rich_cs()).build(), "P");
    assert_eq!(rec.moves(), 3, "rmoveto + hmoveto + vmoveto → 3 contours");
    assert_eq!(
        rec.curves(),
        3,
        "rrcurveto + vhcurveto + hvcurveto → 3 curves"
    );
    assert_eq!(rec.closes(), 3, "three closepath ops");
    assert!(rec.lines() >= 4, "hline/vline/rline segments present");
}

/// TYPE1-012: a `move_to` while a contour is still open closes it, and a glyph
/// that ends without `endchar` (falling off the charstring) is closed by `finish`;
/// a 2000-unit coordinate exercises the 255 32-bit operand decode.
#[test]
fn type1_012_open_contours_finish_and_wide_operand() {
    let (_font, rec) = outline_named(&T1::new().glyph("O", two_open_contours_cs()).build(), "O");
    assert_eq!(rec.moves(), 2, "two rmoveto contours");
    // First contour closed by the second move_to; the tail closed by finish().
    assert_eq!(rec.closes(), 2, "both contours closed (reopen + finish)");
    // The 2000 operand decoded correctly (x = 50 + 2000).
    assert!(
        rec.segs.contains(&Seg::Line(2050, 50)),
        "255 32-bit operand decoded (x=2050): {:?}",
        rec.segs
    );
}

/// TYPE1-013: `/Subrs` are parsed and `callsubr` executes them (with the nested
/// `return`); the drawn glyph reaches the subr's rlineto endpoint.
#[test]
fn type1_013_subrs_callsubr() {
    let prog = T1::new()
        .subr(subr_line())
        .subr(Vec::new()) // empty subr: decrypt yields < lenIV bytes → empty entry.
        .glyph("S", callsubr_cs())
        .build();
    let (_font, rec) = outline_named(&prog, "S");
    // Path: move(50,50) → subr rlineto → (150,50) → rlineto → (150,150) → close.
    assert!(
        rec.segs.contains(&Seg::Line(150, 50)),
        "subr rlineto ran: {:?}",
        rec.segs
    );
    assert!(
        rec.segs.contains(&Seg::Line(150, 150)),
        "post-subr rlineto ran"
    );
    assert_eq!(rec.closes(), 1);
}

/// TYPE1-014: hint-replacement (`OtherSubr 3` + `pop` + `callsubr`) resolves and
/// runs the replacement subr, drawing the same shape as a direct call.
#[test]
fn type1_014_hint_replacement_othersubr3() {
    let prog = T1::new()
        .subr(subr_line())
        .glyph("H", hint_replace_cs())
        .build();
    let (_font, rec) = outline_named(&prog, "H");
    assert!(
        rec.segs.contains(&Seg::Line(150, 50)),
        "replacement subr ran: {:?}",
        rec.segs
    );
    assert!(rec.drawn());
}

/// TYPE1-015: a `flex` sequence (`OtherSubrs 1/2/0` + `pop`/`setcurrentpoint`)
/// emits two cubic curves through the seven collected reference points.
#[test]
fn type1_015_flex_via_othersubrs() {
    let (_font, rec) = outline_named(&T1::new().glyph("F", flex_cs()).build(), "F");
    assert_eq!(rec.moves(), 1, "single opening move, flex points not moves");
    assert_eq!(rec.curves(), 2, "flex emits exactly two cubics");
    assert_eq!(
        rec.segs.iter().find(|s| matches!(s, Seg::Curve(..))),
        Some(&Seg::Curve(70, 70, 80, 70, 90, 60)),
        "first flex cubic runs through the collected control points"
    );
}

/// TYPE1-028: a degenerate flex (fewer than seven captured points, gathered via
/// `vmoveto`/`hmoveto` while in flex) falls back to a single `line_to` — the
/// interpreter tolerates the malformed sequence and still inks a contour.
#[test]
fn type1_028_degenerate_flex() {
    let (_font, rec) = outline_named(&T1::new().glyph("G", degenerate_flex_cs()).build(), "G");
    assert_eq!(rec.moves(), 1, "single opening move");
    assert_eq!(rec.curves(), 0, "degenerate flex emits no cubic");
    assert!(
        rec.segs.contains(&Seg::Line(70, 70)),
        "degenerate flex lines to the last captured point: {:?}",
        rec.segs
    );
}

/// TYPE1-016: `sbw`, `div`, `dotsection`, `vstem3`/`hstem3`, and unknown escape /
/// one-byte ops are all tolerated; `div` feeds a real coordinate (200/2 = 100).
#[test]
fn type1_016_misc_operators() {
    let (_font, rec) = outline_named(&T1::new().glyph("D", misc_ops_cs()).build(), "D");
    // sbw start point (50,50); div → dx 100 → line to (150,50).
    assert!(
        rec.segs.contains(&Seg::Move(50, 50)),
        "sbw start point: {:?}",
        rec.segs
    );
    assert!(
        rec.segs.contains(&Seg::Line(150, 50)),
        "div-computed coordinate used"
    );
}

/// TYPE1-017: a `seac` composite with an OPEN owning contour composes an open base
/// and an open accent — the interpreter closes all three contours and offsets the
/// accent, so the composite inks three contours taller than the base alone.
#[test]
fn type1_017_seac_open_contours() {
    let prog = T1::new()
        .glyph("A", open_box_cs(0, 300, 300))
        .glyph("acute", accent_curve_cs())
        .glyph("Aacute", seac_open_cs())
        .build();
    let (_font, rec) = outline_named(&prog, "Aacute");
    assert_eq!(rec.moves(), 3, "own contour + base + accent");
    assert_eq!(rec.closes(), 3, "all three contours closed (open branches)");
    assert_eq!(
        rec.curves(),
        1,
        "the accent's curve is offset through OffsetBuilder"
    );
    // The accent (curve peak y=60) is placed at ady=300, above the 300-tall base.
    assert!(
        rec.max.1 > 300.0,
        "accent extends above the base box: {}",
        rec.max.1
    );
}

/// TYPE1-018: `.notdef` is forced to GID 0 (sfnt/CFF convention); other glyphs
/// follow in sorted order.
#[test]
fn type1_018_notdef_is_gid_zero() {
    let prog = T1::new()
        .glyph(".notdef", box_cs(0, 100, 100))
        .glyph("A", box_cs(0, 400, 700))
        .build();
    let font = Type1Font::parse(&prog).expect("parse");
    assert_eq!(font.glyph_for_name(".notdef"), Some(0), ".notdef → gid 0");
    assert_eq!(font.num_glyphs(), 2);
    assert!(font.glyph_for_name("A").is_some_and(|g| g != 0));
}

/// TYPE1-019: a PFB-wrapped program (binary `0x80` segment framing) is unwrapped
/// and parses identically to its flat form.
#[test]
fn type1_019_pfb_wrapped() {
    let (_font, rec) = outline_named(
        &T1::new().glyph("A", box_cs(50, 600, 700)).pfb().build(),
        "A",
    );
    assert_eq!(
        rec.segs,
        vec![
            Seg::Move(50, 0),
            Seg::Line(650, 0),
            Seg::Line(650, 700),
            Seg::Line(50, 700),
            Seg::Close,
        ],
        "PFB-wrapped program outlines the same box"
    );
}

/// TYPE1-020: an ASCII-hex eexec block (rather than raw binary) decodes and parses.
#[test]
fn type1_020_hex_eexec() {
    let (font, rec) = outline_named(
        &T1::new().glyph("A", box_cs(50, 600, 700)).hex().build(),
        "A",
    );
    assert_eq!(font.units_per_em(), 1000);
    assert!(rec.drawn(), "hex-eexec program still outlines the glyph");
}

/// TYPE1-021: `/FontMatrix` variants drive the upem: a scaled matrix, a negative
/// sign, a missing matrix, a bracket-less matrix, and an out-of-range scale all
/// resolve to the documented value (round(1/sx), else the 1000 default).
#[test]
fn type1_021_font_matrix_upem() {
    let upem = |t1: T1| {
        Type1Font::parse(&t1.glyph("A", box_cs(0, 400, 700)).build())
            .expect("parse")
            .units_per_em()
    };
    // sx = 0.002 → upem 500 (and a negative sign is taken by |sx|).
    assert_eq!(upem(T1::new().font_matrix("[-0.002 0 0 -0.002 0 0]")), 500);
    // A space after `[` exercises the float scanner's leading-whitespace skip.
    assert_eq!(upem(T1::new().font_matrix("[ 0.5 0 0 0.5 0 0]")), 2);
    // No /FontMatrix → default 1000.
    assert_eq!(upem(T1::new().no_font_matrix()), 1000);
    // A bracket-less /FontMatrix → default 1000 (no array open found).
    assert_eq!(upem(T1::new().font_matrix("0.001 0 0 0.001 0 0")), 1000);
    // sx = 100000 → round(1/sx) = 0, out of [1, 65535] → default 1000.
    assert_eq!(upem(T1::new().font_matrix("[100000 0 0 100000 0 0]")), 1000);
}

/// TYPE1-022: a builtin `/Encoding` maps a code to a non-AGL glyph name; malformed
/// `dup` entries (no code / no name) are skipped, and unassigned codes yield None.
#[test]
fn type1_022_builtin_encoding_and_bad_entries() {
    // A hand-written encoding block with two malformed entries then a valid one.
    let block = "/Encoding 256 array\ndup /A put\ndup 66 put\ndup 67 /ornament put\nreadonly def\n"
        .to_owned();
    let prog = T1::new()
        .glyph(".notdef", box_cs(0, 100, 100))
        .glyph("ornament", box_cs(0, 500, 500))
        .encoding_block(block)
        .build();
    let font = Type1Font::parse(&prog).expect("parse with custom encoding");
    // Only the well-formed `dup 67 /ornament put` took effect.
    let gid = font
        .glyph_for_code(67)
        .expect("code 67 → ornament via builtin");
    assert_eq!(font.glyph_for_name("ornament"), Some(gid));
    assert_ne!(gid, 0, "ornament is not .notdef");
    // The malformed entries left codes 65/66 unassigned.
    assert_eq!(
        font.glyph_for_code(65),
        None,
        "malformed `dup /A put` skipped"
    );
    assert_eq!(
        font.glyph_for_code(66),
        None,
        "malformed `dup 66 put` skipped"
    );
}

/// TYPE1-027: a well-formed builtin `/Encoding` array (the standard `256 array …
/// dup c /n put … readonly def` form) maps every declared code to its glyph.
#[test]
fn type1_027_builtin_encoding_standard_form() {
    let prog = T1::new()
        .glyph(".notdef", box_cs(0, 100, 100))
        .glyph("widget", box_cs(0, 400, 400))
        .glyph("gizmo", box_cs(0, 300, 300))
        .encoding_block(encoding_block(&[(0x80, "widget"), (0x81, "gizmo")]))
        .build();
    let font = Type1Font::parse(&prog).expect("parse");
    assert_eq!(font.glyph_for_code(0x80), font.glyph_for_name("widget"));
    assert_eq!(font.glyph_for_code(0x81), font.glyph_for_name("gizmo"));
    assert_eq!(font.glyph_for_code(0x82), None, "unassigned code → None");
}

/// TYPE1-023: a program without a custom `/Encoding` array records no builtin
/// table, so `glyph_for_code` returns None (the caller's StandardEncoding path).
#[test]
fn type1_023_no_builtin_encoding_table() {
    let prog = T1::new().glyph("A", box_cs(0, 400, 700)).build();
    let font = Type1Font::parse(&prog).expect("parse");
    assert_eq!(font.glyph_for_code(0x41), None);
    assert!(font.glyph_for_name("A").is_some());
}

/// TYPE1-024: `/lenIV -1` (rejected as negative) falls back to the default 4 and
/// the signed integer is parsed correctly — the glyph still decrypts + outlines.
#[test]
fn type1_024_negative_leniv_defaults() {
    let prog = T1::new()
        .len_iv_decl(Some("-1"))
        .glyph("A", box_cs(50, 600, 700))
        .build();
    let (_font, rec) = outline_named(&prog, "A");
    assert!(rec.drawn(), "charstrings decrypt with the default lenIV 4");
}

/// TYPE1-025: `outline` for an out-of-range GID reports no contour (draws nothing).
#[test]
fn type1_025_outline_out_of_range_gid() {
    let font = Type1Font::parse(&T1::new().glyph("A", box_cs(0, 400, 700)).build()).expect("parse");
    let mut rec = Rec::default();
    assert!(!font.outline(9999, &mut rec), "out-of-range gid → false");
    assert!(rec.segs.is_empty(), "nothing drawn");
}

/// TYPE1-026: failure inputs return `None` — garbage bytes (no eexec), a truncated
/// eexec block (too short to hold the private dict), and an empty `/CharStrings`.
#[test]
fn type1_026_parse_failures_return_none() {
    // Not a Type1 program at all (no `eexec`).
    assert!(Type1Font::parse(b"not a font at all").is_none());
    assert!(Type1Font::parse(b"").is_none());

    // A truncated eexec block: only two bytes of "encrypted" data (< 5 after
    // decrypt) → the private dict is too short.
    let mut trunc = Vec::new();
    trunc.extend_from_slice(b"%!FontType1-1.0: Trunc\ncurrentfile eexec\n");
    trunc.extend_from_slice(&[0x01, 0x02]);
    assert!(Type1Font::parse(&trunc).is_none(), "truncated eexec → None");

    // A valid eexec block but zero declared glyphs → empty CharStrings → None.
    let empty = T1::new().build();
    assert!(Type1Font::parse(&empty).is_none(), "no charstrings → None");
}
