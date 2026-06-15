//! A minimal, authored TrueType program with one box glyph mapping `'A'` → gid 1.
//!
//! Shared by the M6d render-page integration tests. License-clean (authored
//! in-code, no third-party font asset). Mirrors the synthesizer in
//! `render_text.rs`; kept here as its own integration-test module so multiple
//! test files can build a real embedded font.
#![allow(dead_code)]

fn checksum(d: &[u8]) -> u32 {
    let mut s = 0u32;
    let mut i = 0;
    while i < d.len() {
        let mut w = [0u8; 4];
        let n = (d.len() - i).min(4);
        w[..n].copy_from_slice(&d[i..i + n]);
        s = s.wrapping_add(u32::from_be_bytes(w));
        i += 4;
    }
    s
}

fn p2(n: u16) -> u16 {
    let mut p = 1;
    while p * 2 <= n {
        p *= 2;
    }
    p
}

fn l2(n: u16) -> u16 {
    let (mut p, mut v) = (0, n);
    while v > 1 {
        v /= 2;
        p += 1;
    }
    p
}

fn box_glyph() -> Vec<u8> {
    let mut g = Vec::new();
    g.extend_from_slice(&1i16.to_be_bytes());
    g.extend_from_slice(&100i16.to_be_bytes());
    g.extend_from_slice(&0i16.to_be_bytes());
    g.extend_from_slice(&900i16.to_be_bytes());
    g.extend_from_slice(&700i16.to_be_bytes());
    g.extend_from_slice(&3u16.to_be_bytes());
    g.extend_from_slice(&0u16.to_be_bytes());
    g.extend(std::iter::repeat_n(0x01u8, 4));
    let xs = [100i16, 900, 900, 100];
    let ys = [0i16, 0, 700, 700];
    let mut prev = 0i16;
    for &x in &xs {
        g.extend_from_slice(&(x - prev).to_be_bytes());
        prev = x;
    }
    let mut prev = 0i16;
    for &y in &ys {
        g.extend_from_slice(&(y - prev).to_be_bytes());
        prev = y;
    }
    g
}

fn cmap() -> Vec<u8> {
    let (end, start, delta) = (0x41u16, 0x41u16, (1i32 - 0x41) as i16);
    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes());
    let lp = sub.len();
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    let seg = 2u16;
    sub.extend_from_slice(&(seg * 2).to_be_bytes());
    let sr = 2 * p2(seg);
    sub.extend_from_slice(&sr.to_be_bytes());
    sub.extend_from_slice(&l2(sr / 2).to_be_bytes());
    sub.extend_from_slice(&(seg * 2 - sr).to_be_bytes());
    for &e in &[end, 0xFFFF] {
        sub.extend_from_slice(&e.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes());
    for &s in &[start, 0xFFFF] {
        sub.extend_from_slice(&s.to_be_bytes());
    }
    for &d in &[delta, 1] {
        sub.extend_from_slice(&d.to_be_bytes());
    }
    for _ in 0..2 {
        sub.extend_from_slice(&0u16.to_be_bytes());
    }
    let len = sub.len() as u16;
    sub[lp..lp + 2].copy_from_slice(&len.to_be_bytes());

    let mut c = Vec::new();
    c.extend_from_slice(&0u16.to_be_bytes());
    c.extend_from_slice(&1u16.to_be_bytes());
    c.extend_from_slice(&3u16.to_be_bytes());
    c.extend_from_slice(&1u16.to_be_bytes());
    c.extend_from_slice(&12u32.to_be_bytes());
    c.extend_from_slice(&sub);
    c
}

struct T {
    tag: [u8; 4],
    data: Vec<u8>,
    ck: u32,
}

