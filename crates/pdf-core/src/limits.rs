//! Resource ceilings — the "never OOM / never hang" guard (PRD §9.6, §9.6.2).
//!
//! Untrusted PDFs are an attack surface (PRD §9.6.1, R12): a tiny compressed
//! stream can declare or expand to gigabytes (a *decompression bomb*). Every
//! decoder threads a `&Limits` and refuses to allocate past
//! [`Limits::max_decompressed_stream`], returning [`crate::Error::LimitExceeded`]
//! instead of OOMing. The shipped defaults are **pinned in PRD §9.6.2** so the
//! gate is testable (`LIMITS-DEFAULT-*`).
//!
//! The filter layer enforces [`Limits::max_decompressed_stream`] and
//! [`Limits::max_decode_ratio`]; the document store enforces
//! [`Limits::max_file_size`] at open and [`Limits::max_total_decompressed`]
//! across its stream decodes; the parser and graph walks enforce
//! [`Limits::max_recursion_depth`].

/// Resource ceilings for parsing and decoding (PRD §9.6.2).
///
/// Construct with [`Limits::default`] (the pinned §9.6.2 defaults) and override
/// individual fields as needed; everything is plain data.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct Limits {
    /// Largest accepted source file, in bytes. Default 4 GiB (§9.6.2:
    /// i32-offset safety + practical ceiling). Checked when a document is
    /// opened ([`crate::error::LimitKind::FileSize`]).
    pub max_file_size: u64,
    /// Largest accepted object count. Default 2²³ (§9.6.2: xref/object-count
    /// bomb bound).
    pub max_objects: u64,
    /// Maximum dict/array/XObject nesting depth. Default 256 (§9.6.2).
    pub max_recursion_depth: u32,
    /// Largest decoded size of a **single** stream, in bytes. Default 1 GiB
    /// (§9.6.2: single-stream bomb cap). Enforced by every M1b decoder.
    pub max_decompressed_stream: usize,
    /// Whole-document decompression budget, in bytes. Default 4 GiB (§9.6.2).
    /// Enforced by [`crate::DocumentStore::decode_stream`] over the **distinct**
    /// non-image streams a document decodes: decoding the same stream again
    /// (another page using a shared Form, a re-render) is not charged twice.
    /// Image XObjects are exempt: rendering a long scanned document legitimately
    /// decodes far more image data than this over a document's lifetime, and
    /// each image decode is already bounded by
    /// [`Limits::max_decompressed_stream`] and [`Limits::max_decode_ratio`].
    /// Past the budget, decoding a not-yet-seen stream fails with
    /// [`crate::error::LimitKind::TotalDecompressed`]. The largest fixture
    /// decodes ~26 MB of distinct streams.
    pub max_total_decompressed: u64,
    /// Per-ObjStm member cap. Default 1,048,576 (§9.6.2).
    pub max_objstm_objects: u64,
    /// Incremental decode-ratio trip for zip bombs (output : input). Default
    /// 200 (§9.6.2). A stream whose output exceeds `input * max_decode_ratio`
    /// trips [`crate::error::LimitKind::DecodeRatio`] — checked while
    /// inflating (Flate, LZW) and across the whole filter chain — but only
    /// once the output is past [`Limits::decode_ratio_floor`]: real streams
    /// are routinely far more compressible than 200:1 (a fixture form carries
    /// a 25 MB stream at 320:1, a near-blank page image can approach deflate's
    /// ~1032:1 maximum), so the ratio only cuts off decodes already a quarter
    /// of the way to the single-stream cap.
    pub max_decode_ratio: u64,
    /// Per-page content work budget: content-stream tokens (operands and
    /// operators) interpreted for one page, across its content streams, Form
    /// XObjects, annotation appearances and (when rendering) Type 3 glyph
    /// procedures; each Form / glyph-procedure invocation additionally costs
    /// [`Limits::CONTENT_INVOCATION_COST`]. Exhausting it truncates the page's
    /// interpretation (what was produced so far is kept). Default 25,000,000:
    /// at ~3.3 bytes per token a page would need ~80 MB of decoded content to
    /// reach it (the largest fixture page has 0.21 M tokens in 0.7 MB, ~120×
    /// below), while Form XObject fan-out — `Do` trees that re-run shared forms
    /// 10× per level — stops at a fixed cost (about a second of interpretation)
    /// however deep it goes, instead of growing 10× per level.
    pub max_page_content_ops: u64,
    /// Per-page output budget: glyphs, vector paths and images emitted by one
    /// page's interpretation. Default 5,000,000 (≈0.5–1 GB of extracted
    /// geometry; the busiest fixture page emits ~14 k glyphs and ~4 k paths).
    pub max_page_content_items: u64,
}

