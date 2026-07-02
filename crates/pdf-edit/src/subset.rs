//! Usage-based TrueType glyph subsetting (PRD-NEXT §10 TS-3).
//!
//! [`subset_truetype`] rebuilds a standalone TrueType font that keeps **only
//! the used glyphs** (plus the recursive closure of composite-glyph
//! components), preserving the ORIGINAL glyph IDs — the shown 2-byte
//! Identity-H codes in already-emitted content streams stay valid, so the
//! subsetter never renumbers anything.
//!
//! Rebuilt tables: `glyf` / `loca` (kept glyphs verbatim, others empty),
//! `hmtx` (all-long metrics up to the max kept glyph), `cmap` (format 4 +
//! format 12 for the used char → glyph mapping), `head` / `hhea` / `maxp`
//! (patched counts / loca format) and a minimal version-3.0 `post`. `OS/2`,
//! `name` and the hinting tables (`cvt `, `fpgm`, `prep`) are copied verbatim
//! so glyph instructions keep working.
//!
//! CFF-flavored OpenType (`OTTO`, no `glyf`) is **not** subset in v1:
//! [`subset_truetype`] returns `None` and the caller falls back to the
//! whole-program embed (documented degradation). Malformed fonts degrade the
//! same way — this module never panics on arbitrary input.

use std::collections::{BTreeMap, BTreeSet};

use ttf_parser::{RawFace, Tag};

/// Reads a big-endian `u16` at `off`.
fn be_u16(d: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(off)?, *d.get(off + 1)?]))
}

/// Reads a big-endian `u32` at `off`.
fn be_u32(d: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(off)?,
        *d.get(off + 1)?,
        *d.get(off + 2)?,
        *d.get(off + 3)?,
    ]))
}

