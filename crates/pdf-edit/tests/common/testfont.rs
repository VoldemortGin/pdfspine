//! Synthetic minimal TrueType font generator for font-embedding tests.
//!
//! Builds a structurally-valid TTF byte vector entirely in code (no external
//! file, no copied font data) so it is a self-contained, license-clean test
//! asset. The font has no real glyph outlines (all glyphs are empty), but it is
//! valid enough that `ttf_parser::Face::parse` accepts it and the tables the
//! production code reads (`head`, `hhea`, `maxp`, `hmtx`, `cmap`, `loca`,
//! `glyf`, `name`, `post`, `OS/2`) all parse correctly.

#![allow(dead_code)] // only used by a subset of tests

const UNITS_PER_EM: u16 = 1000;
const ASCENDER: i16 = 800;
const DESCENDER: i16 = -200;
const LINE_GAP: i16 = 0;
const CAP_HEIGHT: i16 = 700;
const X_HEIGHT: i16 = 500;
const POSTSCRIPT_NAME: &str = "OxipdfTest";

/// One table's tag, raw (unpadded) contents and computed checksum.
struct Table {
    tag: [u8; 4],
    data: Vec<u8>,
    checksum: u32,
}

/// Sum of big-endian u32 words, zero-padded to a 4-byte boundary.
fn checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < data.len() {
        let mut word = [0u8; 4];
        let n = (data.len() - i).min(4);
        word[..n].copy_from_slice(&data[i..i + n]);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
        i += 4;
    }
    sum
}

/// Pad `data` length up to a multiple of 4 with zero bytes.
fn pad4(data: &mut Vec<u8>) {
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
}

