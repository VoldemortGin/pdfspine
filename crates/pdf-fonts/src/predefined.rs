//! Predefined CMap framework (PRD §8.5; ISO 32000-1 §9.7.5.2).
//!
//! A Type0 font's `/Encoding` may be:
//!
//! - **`Identity-H` / `Identity-V`** — 2-byte codes, `CID == code`. These cover
//!   the overwhelming majority of real-world Type0 fonts (every subset-embedded
//!   CIDFontType2 uses Identity-H).
//! - **A bundled predefined CJK CMap name** — the four Adobe *UCS2* families
//!   (`UniGB-UCS2-{H,V}`, `UniCNS-UCS2-{H,V}`, `UniJIS-UCS2-{H,V}`,
//!   `UniKS-UCS2-{H,V}`). For these oxide-pdf bundles the Adobe encoding CMap
//!   (code → CID) and derives the matching ToUnicode table (CID → Unicode) by
//!   inverting it, so a CJK PDF with no embedded `/ToUnicode` still extracts
//!   Unicode. See [`cid_to_unicode`] / [`encoding_cmap`].
//! - **A recognized-but-unbundled predefined CJK CMap name** (e.g. `GBK-EUC-H`,
//!   `90ms-RKSJ-H`, `ETen-B5-H`) — the legacy code→CID encodings whose tables
//!   are **not** bundled (a documented coverage gap). A Type0 font using one
//!   resolves `width`/`iter_codes` via the default 2-byte codespace but yields
//!   `None` for `to_unicode` unless the font carries its own `/ToUnicode`.
//! - **An embedded CMap stream** — handled directly by [`crate::cmap`].

use std::sync::LazyLock;

use smol_str::SmolStr;

use crate::cmap::{CMap, CidUnicode};

/// The names of the predefined CMaps that oxide-pdf bundles in full.
///
/// **Bundled:** `Identity-H`, `Identity-V` (2-byte identity, `CID == code`) plus
/// the four Adobe UCS2 families and their vertical variants (see
/// [`BundledCjk`]).
pub const BUNDLED_PREDEFINED: &[&str] = &[
    "Identity-H",
    "Identity-V",
    "UniGB-UCS2-H",
    "UniGB-UCS2-V",
    "UniCNS-UCS2-H",
    "UniCNS-UCS2-V",
    "UniJIS-UCS2-H",
    "UniJIS-UCS2-V",
    "UniKS-UCS2-H",
    "UniKS-UCS2-V",
];

/// A classification of a Type0 `/Encoding` name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredefinedKind {
    /// `Identity-H` / `Identity-V`: 2-byte codes, `CID == code`.
    Identity,
    /// A bundled predefined CJK CMap (one of the four UCS2 families): the
    /// encoding CMap and the inverted CID→Unicode table are available.
    Cjk,
    /// A recognized predefined CJK CMap name we do **not** bundle (gap).
    KnownUnbundled,
    /// Not a recognized predefined CMap name (likely an embedded-stream
    /// encoding or an unknown / malformed name).
    Unknown,
}

/// Classifies a `/Encoding` name.
#[must_use]
pub fn classify(name: &[u8]) -> PredefinedKind {
    match name {
        b"Identity-H" | b"Identity-V" | b"Identity" => PredefinedKind::Identity,
        _ if BundledCjk::from_name(name).is_some() => PredefinedKind::Cjk,
        _ if is_known_predefined(name) => PredefinedKind::KnownUnbundled,
        _ => PredefinedKind::Unknown,
    }
}

/// The four Adobe character collections whose UCS2 CMaps are bundled.
///
/// The horizontal (`-H`) and vertical (`-V`) names of a collection share one
/// CID space, so they resolve the same CID→Unicode table here (orientation only
/// affects glyph placement, not code points).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundledCjk {
    /// Adobe-GB1 (Simplified Chinese) — `UniGB-UCS2-{H,V}`.
    Gb1,
    /// Adobe-CNS1 (Traditional Chinese) — `UniCNS-UCS2-{H,V}`.
    Cns1,
    /// Adobe-Japan1 — `UniJIS-UCS2-{H,V}`.
    Japan1,
    /// Adobe-Korea1 — `UniKS-UCS2-{H,V}`.
    Korea1,
}

impl BundledCjk {
    /// Maps a predefined `/Encoding` name to a bundled collection, if it is one
    /// of the eight bundled UCS2 names.
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<BundledCjk> {
        match name {
            b"UniGB-UCS2-H" | b"UniGB-UCS2-V" => Some(BundledCjk::Gb1),
            b"UniCNS-UCS2-H" | b"UniCNS-UCS2-V" => Some(BundledCjk::Cns1),
            b"UniJIS-UCS2-H" | b"UniJIS-UCS2-V" => Some(BundledCjk::Japan1),
            b"UniKS-UCS2-H" | b"UniKS-UCS2-V" => Some(BundledCjk::Korea1),
            _ => None,
        }
    }