/// Builds a subset TrueType program from face `face_index` of `program`
/// keeping the glyphs in `used` (gid → char), or `None` when the face is not
/// subsettable (CFF-flavored / malformed) — the caller then embeds the whole
/// program.
pub(crate) fn subset_truetype(
    program: &[u8],
    face_index: u32,
    used: &BTreeMap<u16, char>,
) -> Option<Vec<u8>> {
    let raw = RawFace::parse(program, face_index).ok()?;
    let head = raw.table(Tag::from_bytes(b"head"))?;
    let hhea = raw.table(Tag::from_bytes(b"hhea"))?;
    let maxp = raw.table(Tag::from_bytes(b"maxp"))?;
    let loca = raw.table(Tag::from_bytes(b"loca"))?;
    let glyf = raw.table(Tag::from_bytes(b"glyf"))?;
    let hmtx = raw.table(Tag::from_bytes(b"hmtx"))?;
    if head.len() < 54 || hhea.len() < 36 || maxp.len() < 6 {
        return None;
    }

    let num_glyphs = be_u16(maxp, 4)?;
    let long_loca = be_u16(head, 50)? != 0; // indexToLocFormat: 0 short, 1 long
    let num_h_metrics = be_u16(hhea, 34)?;
    if num_glyphs == 0 || num_h_metrics == 0 {
        return None;
    }

    // The raw glyph-data slice for `gid` via `loca` (empty glyphs allowed).
    let glyph_range = |gid: u16| -> Option<(usize, usize)> {
        if gid >= num_glyphs {
            return None;
        }
        let i = gid as usize;
        let (start, end) = if long_loca {
            (
                be_u32(loca, i * 4)? as usize,
                be_u32(loca, i * 4 + 4)? as usize,
            )
        } else {
            (
                be_u16(loca, i * 2)? as usize * 2,
                be_u16(loca, i * 2 + 2)? as usize * 2,
            )
        };
        if start > end || end > glyf.len() {
            return None;
        }
        Some((start, end))
    };

    // --- keep-set: used gids + .notdef + recursive composite components ----
    let mut keep: BTreeSet<u16> = BTreeSet::new();
    let mut stack: Vec<u16> = used.keys().copied().filter(|&g| g < num_glyphs).collect();
    stack.push(0); // .notdef always survives
    while let Some(gid) = stack.pop() {
        if !keep.insert(gid) {
            continue;
        }
        let (start, end) = glyph_range(gid)?;
        if end - start < 10 {
            continue; // empty glyph (or too short to be composite)
        }
        let data = &glyf[start..end];
        let contours = i16::from_be_bytes([data[0], data[1]]);
        if contours >= 0 {
            continue; // simple glyph
        }
        // Composite: walk the component records and enqueue every component
        // glyph (the closure — PRD TS-3's must-not-miss case).
        let mut off = 10usize;
        loop {
            let flags = be_u16(data, off)?;
            let component = be_u16(data, off + 2)?;
            if component < num_glyphs {
                stack.push(component);
            }
            off += 4;
            off += if flags & 0x0001 != 0 { 4 } else { 2 }; // ARG_1_AND_2_ARE_WORDS
            if flags & 0x0008 != 0 {
                off += 2; // WE_HAVE_A_SCALE
            } else if flags & 0x0040 != 0 {
                off += 4; // AN_X_AND_Y_SCALE
            } else if flags & 0x0080 != 0 {
                off += 8; // WE_HAVE_A_TWO_BY_TWO
            }
            if flags & 0x0020 == 0 {
                break; // no MORE_COMPONENTS
            }
        }
    }

    // Original gid numbering is preserved: glyphs above the max kept gid are
    // dropped entirely, the rest keep their slot (empty when unused).
    let max_gid = *keep.iter().next_back()?;
    let new_count = u32::from(max_gid) + 1;

    // --- glyf + loca (always long format) ----------------------------------
    let mut new_glyf: Vec<u8> = Vec::new();
    let mut new_loca: Vec<u8> = Vec::with_capacity((new_count as usize + 1) * 4);
    for gid in 0..=max_gid {
        new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
        if keep.contains(&gid) {
            let (start, end) = glyph_range(gid)?;
            new_glyf.extend_from_slice(&glyf[start..end]);
            if !new_glyf.len().is_multiple_of(2) {
                new_glyf.push(0); // keep glyph offsets word-aligned
            }
        }
    }
    new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());

    // --- hmtx: all-long metrics for gids 0..=max_gid ------------------------
    // Advances beyond `numberOfHMetrics` repeat the last advance (sfnt rule);
    // a truncated lsb array degrades to 0 (never a panic).
    let advance = |gid: u16| -> Option<u16> {
        let i = gid.min(num_h_metrics - 1) as usize;
        be_u16(hmtx, i * 4)
    };
    let lsb = |gid: u16| -> i16 {
        let off = if gid < num_h_metrics {
            gid as usize * 4 + 2
        } else {
            num_h_metrics as usize * 4 + (gid - num_h_metrics) as usize * 2
        };
        be_u16(hmtx, off).map_or(0, |v| v as i16)
    };
    let mut new_hmtx = Vec::with_capacity(new_count as usize * 4);
    for gid in 0..=max_gid {
        new_hmtx.extend_from_slice(&advance(gid)?.to_be_bytes());
        new_hmtx.extend_from_slice(&lsb(gid).to_be_bytes());
    }

    // --- head / hhea / maxp patches ----------------------------------------
    let mut new_head = head[..54].to_vec();
    new_head[8..12].fill(0); // checkSumAdjustment (patched after assembly)
    new_head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    let mut new_hhea = hhea[..36].to_vec();
    new_hhea[34..36].copy_from_slice(&(new_count as u16).to_be_bytes());

    let mut new_maxp = maxp.to_vec();
    new_maxp[4..6].copy_from_slice(&(new_count as u16).to_be_bytes());

    // --- cmap: used char → gid, original gids -------------------------------
    let mappings: BTreeMap<u32, u16> = used
        .iter()
        .filter(|&(gid, _)| keep.contains(gid))
        .map(|(&gid, &ch)| (ch as u32, gid))
        .collect();
    let new_cmap = build_cmap(&mappings);

    // --- minimal post (version 3.0 — no glyph names) -------------------------
    let new_post = build_post(raw.table(Tag::from_bytes(b"post")));

    // --- assemble ------------------------------------------------------------
    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"cmap", new_cmap),
        (*b"glyf", new_glyf),
        (*b"head", new_head),
        (*b"hhea", new_hhea),
        (*b"hmtx", new_hmtx),
        (*b"loca", new_loca),
        (*b"maxp", new_maxp),
        (*b"post", new_post),
    ];
    for tag in [b"OS/2", b"cvt ", b"fpgm", b"prep", b"name"] {
        if let Some(data) = raw.table(Tag::from_bytes(tag)) {
            tables.push((*tag, data.to_vec()));
        }
    }
    tables.sort_by_key(|(tag, _)| *tag);
    Some(assemble_sfnt(&tables))
}

