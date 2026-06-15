//! Predefined CMap framework (PRD §8.5; ISO 32000-1 §9.7.5.2).
//!
//! A Type0 font's `/Encoding` may be:
//!
//! - **`Identity-H` / `Identity-V`** — fully bundled: 2-byte codes, `CID ==
//!   code`. These cover the overwhelming majority of real-world Type0 fonts
//!   (every subset-embedded CIDFontType2 uses Identity-H).
//! - **A predefined CJK CMap name** (e.g. `GBK-EUC-H`, `UniGB-UCS2-H`,
//!   `90ms-RKSJ-H`, …) — the **framework** is here, but oxide-pdf bundles only the
//!   Identity maps. The full Adobe predefined CJK CMap set (Adobe-Japan1 /
//!   GB1 / CNS1 / Korea1 ROS + their `-UCS2` tables) is large and is a
//!   **documented coverage gap** for this milestone (see
//!   `BUNDLED_PREDEFINED` / [`is_known_predefined`]).
//! - **An embedded CMap stream** — handled directly by [`crate::cmap`].

/// The names of the predefined CMaps that oxide-pdf bundles in full.
///
/// **Bundled:** `Identity-H`, `Identity-V` (2-byte identity, `CID == code`).
///
/// **Documented gap (not bundled):** the Adobe predefined CJK CMaps
/// (`Adobe-Japan1`, `Adobe-GB1`, `Adobe-CNS1`, `Adobe-Korea1`,
/// `Adobe-KR` rosters and their `Uni…-UCS2-{H,V}` ToUnicode tables, plus the
/// legacy `*-EUC-*` / `*-RKSJ-*` / `*-B5-*` encodings). A Type0 font using one
/// of these resolves `width`/`iter_codes` via the default 2-byte codespace but
/// yields `None` for `to_unicode` unless the font carries its own `/ToUnicode`.
pub const BUNDLED_PREDEFINED: &[&str] = &["Identity-H", "Identity-V"];

/// A classification of a Type0 `/Encoding` name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredefinedKind {
    /// `Identity-H` / `Identity-V`: 2-byte codes, `CID == code`.
    Identity,
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
        _ if is_known_predefined(name) => PredefinedKind::KnownUnbundled,
        _ => PredefinedKind::Unknown,
    }
}

/// Whether `name` is a recognized Adobe predefined CJK CMap name (bundled or
/// not). Used to distinguish "known but unsupported CMap" (a documented gap)
/// from "unknown name" (likely an embedded stream / typo) for diagnostics.
///
/// This is a *prefix/registry* heuristic over the four Adobe public ROS naming
/// families — it intentionally recognizes the families without enumerating
/// every member (the full set is the documented gap).
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