/// Builds a minimal valid TrueType program (glyph 1 = box, maps `'A'`).
pub fn ttf() -> Vec<u8> {
    let num_glyphs = 2u16;
    let advance = 1000u16;
    let one = box_glyph();
    let mut glyf = Vec::new();
    let mut loca = vec![0u32, 0];
    glyf.extend_from_slice(&one);
    if !glyf.len().is_multiple_of(2) {
        glyf.push(0);
    }
    loca.push(glyf.len() as u32);
    let mut loca_b = Vec::new();
    for o in loca {
        loca_b.extend_from_slice(&o.to_be_bytes());
    }

    let mut head = Vec::new();
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    head.extend_from_slice(&0u32.to_be_bytes());
    head.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head.extend_from_slice(&0u16.to_be_bytes());
    head.extend_from_slice(&1000u16.to_be_bytes());
    head.extend_from_slice(&0i64.to_be_bytes());
    head.extend_from_slice(&0i64.to_be_bytes());
    head.extend_from_slice(&100i16.to_be_bytes());
    head.extend_from_slice(&0i16.to_be_bytes());
    head.extend_from_slice(&900i16.to_be_bytes());
    head.extend_from_slice(&700i16.to_be_bytes());
    head.extend_from_slice(&0u16.to_be_bytes());
    head.extend_from_slice(&8u16.to_be_bytes());
    head.extend_from_slice(&2i16.to_be_bytes());
    head.extend_from_slice(&1i16.to_be_bytes());
    head.extend_from_slice(&0i16.to_be_bytes());

    let mut hhea = Vec::new();
    hhea.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea.extend_from_slice(&800i16.to_be_bytes());
    hhea.extend_from_slice(&(-200i16).to_be_bytes());
    hhea.extend_from_slice(&0i16.to_be_bytes());
    hhea.extend_from_slice(&advance.to_be_bytes());
    hhea.extend_from_slice(&0i16.to_be_bytes());
    hhea.extend_from_slice(&0i16.to_be_bytes());
    hhea.extend_from_slice(&(advance as i16).to_be_bytes());
    hhea.extend_from_slice(&1i16.to_be_bytes());
    hhea.extend_from_slice(&0i16.to_be_bytes());
    hhea.extend_from_slice(&0i16.to_be_bytes());
    for _ in 0..4 {
        hhea.extend_from_slice(&0i16.to_be_bytes());
    }
    hhea.extend_from_slice(&0i16.to_be_bytes());
    hhea.extend_from_slice(&num_glyphs.to_be_bytes());

    let mut maxp = Vec::new();
    maxp.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp.extend_from_slice(&num_glyphs.to_be_bytes());
    maxp.extend_from_slice(&4u16.to_be_bytes());
    maxp.extend_from_slice(&1u16.to_be_bytes());
    for _ in 0..11 {
        maxp.extend_from_slice(&0u16.to_be_bytes());
    }

    let mut hmtx = Vec::new();
    for _ in 0..num_glyphs {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    let mut post = Vec::new();
    post.extend_from_slice(&0x0003_0000u32.to_be_bytes());
    post.extend_from_slice(&0i32.to_be_bytes());
    post.extend_from_slice(&(-200i16).to_be_bytes());
    post.extend_from_slice(&50i16.to_be_bytes());
    for _ in 0..5 {
        post.extend_from_slice(&0u32.to_be_bytes());
    }

    let mk = |tag: [u8; 4], data: Vec<u8>| T {
        tag,
        ck: checksum(&data),
        data,
    };
    let mut tables = vec![
        mk(*b"cmap", cmap()),
        mk(*b"glyf", glyf),
        mk(*b"head", head),
        mk(*b"hhea", hhea),
        mk(*b"hmtx", hmtx),
        mk(*b"loca", loca_b),
        mk(*b"maxp", maxp),
        mk(*b"post", post),
    ];
    tables.sort_by_key(|t| t.tag);

    let n = tables.len() as u16;
    let sr = p2(n) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&sr.to_be_bytes());
    out.extend_from_slice(&l2(p2(n)).to_be_bytes());
    out.extend_from_slice(&(n * 16 - sr).to_be_bytes());
    let mut running = 12 + 16 * tables.len();
    let mut offs = Vec::new();
    for t in &tables {
        offs.push(running as u32);
        running += t.data.len();
        running += (4 - running % 4) % 4;
    }
    for (i, t) in tables.iter().enumerate() {
        out.extend_from_slice(&t.tag);
        out.extend_from_slice(&t.ck.to_be_bytes());
        out.extend_from_slice(&offs[i].to_be_bytes());
        out.extend_from_slice(&(t.data.len() as u32).to_be_bytes());
    }
    let mut head_off = 0;
    for (i, t) in tables.iter().enumerate() {
        assert_eq!(out.len() as u32, offs[i]);
        if &t.tag == b"head" {
            head_off = out.len();
        }
        out.extend_from_slice(&t.data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_off + 8..head_off + 12].copy_from_slice(&adj.to_be_bytes());
    out
}