/// A deterministic 6-uppercase-letter subset tag (ISO 32000-1 §9.6.4 tagged
/// name convention, `ABCDEF+Base`), derived from the used glyph set via
/// FNV-1a so identical inputs always produce identical bytes.
pub(crate) fn subset_tag(used: &BTreeMap<u16, char>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |b: u8| h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
    for (&gid, &ch) in used {
        for b in gid.to_be_bytes() {
            mix(b);
        }
        for b in (ch as u32).to_be_bytes() {
            mix(b);
        }
    }
    (0..6)
        .map(|i| char::from(b'A' + ((h >> (i * 8)) % 26) as u8))
        .collect()
}

/// Builds a `cmap` with a (3,1) format-4 subtable for the BMP mappings and,
/// when any supplementary-plane char is used, a (3,10) format-12 subtable
/// covering everything.
fn build_cmap(mappings: &BTreeMap<u32, u16>) -> Vec<u8> {
    let bmp: Vec<(u16, u16)> = mappings
        .iter()
        .filter(|&(&cp, _)| cp < 0xFFFF)
        .map(|(&cp, &gid)| (cp as u16, gid))
        .collect();
    let has_supplementary = mappings.keys().any(|&cp| cp > 0xFFFF);

    let fmt4 = build_cmap_format4(&bmp);
    let fmt12 = has_supplementary.then(|| build_cmap_format12(mappings));

    let num_tables: u16 = 1 + u16::from(fmt12.is_some());
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes()); // version
    cmap.extend_from_slice(&num_tables.to_be_bytes());
    let mut subtable_offset = 4 + u32::from(num_tables) * 8;
    // (3,1) Windows Unicode BMP → format 4.
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&subtable_offset.to_be_bytes());
    subtable_offset += fmt4.len() as u32;
    if fmt12.is_some() {
        // (3,10) Windows Unicode full repertoire → format 12.
        cmap.extend_from_slice(&3u16.to_be_bytes());
        cmap.extend_from_slice(&10u16.to_be_bytes());
        cmap.extend_from_slice(&subtable_offset.to_be_bytes());
    }
    cmap.extend_from_slice(&fmt4);
    if let Some(f12) = fmt12 {
        cmap.extend_from_slice(&f12);
    }
    cmap
}