    /// The bundled raw CMap program (Adobe `Uni…-UCS2-H`, BSD-3-Clause).
    fn raw(self) -> &'static [u8] {
        match self {
            BundledCjk::Gb1 => include_bytes!("../data/cmap/UniGB-UCS2-H"),
            BundledCjk::Cns1 => include_bytes!("../data/cmap/UniCNS-UCS2-H"),
            BundledCjk::Japan1 => include_bytes!("../data/cmap/UniJIS-UCS2-H"),
            BundledCjk::Korea1 => include_bytes!("../data/cmap/UniKS-UCS2-H"),
        }
    }

    /// The lazily-parsed-and-inverted CID→Unicode index for this collection.
    fn cid_unicode(self) -> &'static CidUnicode {
        match self {
            BundledCjk::Gb1 => &GB1,
            BundledCjk::Cns1 => &CNS1,
            BundledCjk::Japan1 => &JAPAN1,
            BundledCjk::Korea1 => &KOREA1,
        }
    }

    /// The lazily-parsed forward encoding CMap (code → CID) for this collection.
    fn encoding(self) -> &'static CMap {
        match self {
            BundledCjk::Gb1 => &GB1_ENC,
            BundledCjk::Cns1 => &CNS1_ENC,
            BundledCjk::Japan1 => &JAPAN1_ENC,
            BundledCjk::Korea1 => &KOREA1_ENC,
        }
    }

    /// Maps a content-stream character code to a CID via this collection's
    /// bundled encoding CMap. `None` when the code is outside every range.
    #[must_use]
    pub fn code_to_cid(self, code: u32) -> Option<u32> {
        self.encoding().cid(code)
    }

    /// Maps a CID to its Unicode character via this collection's inverted table.
    #[must_use]
    pub fn cid_to_unicode(self, cid: u32) -> Option<SmolStr> {
        self.cid_unicode().get(cid)
    }

    /// The codespace ranges of this collection's encoding CMap (drives
    /// variable-length code iteration; the UCS2 families are 2-byte).
    #[must_use]
    pub fn codespace(self) -> &'static [crate::cmap::CodespaceRange] {
        self.encoding().codespace()
    }
}

/// Parses a bundled raw CMap (no `usecmap` chaining occurs in the UCS2 files).
fn parse_bundled(raw: &[u8]) -> CMap {
    let mut no_use = |_: &[u8]| None;
    CMap::parse(raw, &mut no_use)
}

// Forward encoding CMaps (code → CID), parsed once on first use.
static GB1_ENC: LazyLock<CMap> = LazyLock::new(|| parse_bundled(BundledCjk::Gb1.raw()));
static CNS1_ENC: LazyLock<CMap> = LazyLock::new(|| parse_bundled(BundledCjk::Cns1.raw()));
static JAPAN1_ENC: LazyLock<CMap> = LazyLock::new(|| parse_bundled(BundledCjk::Japan1.raw()));
static KOREA1_ENC: LazyLock<CMap> = LazyLock::new(|| parse_bundled(BundledCjk::Korea1.raw()));

// Inverted CID → Unicode indices, derived once on first use.
static GB1: LazyLock<CidUnicode> = LazyLock::new(|| GB1_ENC.invert_to_cid_unicode());
static CNS1: LazyLock<CidUnicode> = LazyLock::new(|| CNS1_ENC.invert_to_cid_unicode());
static JAPAN1: LazyLock<CidUnicode> = LazyLock::new(|| JAPAN1_ENC.invert_to_cid_unicode());
static KOREA1: LazyLock<CidUnicode> = LazyLock::new(|| KOREA1_ENC.invert_to_cid_unicode());

/// Resolves `CID → Unicode` for a bundled predefined CJK CMap name.
///
/// Returns `None` when `name` is not a bundled UCS2 family, or the CID is not in
/// the table. Never panics. This is the extraction path for a Type0 font whose
/// `/Encoding` is a predefined CJK name and which carries no `/ToUnicode`.
#[must_use]
pub fn cid_to_unicode(name: &str, cid: u32) -> Option<SmolStr> {
    BundledCjk::from_name(name.as_bytes())?
        .cid_unicode()
        .get(cid)
}

/// The bundled forward encoding CMap (code → CID) for a predefined CJK name, if
/// bundled. Used to map content-stream codes to CIDs for the bundled families.
#[must_use]
pub fn encoding_cmap(name: &[u8]) -> Option<&'static CMap> {
    BundledCjk::from_name(name).map(BundledCjk::encoding)
}

/// Whether `name` is a recognized Adobe predefined CJK CMap name (bundled or
/// not). Used to distinguish "known but unsupported CMap" (a documented gap)
/// from "unknown name" (likely an embedded stream / typo) for diagnostics.
///
/// This is a *prefix/registry* heuristic over the four Adobe public ROS naming
/// families — it intentionally recognizes the families without enumerating
/// every member (the legacy set is the documented gap).
#[must_use]
pub fn is_known_predefined(name: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(name) else {
        return false;
    };
    // Adobe public predefined-CMap naming families (ISO 32000-1 Annex; Adobe
    // tech notes #5078/#5079/#5080/#5094). Recognized by the documented prefixes
    // and the orientation suffix.
    const FAMILIES: &[&str] = &[
        // Chinese (Simplified) — Adobe-GB1
        "GB-", "GBpc-", "GBK-", "GBK2K-", "GBKp-", "UniGB-",
        // Chinese (Traditional) — Adobe-CNS1
        "B5pc-", "HKscs-", "ETen-", "ETenms-", "CNS-", "UniCNS-",
        // Japanese — Adobe-Japan1
        "83pv-", "90ms-", "90msp-", "90pv-", "Add-", "EUC-", "Ext-", "H", "V", "NWP-", "RKSJ-",
        "UniJIS-", "UniJISX", // Korean — Adobe-Korea1 / Adobe-KR
        "KSC-", "KSCms-", "KSCpc-", "UniKS-", "UniAKR",
    ];
    FAMILIES.iter().any(|p| s.starts_with(p))
}