/// Builds a structurally-valid synthetic TrueType font (no real outlines) that
/// `ttf_parser::Face::parse` accepts, mapping the ASCII chars in `chars` to
/// glyph IDs 1.. with the given per-glyph advance (1000-unit em). PostScript
/// name "OxipdfTest". For font-embedding tests only.
pub fn build_test_ttf(chars: &[char], advance: u16) -> Vec<u8> {
    let num_glyphs: u16 = (chars.len() as u16) + 1; // + .notdef

    // --- glyf: every glyph is empty (zero-length). ----------------------
    // An empty glyf entry (loca[i] == loca[i+1]) is a valid "no contours"
    // glyph, which is what a space-like glyph is.
    let glyf: Vec<u8> = Vec::new();

    // --- loca (short format, indexToLocFormat = 0). ---------------------
    // Short loca stores offset/2; all offsets are 0 -> every glyph empty.
    // Needs num_glyphs + 1 entries.
    let mut loca = Vec::new();
    for _ in 0..=num_glyphs {
        loca.extend_from_slice(&0u16.to_be_bytes());
    }

    // --- head ----------------------------------------------------------
    // checkSumAdjustment is written as 0 first, then patched at the end.
    let mut head = Vec::new();
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // fontRevision
    head.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment (patched)
    head.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head.extend_from_slice(&0u16.to_be_bytes()); // flags
    head.extend_from_slice(&UNITS_PER_EM.to_be_bytes()); // unitsPerEm
    head.extend_from_slice(&0i64.to_be_bytes()); // created
    head.extend_from_slice(&0i64.to_be_bytes()); // modified
    head.extend_from_slice(&0i16.to_be_bytes()); // xMin
    head.extend_from_slice(&DESCENDER.to_be_bytes()); // yMin
    head.extend_from_slice(&(advance as i16).to_be_bytes()); // xMax
    head.extend_from_slice(&ASCENDER.to_be_bytes()); // yMax
    head.extend_from_slice(&0u16.to_be_bytes()); // macStyle
    head.extend_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    head.extend_from_slice(&2i16.to_be_bytes()); // fontDirectionHint
    head.extend_from_slice(&0i16.to_be_bytes()); // indexToLocFormat (0 = short)
    head.extend_from_slice(&0i16.to_be_bytes()); // glyphDataFormat

    // --- hhea ----------------------------------------------------------
    let mut hhea = Vec::new();
    hhea.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    hhea.extend_from_slice(&ASCENDER.to_be_bytes()); // ascender
    hhea.extend_from_slice(&DESCENDER.to_be_bytes()); // descender
    hhea.extend_from_slice(&LINE_GAP.to_be_bytes()); // lineGap
    hhea.extend_from_slice(&advance.to_be_bytes()); // advanceWidthMax
    hhea.extend_from_slice(&0i16.to_be_bytes()); // minLeftSideBearing
    hhea.extend_from_slice(&0i16.to_be_bytes()); // minRightSideBearing
    hhea.extend_from_slice(&(advance as i16).to_be_bytes()); // xMaxExtent
    hhea.extend_from_slice(&1i16.to_be_bytes()); // caretSlopeRise
    hhea.extend_from_slice(&0i16.to_be_bytes()); // caretSlopeRun
    hhea.extend_from_slice(&0i16.to_be_bytes()); // caretOffset
    hhea.extend_from_slice(&0i16.to_be_bytes()); // reserved
    hhea.extend_from_slice(&0i16.to_be_bytes()); // reserved
    hhea.extend_from_slice(&0i16.to_be_bytes()); // reserved
    hhea.extend_from_slice(&0i16.to_be_bytes()); // reserved
    hhea.extend_from_slice(&0i16.to_be_bytes()); // metricDataFormat
    hhea.extend_from_slice(&num_glyphs.to_be_bytes()); // numberOfHMetrics

    // --- maxp (version 1.0 for TrueType). ------------------------------
    let mut maxp = Vec::new();
    maxp.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    maxp.extend_from_slice(&num_glyphs.to_be_bytes()); // numGlyphs
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxPoints
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxContours
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxCompositePoints
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxCompositeContours
    maxp.extend_from_slice(&1u16.to_be_bytes()); // maxZones
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxTwilightPoints
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxStorage
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxFunctionDefs
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxInstructionDefs
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxStackElements
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxSizeOfInstructions
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxComponentElements
    maxp.extend_from_slice(&0u16.to_be_bytes()); // maxComponentDepth

    // --- hmtx (numberOfHMetrics == numGlyphs). -------------------------
    // Each entry: advanceWidth (u16) + leftSideBearing (i16).
    let mut hmtx = Vec::new();
    for _ in 0..num_glyphs {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    // --- cmap (platform 3 / encoding 1, format 4). ---------------------
    let cmap = build_cmap(chars);

    // --- name (nameID 6 PostScript name, platform 3/encoding 1). -------
    let name = build_name();

    // --- post (version 3.0: no glyph names). ---------------------------
    let mut post = Vec::new();
    post.extend_from_slice(&0x0003_0000u32.to_be_bytes()); // version 3.0
    post.extend_from_slice(&0i32.to_be_bytes()); // italicAngle
    post.extend_from_slice(&DESCENDER.to_be_bytes()); // underlinePosition
    post.extend_from_slice(&50i16.to_be_bytes()); // underlineThickness
    post.extend_from_slice(&0u32.to_be_bytes()); // isFixedPitch
    post.extend_from_slice(&0u32.to_be_bytes()); // minMemType42
    post.extend_from_slice(&0u32.to_be_bytes()); // maxMemType42
    post.extend_from_slice(&0u32.to_be_bytes()); // minMemType1
    post.extend_from_slice(&0u32.to_be_bytes()); // maxMemType1

    // --- OS/2 (version 4). ---------------------------------------------
    let os2 = build_os2(advance, chars);

    // Assemble tables. Order in the file is arbitrary; the directory must be
    // sorted by tag. We register in a stable order and sort the directory.
    let mut tables = vec![
        new_table(*b"OS/2", os2),
        new_table(*b"cmap", cmap),
        new_table(*b"glyf", glyf),
        new_table(*b"head", head),
        new_table(*b"hhea", hhea),
        new_table(*b"hmtx", hmtx),
        new_table(*b"loca", loca),
        new_table(*b"maxp", maxp),
        new_table(*b"name", name),
        new_table(*b"post", post),
    ];
    tables.sort_by_key(|t| t.tag);

    assemble_font(&mut tables)
}

fn new_table(tag: [u8; 4], data: Vec<u8>) -> Table {
    let checksum = checksum(&data);
    Table {
        tag,
        data,
        checksum,
    }
}

/// Builds a format-4 cmap with a single (3,1) subtable mapping each char in
/// `chars` to glyph IDs 1.. (in input order).
fn build_cmap(chars: &[char]) -> Vec<u8> {
    // Collect (codepoint, glyph_id) for BMP chars, sorted by codepoint.
    let mut mappings: Vec<(u16, u16)> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        let cp = c as u32;
        debug_assert!(cp <= 0xFFFF, "only BMP chars are supported");
        mappings.push((cp as u16, (i as u16) + 1));
    }
    mappings.sort_by_key(|&(cp, _)| cp);

    // Build segments. Each char becomes its own segment for simplicity, plus
    // the mandatory terminating 0xFFFF segment.
    // Arrays: endCode[], startCode[], idDelta[], idRangeOffset[].
    let mut end_code = Vec::new();
    let mut start_code = Vec::new();
    let mut id_delta = Vec::new();
    let mut id_range_offset = Vec::new();

    for &(cp, gid) in &mappings {
        end_code.push(cp);
        start_code.push(cp);
        // idDelta such that (cp + idDelta) mod 65536 == gid.
        let delta = (gid as i32 - cp as i32) as i16;
        id_delta.push(delta);
        id_range_offset.push(0u16);
    }
    // Terminating segment.
    end_code.push(0xFFFF);
    start_code.push(0xFFFF);
    id_delta.push(1);
    id_range_offset.push(0);

    let seg_count = end_code.len() as u16;
    let seg_count_x2 = seg_count * 2;
    let search_range = 2 * pow2_floor(seg_count);
    let entry_selector = log2_floor(search_range / 2);
    let range_shift = seg_count_x2 - search_range;

    // Format-4 subtable body.
    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes()); // format
    let length_pos = sub.len();
    sub.extend_from_slice(&0u16.to_be_bytes()); // length (patched)
    sub.extend_from_slice(&0u16.to_be_bytes()); // language
    sub.extend_from_slice(&seg_count_x2.to_be_bytes());
    sub.extend_from_slice(&search_range.to_be_bytes());
    sub.extend_from_slice(&entry_selector.to_be_bytes());
    sub.extend_from_slice(&range_shift.to_be_bytes());
    for &e in &end_code {
        sub.extend_from_slice(&e.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for &s in &start_code {
        sub.extend_from_slice(&s.to_be_bytes());
    }
    for &d in &id_delta {
        sub.extend_from_slice(&d.to_be_bytes());
    }
    for &r in &id_range_offset {
        sub.extend_from_slice(&r.to_be_bytes());
    }
    // glyphIdArray is empty (all idRangeOffset == 0).

    // Patch subtable length.
    let sub_len = sub.len() as u16;
    sub[length_pos..length_pos + 2].copy_from_slice(&sub_len.to_be_bytes());

    // cmap header: version + numTables, then one encoding record.
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes()); // version
    cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
    cmap.extend_from_slice(&3u16.to_be_bytes()); // platformID (Windows)
    cmap.extend_from_slice(&1u16.to_be_bytes()); // encodingID (Unicode BMP)
    let subtable_offset = 12u32; // 4 (header) + 8 (one record).
    cmap.extend_from_slice(&subtable_offset.to_be_bytes());
    cmap.extend_from_slice(&sub);
    cmap
}