/// A format-4 subtable: contiguous (codepoint, gid) runs become one segment
/// each, plus the mandatory 0xFFFF terminator segment.
fn build_cmap_format4(bmp: &[(u16, u16)]) -> Vec<u8> {
    // (start_cp, end_cp, start_gid) runs.
    let mut segs: Vec<(u16, u16, u16)> = Vec::new();
    for &(cp, gid) in bmp {
        match segs.last_mut() {
            Some((start, end, sgid))
                if u32::from(cp) == u32::from(*end) + 1
                    && u32::from(gid) == u32::from(*sgid) + u32::from(cp - *start) =>
            {
                *end = cp;
            }
            _ => segs.push((cp, cp, gid)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 0)); // terminator

    let seg_count = segs.len() as u16;
    let seg_count_x2 = seg_count * 2;
    let search_range = 2 * pow2_floor(seg_count);
    let entry_selector = log2_floor(search_range / 2);
    let range_shift = seg_count_x2 - search_range;

    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes()); // format
    let length_pos = sub.len();
    sub.extend_from_slice(&0u16.to_be_bytes()); // length (patched below)
    sub.extend_from_slice(&0u16.to_be_bytes()); // language
    sub.extend_from_slice(&seg_count_x2.to_be_bytes());
    sub.extend_from_slice(&search_range.to_be_bytes());
    sub.extend_from_slice(&entry_selector.to_be_bytes());
    sub.extend_from_slice(&range_shift.to_be_bytes());
    for &(_, end, _) in &segs {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for &(start, _, _) in &segs {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for (i, &(start, _, sgid)) in segs.iter().enumerate() {
        // idDelta such that (cp + idDelta) mod 65536 == gid; terminator maps 0.
        let delta = if i + 1 == segs.len() {
            1i16
        } else {
            (i32::from(sgid) - i32::from(start)) as i16
        };
        sub.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in &segs {
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }
    let len = sub.len() as u16;
    sub[length_pos..length_pos + 2].copy_from_slice(&len.to_be_bytes());
    sub
}

/// A format-12 subtable: contiguous (codepoint, gid) runs → SequentialMapGroup.
fn build_cmap_format12(mappings: &BTreeMap<u32, u16>) -> Vec<u8> {
    let mut groups: Vec<(u32, u32, u32)> = Vec::new(); // (start_cp, end_cp, start_gid)
    for (&cp, &gid) in mappings {
        match groups.last_mut() {
            Some((start, end, sgid))
                if cp == *end + 1 && u32::from(gid) == *sgid + (cp - *start) =>
            {
                *end = cp;
            }
            _ => groups.push((cp, cp, u32::from(gid))),
        }
    }
    let mut sub = Vec::new();
    sub.extend_from_slice(&12u16.to_be_bytes()); // format
    sub.extend_from_slice(&0u16.to_be_bytes()); // reserved
    sub.extend_from_slice(&((16 + groups.len() * 12) as u32).to_be_bytes()); // length
    sub.extend_from_slice(&0u32.to_be_bytes()); // language
    sub.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (start, end, sgid) in groups {
        sub.extend_from_slice(&start.to_be_bytes());
        sub.extend_from_slice(&end.to_be_bytes());
        sub.extend_from_slice(&sgid.to_be_bytes());
    }
    sub
}

/// A minimal version-3.0 `post` (no glyph names), carrying over the italic /
/// underline metrics from the original table when present.
fn build_post(original: Option<&[u8]>) -> Vec<u8> {
    let mut post = Vec::with_capacity(32);
    post.extend_from_slice(&0x0003_0000u32.to_be_bytes()); // version 3.0
    match original {
        Some(orig) if orig.len() >= 16 => post.extend_from_slice(&orig[4..16]),
        _ => post.extend_from_slice(&[0u8; 12]),
    }
    post.extend_from_slice(&[0u8; 16]); // min/maxMemType42 + min/maxMemType1
    post
}

/// Sum of big-endian u32 words, zero-padded to a 4-byte boundary (the sfnt
/// table-checksum algorithm).
fn sfnt_checksum(data: &[u8]) -> u32 {
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

/// Largest power of two `<= n` (`n >= 1`).
fn pow2_floor(n: u16) -> u16 {
    let mut p = 1u16;
    while p * 2 <= n {
        p *= 2;
    }
    p
}

/// `floor(log2(n))` for `n >= 1`.
fn log2_floor(n: u16) -> u16 {
    let mut p = 0u16;
    let mut v = n;
    while v > 1 {
        v /= 2;
        p += 1;
    }
    p
}

/// Lays out offset table + directory (tags pre-sorted) + 4-byte-aligned table
/// bodies, then patches `head.checkSumAdjustment`.
fn assemble_sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let num_tables = tables.len() as u16;
    let search_range = pow2_floor(num_tables) * 16;
    let entry_selector = log2_floor(pow2_floor(num_tables));
    let range_shift = num_tables * 16 - search_range;

    let mut running = 12 + 16 * tables.len();
    let mut offsets: Vec<u32> = Vec::with_capacity(tables.len());
    for (_, data) in tables {
        offsets.push(running as u32);
        running += data.len();
        running += (4 - running % 4) % 4;
    }

    let mut out = Vec::with_capacity(running);
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfnt version (glyf)
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());
    for (i, (tag, data)) in tables.iter().enumerate() {
        out.extend_from_slice(tag);
        out.extend_from_slice(&sfnt_checksum(data).to_be_bytes());
        out.extend_from_slice(&offsets[i].to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let mut head_offset = None;
    for (i, (tag, data)) in tables.iter().enumerate() {
        debug_assert_eq!(out.len() as u32, offsets[i]);
        if tag == b"head" {
            head_offset = Some(out.len());
        }
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    // checkSumAdjustment = 0xB1B0AFBA - checksum(font with the field zeroed);
    // the field is still zero in the buffer here.
    if let Some(pos) = head_offset {
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(sfnt_checksum(&out));
        out[pos + 8..pos + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    out
}
