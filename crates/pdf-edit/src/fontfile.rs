//! User-supplied TrueType/OpenType font embedding (PRD §8.5 / §8.5.2).
//!
//! `insert_text` with a `fontfile=` argument **fully embeds** the whole font
//! program (the full-embed fallback of §8.5.2 — subsetting is the deferred,
//! feature-gated `subset` path) as a composite `/Type0` font:
//!
//! ```text
//! /Type0  /Encoding /Identity-H        ── 2-byte codes == glyph IDs
//!   └─ DescendantFonts [ /CIDFontType2 ]
//!         ├─ /CIDToGIDMap /Identity     ── CID == GID
//!         ├─ /W  [ … per-glyph advances ]
//!         └─ /FontDescriptor /FontFile2 ── the whole TTF program (Flate)
//! /ToUnicode  ── CMap mapping each 2-byte code back to its Unicode scalar(s)
//! ```
//!
//! With Identity-H + Identity `/CIDToGIDMap`, a shown 2-byte code is the glyph
//! ID directly, so emitting text reduces to "look up each char's glyph ID via
//! the font `cmap`, write the 2-byte code." Widths come from the font `hmtx`
//! table (read by `ttf-parser`); the `/ToUnicode` map makes the inserted text
//! extractable / searchable (the M2 round-trip oracle).
//!
//! `ttf-parser` is `#![forbid(unsafe_code)]` and pure-Rust, preserving the
//! crate purity invariant.

use std::collections::BTreeMap;

use pdf_core::error::{Error, Result};
use pdf_core::filters::flate;
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::DocumentStore;

/// A parsed, embeddable font program plus the lookup tables `insert_text` needs:
/// per-char glyph-ID mapping (`cmap`) and per-glyph advance (`hmtx`).
pub struct EmbeddedFont {
    /// The raw font-program bytes (embedded verbatim — full embed).
    program: Vec<u8>,
    /// `units_per_em`, used to scale `hmtx` advances to the 1000-unit text space.
    units_per_em: f64,
    /// PostScript / family name for `/BaseFont` (sanitized).
    base_name: String,
    /// FontDescriptor metrics, scaled to 1000-unit text space.
    ascent: f64,
    descent: f64,
    cap_height: f64,
    bbox: [f64; 4],
    italic_angle: f64,
    is_fixed_pitch: bool,
    /// Number of glyphs in the font (bounds the `/W` array).
    num_glyphs: u16,
    /// Glyph-ID → 1000-unit advance, cached so a re-show is cheap.
    advances: Vec<f64>,
}

impl EmbeddedFont {
    /// Parses `program` (a TTF/OTF byte blob) into an embeddable font.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if the bytes are not a parseable font (never
    /// panics on arbitrary input).
    pub fn parse(program: &[u8]) -> Result<Self> {
        let face = ttf_parser::Face::parse(program, 0)
            .map_err(|_| Error::Unsupported("insert_text fontfile: not a parseable TTF/OTF"))?;

        let upem = f64::from(face.units_per_em());
        let scale = if upem > 0.0 { 1000.0 / upem } else { 1.0 };

        let num_glyphs = face.number_of_glyphs();
        let mut advances = Vec::with_capacity(num_glyphs as usize);
        for gid in 0..num_glyphs {
            let adv = face
                .glyph_hor_advance(ttf_parser::GlyphId(gid))
                .map_or(0.0, |a| f64::from(a) * scale);
            advances.push(adv);
        }

        let base_name = sanitize_base_name(font_name(&face));
        let bbox = face.global_bounding_box();

        Ok(EmbeddedFont {
            program: program.to_vec(),
            units_per_em: upem,
            base_name,
            ascent: f64::from(face.ascender()) * scale,
            descent: f64::from(face.descender()) * scale,
            cap_height: face
                .capital_height()
                .map_or(f64::from(face.ascender()) * scale, |h| f64::from(h) * scale),
            bbox: [
                f64::from(bbox.x_min) * scale,
                f64::from(bbox.y_min) * scale,
                f64::from(bbox.x_max) * scale,
                f64::from(bbox.y_max) * scale,
            ],
            italic_angle: f64::from(face.italic_angle()),
            is_fixed_pitch: face.is_monospaced(),
            num_glyphs,
            advances,
        })
    }

    /// The glyph ID for `ch` via the font `cmap`, or `0` (`.notdef`) if the font
    /// has no glyph for it.
    #[must_use]
    pub fn glyph_id(&self, ch: char) -> u16 {
        // Re-parse is cheap (zero-copy header parse) and avoids holding a
        // self-referential `Face<'_>`; only used while emitting text.
        ttf_parser::Face::parse(&self.program, 0)
            .ok()
            .and_then(|f| f.glyph_index(ch))
            .map_or(0, |g| g.0)
    }

    /// The 1000-unit advance of glyph `gid`.
    #[must_use]
    pub fn advance(&self, gid: u16) -> f64 {
        self.advances.get(gid as usize).copied().unwrap_or(0.0)
    }