/// Builds a name table with a single record: nameID 6 (PostScript name),
/// platform 3 / encoding 1 / language 0x409, UTF-16BE.
fn build_name() -> Vec<u8> {
    let value: Vec<u8> = POSTSCRIPT_NAME
        .encode_utf16()
        .flat_map(|u| u.to_be_bytes())
        .collect();

    let mut name = Vec::new();
    name.extend_from_slice(&0u16.to_be_bytes()); // format 0
    name.extend_from_slice(&1u16.to_be_bytes()); // count
    let storage_offset: u16 = 6 + 12; // header + one record.
    name.extend_from_slice(&storage_offset.to_be_bytes());
    // Name record.
    name.extend_from_slice(&3u16.to_be_bytes()); // platformID
    name.extend_from_slice(&1u16.to_be_bytes()); // encodingID
    name.extend_from_slice(&0x0409u16.to_be_bytes()); // languageID (en-US)
    name.extend_from_slice(&6u16.to_be_bytes()); // nameID = PostScript name
    name.extend_from_slice(&(value.len() as u16).to_be_bytes()); // length
    name.extend_from_slice(&0u16.to_be_bytes()); // offset into storage
    name.extend_from_slice(&value);
    name
}

/// Builds an OS/2 table (version 4).
fn build_os2(advance: u16, chars: &[char]) -> Vec<u8> {
    let mut os2 = Vec::new();
    os2.extend_from_slice(&4u16.to_be_bytes()); // version
    os2.extend_from_slice(&(advance as i16).to_be_bytes()); // xAvgCharWidth
    os2.extend_from_slice(&400u16.to_be_bytes()); // usWeightClass (Normal)
    os2.extend_from_slice(&5u16.to_be_bytes()); // usWidthClass (Medium)
    os2.extend_from_slice(&0u16.to_be_bytes()); // fsType
    os2.extend_from_slice(&500i16.to_be_bytes()); // ySubscriptXSize
    os2.extend_from_slice(&500i16.to_be_bytes()); // ySubscriptYSize
    os2.extend_from_slice(&0i16.to_be_bytes()); // ySubscriptXOffset
    os2.extend_from_slice(&100i16.to_be_bytes()); // ySubscriptYOffset
    os2.extend_from_slice(&500i16.to_be_bytes()); // ySuperscriptXSize
    os2.extend_from_slice(&500i16.to_be_bytes()); // ySuperscriptYSize
    os2.extend_from_slice(&0i16.to_be_bytes()); // ySuperscriptXOffset
    os2.extend_from_slice(&400i16.to_be_bytes()); // ySuperscriptYOffset
    os2.extend_from_slice(&50i16.to_be_bytes()); // yStrikeoutSize
    os2.extend_from_slice(&250i16.to_be_bytes()); // yStrikeoutPosition
    os2.extend_from_slice(&0i16.to_be_bytes()); // sFamilyClass
                                                // panose (10 bytes, all zero = "any").
    os2.extend_from_slice(&[0u8; 10]);
    // ulUnicodeRange1..4. Bit 0 (Basic Latin) covers our ASCII chars.
    os2.extend_from_slice(&1u32.to_be_bytes()); // ulUnicodeRange1
    os2.extend_from_slice(&0u32.to_be_bytes()); // ulUnicodeRange2
    os2.extend_from_slice(&0u32.to_be_bytes()); // ulUnicodeRange3
    os2.extend_from_slice(&0u32.to_be_bytes()); // ulUnicodeRange4
    os2.extend_from_slice(b"OXIP"); // achVendID
    os2.extend_from_slice(&0x0040u16.to_be_bytes()); // fsSelection (REGULAR)
    let (first, last) = char_range(chars);
    os2.extend_from_slice(&first.to_be_bytes()); // usFirstCharIndex
    os2.extend_from_slice(&last.to_be_bytes()); // usLastCharIndex
    os2.extend_from_slice(&ASCENDER.to_be_bytes()); // sTypoAscender
    os2.extend_from_slice(&DESCENDER.to_be_bytes()); // sTypoDescender
    os2.extend_from_slice(&LINE_GAP.to_be_bytes()); // sTypoLineGap
    os2.extend_from_slice(&(ASCENDER as u16).to_be_bytes()); // usWinAscent
    os2.extend_from_slice(&((-DESCENDER) as u16).to_be_bytes()); // usWinDescent
    os2.extend_from_slice(&1u32.to_be_bytes()); // ulCodePageRange1 (Latin 1)
    os2.extend_from_slice(&0u32.to_be_bytes()); // ulCodePageRange2
    os2.extend_from_slice(&X_HEIGHT.to_be_bytes()); // sxHeight
    os2.extend_from_slice(&CAP_HEIGHT.to_be_bytes()); // sCapHeight
    os2.extend_from_slice(&0u16.to_be_bytes()); // usDefaultChar
    os2.extend_from_slice(&(b' ' as u16).to_be_bytes()); // usBreakChar
    os2.extend_from_slice(&0u16.to_be_bytes()); // usMaxContext
    os2
}

