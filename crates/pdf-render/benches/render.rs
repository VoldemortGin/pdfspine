//! Criterion benchmark for `render_page` so rasterizer perf regressions are
//! catchable WITHOUT pdfium / a Python venv (the one real remaining perf gap is
//! `render_page` vs pdfium2's C engine).
//!
//! The benchmark builds a single, self-contained MIXED page in raw PDF bytes:
//! many text show-ops (an embedded authored TrueType box font, the same glyph
//! repeated thousands of times so per-glyph caching matters), several vector
//! fills + strokes, and a raw RGB image XObject. It renders that page through
//! `render_page` at 150 dpi (the common screen-render resolution).
//!
//! The page is built once outside the timed loop; only `render_page` is timed.

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};

use pdf_core::{DocumentStore, Limits, ObjRef, Page};
use pdf_render::{render_page, RenderOptions};

// ===========================================================================
// Minimal classic-xref PDF builder (mirrors the tests/ builder).
// ===========================================================================

struct Pdf {
    objects: Vec<(u32, Vec<u8>)>,
}

impl Pdf {
    fn new() -> Self {
        Pdf {
            objects: Vec::new(),
        }
    }

    fn obj(mut self, num: u32, body: impl AsRef<[u8]>) -> Self {
        self.objects.push((num, body.as_ref().to_vec()));
        self
    }