    /// The 1000-unit advance of `ch` (via its glyph ID).
    #[must_use]
    pub fn char_advance(&self, ch: char) -> f64 {
        self.advance(self.glyph_id(ch))
    }

    /// The number of font units per em (for diagnostics / tests).
    #[must_use]
    pub fn units_per_em(&self) -> f64 {
        self.units_per_em
    }

    /// The length of the embedded font program in bytes (full-embed check).
    #[must_use]
    pub fn program_len(&self) -> usize {
        self.program.len()
    }

    /// The `/BaseFont` name chosen for this font.
    #[must_use]
    pub fn base_name(&self) -> &str {
        &self.base_name
    }

    /// Writes the complete `/Type0` font object graph into `doc` and returns the
    /// `/Type0` font reference, ready to register under `/Resources /Font`.
    ///
    /// `used` is the set of `(glyph_id, ch)` pairs the caller actually shows; it
    /// drives the `/W` width array and the `/ToUnicode` CMap. (The *program*
    /// itself is still fully embedded — only the metadata tables are scoped to
    /// the used glyphs, which is correct and keeps them compact.)
    ///
    /// # Errors
    ///
    /// Propagates ChangeSet-allocation errors.
    pub fn write_type0(&self, doc: &DocumentStore, used: &BTreeMap<u16, char>) -> Result<ObjRef> {
        // --- FontFile2: the whole program, Flate-compressed ----------------
        let compressed = flate::encode(&self.program);
        let mut ff_dict = Dict::new();
        ff_dict.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
        ff_dict.insert(
            Name::new("Length"),
            Object::Integer(compressed.len() as i64),
        );
        // `/Length1` is the *uncompressed* program length (required for TrueType
        // FontFile2 per ISO 32000-1 §9.9, Table 127).
        ff_dict.insert(
            Name::new("Length1"),
            Object::Integer(self.program.len() as i64),
        );
        let fontfile =
            doc.add_object(Object::Stream(StreamObj::new_encoded(ff_dict, compressed)))?;

        // --- FontDescriptor ------------------------------------------------
        let mut fd = Dict::new();
        fd.insert(Name::new("Type"), Object::Name(Name::new("FontDescriptor")));
        fd.insert(
            Name::new("FontName"),
            Object::Name(Name::new(&self.base_name)),
        );
        fd.insert(Name::new("Flags"), Object::Integer(self.descriptor_flags()));
        fd.insert(
            Name::new("FontBBox"),
            Object::Array(self.bbox.iter().map(|v| Object::Real(*v)).collect()),
        );
        fd.insert(Name::new("ItalicAngle"), Object::Real(self.italic_angle));
        fd.insert(Name::new("Ascent"), Object::Real(self.ascent));
        fd.insert(Name::new("Descent"), Object::Real(self.descent));
        fd.insert(Name::new("CapHeight"), Object::Real(self.cap_height));
        // StemV is not in the OpenType tables; a conventional middle value.
        fd.insert(Name::new("StemV"), Object::Integer(80));
        fd.insert(Name::new("FontFile2"), Object::Reference(fontfile));
        let descriptor = doc.add_object(Object::Dictionary(fd))?;

        // --- ToUnicode CMap ------------------------------------------------
        let tounicode_data = build_tounicode(used);
        let compressed_tu = flate::encode(&tounicode_data);
        let mut tu_dict = Dict::new();
        tu_dict.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
        tu_dict.insert(
            Name::new("Length"),
            Object::Integer(compressed_tu.len() as i64),
        );
        let tounicode = doc.add_object(Object::Stream(StreamObj::new_encoded(
            tu_dict,
            compressed_tu,
        )))?;

        // --- CIDFontType2 (descendant) -------------------------------------
        let mut cidfont = Dict::new();
        cidfont.insert(Name::new("Type"), Object::Name(Name::new("Font")));
        cidfont.insert(
            Name::new("Subtype"),
            Object::Name(Name::new("CIDFontType2")),
        );
        cidfont.insert(
            Name::new("BaseFont"),
            Object::Name(Name::new(&self.base_name)),
        );
        let mut cidsysinfo = Dict::new();
        cidsysinfo.insert(
            Name::new("Registry"),
            Object::String(pdf_core::object::PdfString::literal(b"Adobe".to_vec())),
        );
        cidsysinfo.insert(
            Name::new("Ordering"),
            Object::String(pdf_core::object::PdfString::literal(b"Identity".to_vec())),
        );
        cidsysinfo.insert(Name::new("Supplement"), Object::Integer(0));
        cidfont.insert(Name::new("CIDSystemInfo"), Object::Dictionary(cidsysinfo));
        cidfont.insert(
            Name::new("CIDToGIDMap"),
            Object::Name(Name::new("Identity")),
        );
        cidfont.insert(Name::new("FontDescriptor"), Object::Reference(descriptor));
        cidfont.insert(Name::new("DW"), Object::Integer(1000));
        cidfont.insert(Name::new("W"), self.width_array(used));
        let cidfont_ref = doc.add_object(Object::Dictionary(cidfont))?;

        // --- Type0 (the registered font) -----------------------------------
        let mut type0 = Dict::new();
        type0.insert(Name::new("Type"), Object::Name(Name::new("Font")));
        type0.insert(Name::new("Subtype"), Object::Name(Name::new("Type0")));
        type0.insert(
            Name::new("BaseFont"),
            Object::Name(Name::new(&self.base_name)),
        );
        type0.insert(Name::new("Encoding"), Object::Name(Name::new("Identity-H")));
        type0.insert(
            Name::new("DescendantFonts"),
            Object::Array(vec![Object::Reference(cidfont_ref)]),
        );
        type0.insert(Name::new("ToUnicode"), Object::Reference(tounicode));
        doc.add_object(Object::Dictionary(type0))
    }