/// Returns (usFirstCharIndex, usLastCharIndex) over the mapped BMP chars.
fn char_range(chars: &[char]) -> (u16, u16) {
    let mut first: u16 = 0xFFFF;
    let mut last: u16 = 0;
    for &c in chars {
        let cp = c as u32;
        if cp <= 0xFFFF {
            let cp = cp as u16;
            first = first.min(cp);
            last = last.max(cp);
        }
    }
    if last == 0 {
        (0, 0)
    } else {
        (first, last)
    }
}

/// Largest power of two <= n (n >= 1).
fn pow2_floor(n: u16) -> u16 {
    let mut p = 1u16;
    while p * 2 <= n {
        p *= 2;
    }
    p
}

/// floor(log2(n)) for n >= 1.
fn log2_floor(n: u16) -> u16 {
    let mut p = 0u16;
    let mut v = n;
    while v > 1 {
        v /= 2;
        p += 1;
    }
    p
}

/// Lays out the offset table + table directory + (padded) table bodies, then
/// patches `head.checkSumAdjustment`.
fn assemble_font(tables: &mut [Table]) -> Vec<u8> {
    let num_tables = tables.len() as u16;
    let search_range = pow2_floor(num_tables) * 16;
    let entry_selector = log2_floor(pow2_floor(num_tables));
    let range_shift = num_tables * 16 - search_range;

    let offset_table_len = 12;
    let dir_len = 16 * tables.len();
    let mut running_offset = offset_table_len + dir_len;

    // Compute each table's offset (4-byte aligned) and remember head's slot.
    let mut offsets: Vec<u32> = Vec::with_capacity(tables.len());
    let mut head_dir_index: Option<usize> = None;
    for (i, t) in tables.iter().enumerate() {
        offsets.push(running_offset as u32);
        if &t.tag == b"head" {
            head_dir_index = Some(i);
        }
        running_offset += t.data.len();
        running_offset += (4 - running_offset % 4) % 4; // align next start.
    }

    let mut out = Vec::with_capacity(running_offset);
    // Offset table.
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfnt version
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    // Table directory (already sorted by tag).
    for (i, t) in tables.iter().enumerate() {
        out.extend_from_slice(&t.tag);
        out.extend_from_slice(&t.checksum.to_be_bytes());
        out.extend_from_slice(&offsets[i].to_be_bytes());
        out.extend_from_slice(&(t.data.len() as u32).to_be_bytes());
    }

    // Table bodies, each padded to a 4-byte boundary.
    let mut head_offset = 0usize;
    for (i, t) in tables.iter().enumerate() {
        debug_assert_eq!(out.len() as u32, offsets[i]);
        if &t.tag == b"head" {
            head_offset = out.len();
        }
        out.extend_from_slice(&t.data);
        pad4(&mut out);
    }

    // Patch head.checkSumAdjustment.
    // checkSumAdjustment = 0xB1B0AFBA - checksum(whole font with field == 0).
    // The field is currently 0 in the buffer, so checksum the whole thing.
    debug_assert!(head_dir_index.is_some());
    let _ = head_dir_index;
    let total = checksum(&out);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(total);
    // checkSumAdjustment is at offset 8 within the head table.
    let pos = head_offset + 8;
    out[pos..pos + 4].copy_from_slice(&adjustment.to_be_bytes());

    out
}