    fn build(mut self) -> Vec<u8> {
        self.objects.sort_by_key(|(n, _)| *n);
        let max = self.objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = vec![0usize; (max + 1) as usize];
        for (num, body) in &self.objects {
            offsets[*num as usize] = out.len();
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_off = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", max + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for n in 1..=max {
            out.extend_from_slice(format!("{:010} 00000 n \n", offsets[n as usize]).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                max + 1,
                xref_off
            )
            .as_bytes(),
        );
        out
    }
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(format!("<< {} /Length {} >>\nstream\n", dict, data.len()).as_bytes());
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

// ===========================================================================
// Authored TrueType program: one box glyph (gid 1), maps `'A'`. Vendored from
// the test synth so the benchmark is self-contained + license-clean.
// ===========================================================================

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

/// A non-trivial authored glyph: two contours (an outer rounded body + an inner
/// counter), built from on-curve and off-curve (quadratic) points so the outline
/// extraction + path build is representative of a real letter (not a single
/// 4-point rectangle, which would under-represent the per-glyph outline cost).
fn box_glyph() -> Vec<u8> {
    // Outer contour: a rounded rectangle approximated with quadratic corners
    // (on/off alternating). 12 points. Inner contour: a smaller box, 4 points.
    // Points: (x, y, on_curve).
    let outer: &[(i16, i16, bool)] = &[
        (200, 0, true),
        (700, 0, true),
        (850, 0, false), // corner control
        (900, 150, true),
        (900, 550, true),
        (850, 700, false),
        (700, 700, true),
        (200, 700, true),
        (50, 700, false),
        (0, 550, true),
        (0, 150, true),
        (50, 0, false),
    ];
    let inner: &[(i16, i16, bool)] = &[
        (300, 200, true),
        (600, 200, true),
        (600, 500, true),
        (300, 500, true),
    ];

    let mut all: Vec<(i16, i16, bool)> = Vec::new();
    all.extend_from_slice(outer);
    all.extend_from_slice(inner);

    let mut g = Vec::new();
    g.extend_from_slice(&2i16.to_be_bytes()); // numberOfContours
    g.extend_from_slice(&0i16.to_be_bytes()); // xMin
    g.extend_from_slice(&0i16.to_be_bytes()); // yMin
    g.extend_from_slice(&900i16.to_be_bytes()); // xMax
    g.extend_from_slice(&700i16.to_be_bytes()); // yMax
                                                // endPtsOfContours
    g.extend_from_slice(&((outer.len() - 1) as u16).to_be_bytes());
    g.extend_from_slice(&((all.len() - 1) as u16).to_be_bytes());
    g.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
                                              // flags: ON_CURVE = 0x01, off-curve = 0x00 (long-form coords).
    for &(_, _, on) in &all {
        g.push(if on { 0x01 } else { 0x00 });
    }
    // x deltas (signed 16-bit each)
    let mut prev = 0i16;
    for &(x, _, _) in &all {
        g.extend_from_slice(&(x - prev).to_be_bytes());
        prev = x;
    }
    // y deltas
    let mut prev = 0i16;
    for &(_, y, _) in &all {
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
fn ttf() -> Vec<u8> {
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
    maxp.extend_from_slice(&16u16.to_be_bytes()); // maxPoints
    maxp.extend_from_slice(&2u16.to_be_bytes()); // maxContours
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

// ===========================================================================
// The mixed benchmark page.
// ===========================================================================

const MEDIA: &str = "[0 0 612 792]"; // US-Letter, the common screen-render page.

/// The font / descriptor / FontFile2 objects (resource `/F1`, objects 10-12).
fn font_objs() -> Vec<(u32, Vec<u8>)> {
    let prog = ttf();
    vec![
        (
            10,
            b"<< /Type /Font /Subtype /TrueType /BaseFont /BoxFont \
              /FirstChar 65 /LastChar 65 /Widths [1000] \
              /FontDescriptor 11 0 R /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ),
        (
            11,
            b"<< /Type /FontDescriptor /FontName /BoxFont /Flags 4 \
              /FontBBox [100 0 900 700] /ItalicAngle 0 /Ascent 800 /Descent -200 \
              /CapHeight 700 /StemV 80 /FontFile2 12 0 R >>"
                .to_vec(),
        ),
        (12, stream("/Length1 0", &prog)),
    ]
}

/// A small raw-RGB gradient image XObject (object 20, resource `/Im0`).
fn image_objs() -> Vec<(u32, Vec<u8>)> {
    let (w, h) = (64usize, 64usize);
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            data.push((x * 4) as u8);
            data.push((y * 4) as u8);
            data.push(128);
        }
    }
    vec![(
        20,
        stream(
            "/Type /XObject /Subtype /Image /Width 64 /Height 64 \
             /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &data,
        ),
    )]
}

/// Builds the mixed content stream: a grid of repeated text rows (many glyph
/// occurrences of the same `(font, gid)`), several colored vector fills, several
/// stroked paths, and one placed image.
fn mixed_content() -> Vec<u8> {
    let mut c = Vec::new();

    // --- vector fills: a column of colored rectangles ---
    let fills: &[(&str, i32)] = &[
        ("1 0 0 rg", 700),
        ("0 0.6 0 rg", 640),
        ("0 0 1 rg", 580),
        ("0.8 0.4 0 rg", 520),
        ("0.5 0 0.5 rg", 460),
    ];
    for (color, y) in fills {
        c.extend_from_slice(format!("{color} 40 {y} 120 40 re f\n").as_bytes());
    }

    // --- vector strokes: a fan of diagonal lines ---
    c.extend_from_slice(b"0 0 0 RG 2 w\n");
    for i in 0..24 {
        let x0 = 200 + i * 6;
        c.extend_from_slice(format!("{x0} 760 m {} 420 l S\n", 200 + i * 12).as_bytes());
    }
    // a dashed stroked rectangle
    c.extend_from_slice(b"0.2 0.2 0.2 RG 3 w [6 4] 0 d 420 600 150 120 re S\n");
    c.extend_from_slice(b"[] 0 d\n");

    // --- image placement ---
    c.extend_from_slice(b"q 120 0 0 120 440 420 cm /Im0 Do Q\n");

    // --- text: 30 rows x ~40 glyphs each = ~1200 glyph occurrences, all the
    //     same (font, gid). This is what stresses the per-glyph outline/path
    //     work; the box font remaps every shown 'A' to gid 1. ---
    let line: String = "A".repeat(40);
    for row in 0..30 {
        let y = 380 - row * 12;
        if y < 20 {
            break;
        }
        c.extend_from_slice(
            format!("BT /F1 11 Tf 1 0 0 1 30 {y} Tm 0 0 0 rg ({line}) Tj ET\n").as_bytes(),
        );
    }

    c
}

/// Assembles the full single-page mixed PDF.
fn mixed_pdf() -> Vec<u8> {
    let content = mixed_content();
    let mut pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox {MEDIA} \
                 /Resources << /Font << /F1 10 0 R >> /XObject << /Im0 20 0 R >> >> \
                 /Contents 4 0 R >>"
            )
            .into_bytes(),
        )
        .obj(4, stream("", &content));
    for (num, body) in font_objs() {
        pdf = pdf.obj(num, body);
    }
    for (num, body) in image_objs() {
        pdf = pdf.obj(num, body);
    }
    pdf.build()
}

fn open_page(bytes: Vec<u8>) -> (Arc<DocumentStore>, Page) {
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open pdf");
    let arc = Arc::new(doc);
    let page = Page::new(arc.clone(), 0, ObjRef::new(3, 0));
    (arc, page)
}

fn bench_render_page(c: &mut Criterion) {
    let (doc, page) = open_page(mixed_pdf());
    // 150 dpi → 612x792 pt page becomes ~1275x1650 px.
    let opts = RenderOptions {
        dpi: Some(150),
        ..RenderOptions::default()
    };
    // Sanity-check the page actually renders something before timing.
    let pm = render_page(&doc, &page, &opts).expect("render ok");
    assert_eq!((pm.width, pm.height), (1275, 1650));

    c.bench_function("render_page_mixed_150dpi", |b| {
        b.iter(|| {
            let pm = render_page(&doc, &page, &opts).expect("render ok");
            std::hint::black_box(pm);
        })
    });
}

criterion_group!(benches, bench_render_page);
criterion_main!(benches);