    /// The `/W` array: for each used glyph ID, `[gid [advance]]` (the array form
    /// `c [w]`, ISO 32000-1 §9.7.4.3).
    fn width_array(&self, used: &BTreeMap<u16, char>) -> Object {
        let mut out = Vec::new();
        for &gid in used.keys() {
            if gid >= self.num_glyphs {
                continue;
            }
            out.push(Object::Integer(i64::from(gid)));
            out.push(Object::Array(vec![Object::Real(self.advance(gid))]));
        }
        Object::Array(out)
    }

    /// The `/FontDescriptor /Flags` bitfield (ISO 32000-1 §9.8.2, Table 121):
    /// bit1 FixedPitch, bit3 Symbolic (we set Nonsymbolic bit6 for text fonts),
    /// bit7 Italic.
    fn descriptor_flags(&self) -> i64 {
        let mut flags = 0i64;
        if self.is_fixed_pitch {
            flags |= 1; // FixedPitch
        }
        flags |= 1 << 5; // Nonsymbolic (bit 6, 1-based)
        if self.italic_angle != 0.0 {
            flags |= 1 << 6; // Italic (bit 7, 1-based)
        }
        flags
    }
}

/// Builds a `/ToUnicode` CMap (ISO 32000-1 §9.10.3) mapping each used 2-byte
/// code (the glyph ID under Identity-H) to its Unicode scalar(s), so the
/// inserted text is extractable / searchable.
fn build_tounicode(used: &BTreeMap<u16, char>) -> Vec<u8> {
    let mut s = String::new();
    s.push_str("/CIDInit /ProcSet findresource begin\n");
    s.push_str("12 dict begin\n");
    s.push_str("begincmap\n");
    s.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    s.push_str("/CMapName /Adobe-Identity-UCS def\n");
    s.push_str("/CMapType 2 def\n");
    s.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");

    // `bfchar` entries, chunked at 100 per `beginbfchar` block (the CMap spec
    // limit). Each entry: `<src2bytes> <dstUTF16BE>`.
    let entries: Vec<(u16, char)> = used.iter().map(|(&g, &c)| (g, c)).collect();
    for chunk in entries.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, ch) in chunk {
            s.push_str(&format!("<{gid:04X}> <{}>\n", utf16be_hex(*ch)));
        }
        s.push_str("endbfchar\n");
    }

    s.push_str("endcmap\n");
    s.push_str("CMapName currentdict /CMap defineresource pop\n");
    s.push_str("end\nend\n");
    s.into_bytes()
}

/// Encodes `ch` as upper-hex UTF-16BE (one or two code units).
fn utf16be_hex(ch: char) -> String {
    let mut buf = [0u16; 2];
    let units = ch.encode_utf16(&mut buf);
    let mut s = String::new();
    for u in units.iter() {
        s.push_str(&format!("{u:04X}"));
    }
    s
}

/// Reads a usable font name (prefer the PostScript name, then the typographic /
/// full family name), defaulting to `"Embedded"`.
fn font_name(face: &ttf_parser::Face) -> String {
    let pick = |id: u16| -> Option<String> {
        face.names()
            .into_iter()
            .find(|n| n.name_id == id && n.is_unicode())
            .and_then(|n| n.to_string())
    };
    pick(ttf_parser::name_id::POST_SCRIPT_NAME)
        .or_else(|| pick(ttf_parser::name_id::FULL_NAME))
        .or_else(|| pick(ttf_parser::name_id::FAMILY))
        .unwrap_or_else(|| "Embedded".to_string())
}

/// Sanitizes a `/BaseFont` name: drops whitespace and bytes illegal in a PDF
/// name, keeps it non-empty.
fn sanitize_base_name(raw: String) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            c.is_ascii_graphic()
                && !matches!(
                    c,
                    '/' | '(' | ')' | '<' | '>' | '[' | ']' | '{' | '}' | '%' | '#'
                )
        })
        .collect();
    if cleaned.is_empty() {
        "Embedded".to_string()
    } else {
        cleaned
    }
}