impl Limits {
    /// The pinned PRD §9.6.2 defaults, as an associated const for use in
    /// `const` contexts and tests (`LIMITS-DEFAULT-*`).
    pub const DEFAULT: Limits = Limits {
        max_file_size: 4 * GIB,
        max_objects: 8_388_608, // 2^23
        max_recursion_depth: 256,
        max_decompressed_stream: GIB as usize,
        max_total_decompressed: 4 * GIB,
        max_objstm_objects: 1_048_576,
        max_decode_ratio: 200,
        max_page_content_ops: 25_000_000,
        max_page_content_items: 5_000_000,
    };

    /// The [`Limits::max_page_content_ops`] charge for invoking one Form
    /// XObject or Type 3 glyph procedure, on top of its own tokens: resolving
    /// the XObject, decoding its stream and cloning its resources measured at
    /// ~3.5 µs per invocation, roughly the cost of interpreting 35–70 tokens.
    pub const CONTENT_INVOCATION_COST: u64 = 64;

    /// A permissive instance for trusted inputs / tests that intentionally
    /// decode large outputs. `max_decompressed_stream` is `usize::MAX` and the
    /// ratio guard is effectively disabled; all other ceilings keep the §9.6.2
    /// defaults. Not a default — opt in explicitly.
    #[must_use]
    pub fn unbounded_decode() -> Limits {
        Limits {
            max_decompressed_stream: usize::MAX,
            max_decode_ratio: u64::MAX,
            ..Limits::DEFAULT
        }
    }

    // --- builder-style overrides ------------------------------------------
    //
    // `Limits` is `#[non_exhaustive]`, so downstream crates/tests cannot use a
    // struct-update literal to tweak one field. These consuming setters provide
    // that ergonomics without exposing the field set as a stable constructor.

    /// The decoded size past which [`Limits::max_decode_ratio`] applies: a
    /// quarter of [`Limits::max_decompressed_stream`] (256 MiB by default, 10×
    /// the largest fixture stream).
    #[must_use]
    pub fn decode_ratio_floor(&self) -> usize {
        self.max_decompressed_stream / 4
    }

    /// Whether `out_len` decoded bytes from `in_len` encoded bytes trip the
    /// decode-ratio guard (see [`Limits::max_decode_ratio`]).
    #[must_use]
    pub fn decode_ratio_exceeded(&self, in_len: usize, out_len: usize) -> bool {
        out_len > self.decode_ratio_floor()
            && out_len as u64 > (in_len as u64).saturating_mul(self.max_decode_ratio)
    }

    /// Returns a copy with [`Limits::max_recursion_depth`] overridden.
    #[must_use]
    pub fn with_max_recursion_depth(mut self, v: u32) -> Self {
        self.max_recursion_depth = v;
        self
    }

    /// Returns a copy with [`Limits::max_objstm_objects`] overridden.
    #[must_use]
    pub fn with_max_objstm_objects(mut self, v: u64) -> Self {
        self.max_objstm_objects = v;
        self
    }

    /// Returns a copy with [`Limits::max_objects`] overridden.
    #[must_use]
    pub fn with_max_objects(mut self, v: u64) -> Self {
        self.max_objects = v;
        self
    }

    /// Returns a copy with [`Limits::max_page_content_ops`] overridden.
    #[must_use]
    pub fn with_max_page_content_ops(mut self, v: u64) -> Self {
        self.max_page_content_ops = v;
        self
    }

    /// Returns a copy with [`Limits::max_page_content_items`] overridden.
    #[must_use]
    pub fn with_max_page_content_items(mut self, v: u64) -> Self {
        self.max_page_content_items = v;
        self
    }
}

/// One gibibyte, the unit several §9.6.2 ceilings are pinned in.
const GIB: u64 = 1024 * 1024 * 1024;

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
