//! System font resolution & management (TS-2, PRD §10 scope (b)).
//!
//! [`fontdb`] supplies enumeration only — the directory scan, TTC face
//! enumeration (`FaceInfo.index`) and `(bytes, face_index)` access. Its own
//! `query` is exact/case-sensitive, so everything smarter is first-party here
//! (locked design, 2026-07-02 family brief):
//!
//! 1. a **normalized (case/width-folded) family-name index** over every
//!    localized name-table family record (so `宋体` matches a zh-localized
//!    name, and `ＣＡＬＩＢＲＩ` still finds Calibri);
//! 2. bold/italic → **weight/style matching** over a family's faces (nearest
//!    face; **no synthetic bold** — a missing slot degrades to the nearest
//!    style + [`ExportWarning::StyleApproximated`]);
//! 3. a **configurable substitution table** with built-in three-platform
//!    defaults ([`Substitutions::builtin`]);
//! 4. a **per-character fallback chain** ([`FontResolver::resolve_char`]);
//! 5. a final fallback to the **bundled Liberation/Noto** faces
//!    (`crates/pdf-fonts/fonts/`), which are always loaded into the database —
//!    so resolution is total: every request returns *some* face, every
//!    degradation appends an [`ExportWarning`], never an error (house rule:
//!    degrade-never-panic).
//!
//! # Resolution order
//!
//! `requested family` → substitution candidates (in table order) → bundled
//! Liberation Sans. The first two steps match through the folded-name index;
//! a hit past step one appends exactly one
//! [`ExportWarning::FontSubstituted`].
//!
//! # `memmap` decision (recorded)
//!
//! fontdb's `memmap` feature is **ON** (see the root `Cargo.toml` note): macOS
//! keeps multi-GB CJK collections under `/System/Library/AssetsV2` (PingFang
//! lives there on modern macOS), and an eager-read scan would cost seconds and
//! GBs of transient allocations. The `unsafe` stays inside the `memmap2` leaf;
//! this crate remains `#![forbid(unsafe_code)]`. Face bytes handed out by
//! [`FontResolver::face_data`] are copied once per used face into an
//! `Arc<Vec<u8>>` cache (the embed path needs owned bytes anyway, TS-3).
//!
//! # Determinism
//!
//! Per-font-environment (PRD §10): same machine + same installed fonts ⇒
//! identical picks (candidate ties break on PostScript name / TTC index).
//! [`FontResolver::without_system_fonts`] / [`FontResolver::with_platform`]
//! (bundled faces only, injectable fixtures via
//! [`FontResolver::add_font_data`]) are fully deterministic for tests.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use fontdb::{Database, Source, Style};
use unicode_normalization::UnicodeNormalization;

use pdf_fonts::liberation::{liberation_face, symbol_faces, zapf_faces, LiberationFamily};

use crate::warn::ExportWarning;

/// The folded name of the guaranteed bundled final-fallback family.
const FALLBACK_FAMILY_FOLDED: &str = "liberation sans";

/// Weight at or above which a face counts as bold (CSS SemiBold boundary).
const BOLD_WEIGHT_MIN: u16 = 600;

/// Bundled families appended to every per-character fallback chain, in order:
/// the Noto symbol repertoire first, then Liberation for Latin coverage.
const BUNDLED_CHAR_FALLBACK: &[&str] = &[
    "Noto Sans Math",
    "Noto Sans Symbols 2",
    "Noto Sans Symbols",
    "Liberation Sans",
];

/// The OS family whose built-in substitution table / char-fallback chain to
/// use. Decoupled from `target_os` so any table can be exercised on any
/// machine (deterministic CI tests).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Platform {
    /// macOS (Songti SC / PingFang SC world).
    MacOs,
    /// Windows (SimSun / Microsoft YaHei world).
    Windows,
    /// Linux and everything else (Noto CJK world).
    Linux,
}

impl Platform {
    /// The platform this binary is running on (non-mac/windows ⇒ [`Linux`]).
    ///
    /// [`Linux`]: Platform::Linux
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// The configurable family substitution table: requested family → candidate
/// families tried in order (all lookups case/width-folded).
#[derive(Clone, Debug, Default)]
pub struct Substitutions {
    /// Folded requested name → candidate family names, in priority order.
    map: HashMap<String, Vec<String>>,
}

impl Substitutions {
    /// An empty table (no substitutions; unknown families go straight to the
    /// bundled fallback).
    #[must_use]
    pub fn empty() -> Self {
        Substitutions::default()
    }

    /// The built-in defaults for `platform` (PRD §10 locked table):
    ///
    /// | requested | macOS | Windows | Linux |
    /// |---|---|---|---|
    /// | 宋体 / SimSun | Songti SC | SimSun | Noto Serif CJK SC |
    /// | 微软雅黑 / Microsoft YaHei | PingFang SC | Microsoft YaHei | Noto Sans CJK SC |
    /// | Calibri | Carlito (if present) | Carlito (if present) | Carlito (if present) |
    /// | Times New Roman | bundled Liberation Serif | ditto | ditto |
    ///
    /// Each macOS/Linux CJK entry carries one secondary candidate (Hiragino
    /// Sans GB / STSong; the short-name Noto spellings) because the primary's
    /// availability varies by OS release; a candidate hit still reports the
    /// substitution.
    #[must_use]
    pub fn builtin(platform: Platform) -> Self {
        type Entry = (&'static [&'static str], &'static [&'static str]);
        let entries: &[Entry] = match platform {
            Platform::MacOs => &[
                (&["宋体", "SimSun"], &["Songti SC", "STSong"]),
                (
                    &["微软雅黑", "Microsoft YaHei"],
                    &["PingFang SC", "Hiragino Sans GB"],
                ),
                (&["Calibri"], &["Carlito"]),
                (&["Times New Roman"], &["Liberation Serif"]),
            ],
            Platform::Windows => &[
                (&["宋体", "SimSun"], &["SimSun"]),
                (&["微软雅黑", "Microsoft YaHei"], &["Microsoft YaHei"]),
                (&["Calibri"], &["Carlito"]),
                (&["Times New Roman"], &["Liberation Serif"]),
            ],
            Platform::Linux => &[
                (&["宋体", "SimSun"], &["Noto Serif CJK SC", "Noto Serif SC"]),
                (
                    &["微软雅黑", "Microsoft YaHei"],
                    &["Noto Sans CJK SC", "Noto Sans SC"],
                ),
                (&["Calibri"], &["Carlito"]),
                (&["Times New Roman"], &["Liberation Serif"]),
            ],
        };
        let mut table = Substitutions::default();
        for (requested, candidates) in entries {
            for name in *requested {
                table.set(name, candidates);
            }
        }
        table
    }

    /// Sets (or replaces) the candidate list for one requested family.
    pub fn set(&mut self, requested: &str, candidates: &[&str]) {
        self.map.insert(
            fold(requested),
            candidates.iter().map(|c| (*c).to_string()).collect(),
        );
    }

    /// The candidates for a folded requested name (empty when absent).
    fn candidates(&self, folded: &str) -> &[String] {
        self.map.get(folded).map_or(&[], Vec::as_slice)
    }
}

/// An opaque handle to one enumerated face (family × style × TTC index).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FaceKey(fontdb::ID);

/// One resolved face: the handle plus everything a consumer / the embed
/// registry (TS-3) needs to identify and load it.
#[derive(Clone, Debug)]
pub struct ResolvedFace {
    key: FaceKey,
    /// The family actually used (first name-table family record, canonical).
    pub family: String,
    /// The face's PostScript name (deterministic tie-break identity).
    pub post_script_name: String,
    /// The TTC face index (0 for standalone TTF/OTF) — feed to
    /// `EmbeddedFont::parse_indexed` (TS-3).
    pub index: u32,
    /// The originally requested bold flag (drives per-char fallback picks).
    bold: bool,
    /// The originally requested italic flag.
    italic: bool,
}

impl ResolvedFace {
    /// The opaque face handle (stable within one resolver).
    #[must_use]
    pub fn key(&self) -> FaceKey {
        self.key
    }
}

/// The font resolver: a [`fontdb`] database (system faces and/or injected
/// fixtures, plus the always-loaded bundled Liberation/Noto faces) behind the
/// first-party resolution layers described in the module docs.
pub struct FontResolver {
    db: Database,
    /// Folded family name → face IDs carrying that name.
    index: HashMap<String, Vec<fontdb::ID>>,
    subst: Substitutions,
    /// Folded family names of the per-character fallback chain, in order.
    char_chain: Vec<String>,
    /// Bundled Liberation Sans Regular — the guaranteed terminal fallback.
    fallback_regular: fontdb::ID,
    /// Owned face bytes, copied once per used face.
    bytes: RefCell<HashMap<fontdb::ID, Arc<Vec<u8>>>>,
    /// Memoized per-(face, char) cmap coverage.
    coverage: RefCell<HashMap<(fontdb::ID, char), bool>>,
}

impl FontResolver {
    /// A resolver over the **system fonts** plus the bundled faces, with the
    /// current platform's built-in substitution table. This scans the OS font
    /// directories (macOS incl. `/System/Library/AssetsV2` font assets; Linux
    /// via pure-Rust fontconfig parsing) — output becomes deterministic *per
    /// font environment* only.
    #[must_use]
    pub fn with_system_fonts() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        Self::build(db, Platform::current())
    }

    /// A fully deterministic resolver: **bundled faces only** (12 Liberation +
    /// 3 Noto symbol faces), current platform's built-in table. Inject fixture
    /// fonts with [`FontResolver::add_font_data`].
    #[must_use]
    pub fn without_system_fonts() -> Self {
        Self::build(Database::new(), Platform::current())
    }

    /// Like [`FontResolver::without_system_fonts`], but with `platform`'s
    /// built-in substitution table and char-fallback chain — lets tests
    /// exercise any platform's tables on any machine.
    #[must_use]
    pub fn with_platform(platform: Platform) -> Self {
        Self::build(Database::new(), platform)
    }

    /// Builds the resolver: loads the bundled faces, indexes every family
    /// name, and pins the guaranteed terminal fallback face.
    fn build(mut db: Database, platform: Platform) -> Self {
        load_bundled(&mut db);
        let index = build_index(&db);
        let fallback_regular = index
            .get(FALLBACK_FAMILY_FOLDED)
            .and_then(|ids| pick(&db, ids, false, false))
            .expect("bundled Liberation faces must load (build-time asset invariant)");
        let sys_chain: &[&str] = match platform {
            Platform::MacOs => &[
                "PingFang SC",
                "Hiragino Sans GB",
                "Songti SC",
                "Heiti SC",
                "Hiragino Sans",
                "Apple SD Gothic Neo",
                "Apple Symbols",
                "Arial Unicode MS",
            ],
            Platform::Windows => &[
                "Microsoft YaHei",
                "SimSun",
                "Yu Gothic UI",
                "Malgun Gothic",
                "Segoe UI Symbol",
                "Arial Unicode MS",
            ],
            Platform::Linux => &[
                "Noto Sans CJK SC",
                "Noto Sans SC",
                "Noto Serif CJK SC",
                "WenQuanYi Zen Hei",
                "DejaVu Sans",
            ],
        };
        let char_chain = sys_chain
            .iter()
            .chain(BUNDLED_CHAR_FALLBACK)
            .map(|name| fold(name))
            .collect();
        FontResolver {
            db,
            index,
            subst: Substitutions::builtin(platform),
            char_chain,
            fallback_regular,
            bytes: RefCell::new(HashMap::new()),
            coverage: RefCell::new(HashMap::new()),
        }
    }

    /// Adds a font program (TTF / OTF / **TTC** — every collection face is
    /// enumerated) to the database, e.g. a test fixture or a document-embedded
    /// font, and refreshes the name index.
    pub fn add_font_data(&mut self, data: Vec<u8>) {
        self.db.load_font_source(Source::Binary(Arc::new(data)));
        self.index = build_index(&self.db);
    }

    /// Replaces the substitution table (drops the built-in defaults).
    pub fn set_substitutions(&mut self, subst: Substitutions) {
        self.subst = subst;
    }

    /// Sets (or replaces) the candidates for one requested family on top of
    /// the current table.
    pub fn add_substitution(&mut self, requested: &str, candidates: &[&str]) {
        self.subst.set(requested, candidates);
    }

    /// The number of enumerated faces (bundled + system + injected).
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.db.faces().count()
    }

    /// Resolves a family + bold/italic request to a face. Total: falls through
    /// requested family → substitution candidates → bundled Liberation Sans,
    /// appending [`ExportWarning::FontSubstituted`] on any family change and
    /// [`ExportWarning::StyleApproximated`] when the chosen family misses the
    /// requested style slot (never an error, never synthetic bold).
    pub fn resolve(
        &self,
        family: &str,
        bold: bool,
        italic: bool,
        warnings: &mut Vec<ExportWarning>,
    ) -> ResolvedFace {
        let folded = fold(family);
        let mut resolved = self.query_family(&folded, bold, italic);
        if resolved.is_none() {
            for candidate in self.subst.candidates(&folded) {
                if let Some(face) = self.query_family(&fold(candidate), bold, italic) {
                    warnings.push(ExportWarning::FontSubstituted {
                        requested: family.to_string(),
                        used: face.family.clone(),
                    });
                    resolved = Some(face);
                    break;
                }
            }
        }
        let face = match resolved {
            Some(face) => face,
            None => {
                let face = self
                    .query_family(FALLBACK_FAMILY_FOLDED, bold, italic)
                    .unwrap_or_else(|| self.terminal_fallback(bold, italic));
                warnings.push(ExportWarning::FontSubstituted {
                    requested: family.to_string(),
                    used: face.family.clone(),
                });
                face
            }
        };
        if let Some(info) = self.db.face(face.key.0) {
            let got_bold = info.weight.0 >= BOLD_WEIGHT_MIN;
            let got_italic = matches!(info.style, Style::Italic | Style::Oblique);
            if got_bold != bold || got_italic != italic {
                warnings.push(ExportWarning::StyleApproximated {
                    family: face.family.clone(),
                    bold,
                    italic,
                });
            }
        }
        face
    }

    /// Resolves one character against `base`, walking the per-character
    /// fallback chain (platform CJK/symbol families, then the bundled Noto /
    /// Liberation tail) when `base` has no glyph for it. Appends
    /// [`ExportWarning::GlyphFallback`] whenever `base` cannot draw `ch` —
    /// whether a chain face takes over or (chain exhausted) `base` is returned
    /// to degrade as `.notdef`.
    pub fn resolve_char(
        &self,
        base: &ResolvedFace,
        ch: char,
        warnings: &mut Vec<ExportWarning>,
    ) -> ResolvedFace {
        if self.has_glyph(base, ch) {
            return base.clone();
        }
        for folded in &self.char_chain {
            if let Some(face) = self.query_family(folded, base.bold, base.italic) {
                if face.key != base.key && self.has_glyph(&face, ch) {
                    warnings.push(ExportWarning::GlyphFallback {
                        ch,
                        family: base.family.clone(),
                    });
                    return face;
                }
            }
        }
        warnings.push(ExportWarning::GlyphFallback {
            ch,
            family: base.family.clone(),
        });
        base.clone()
    }

    /// Whether `face` has a real glyph (not `.notdef`) for `ch`. Memoized.
    #[must_use]
    pub fn has_glyph(&self, face: &ResolvedFace, ch: char) -> bool {
        let cache_key = (face.key.0, ch);
        if let Some(&covered) = self.coverage.borrow().get(&cache_key) {
            return covered;
        }
        let covered = self
            .bytes_for(face.key.0)
            .and_then(|bytes| {
                let parsed = ttf_parser::Face::parse(&bytes, face.index).ok()?;
                Some(parsed.glyph_index(ch).is_some())
            })
            .unwrap_or(false);
        self.coverage.borrow_mut().insert(cache_key, covered);
        covered
    }

    /// The face's full font-program bytes (for a collection: the **whole
    /// TTC**, to be parsed at [`ResolvedFace::index`] — TS-3
    /// `parse_indexed`). Copied once per face, then served from cache.
    /// `None` only for a stale key (never for keys minted by this resolver).
    #[must_use]
    pub fn face_data(&self, face: &ResolvedFace) -> Option<Arc<Vec<u8>>> {
        self.bytes_for(face.key.0)
    }

    /// Cached owned bytes of one face's source program.
    fn bytes_for(&self, id: fontdb::ID) -> Option<Arc<Vec<u8>>> {
        if let Some(bytes) = self.bytes.borrow().get(&id) {
            return Some(bytes.clone());
        }
        let bytes = self
            .db
            .with_face_data(id, |data, _| Arc::new(data.to_vec()))?;
        self.bytes.borrow_mut().insert(id, bytes.clone());
        Some(bytes)
    }

    /// Looks one folded family name up in the index and picks the nearest
    /// weight/style face deterministically.
    fn query_family(&self, folded: &str, bold: bool, italic: bool) -> Option<ResolvedFace> {
        let ids = self.index.get(folded)?;
        let id = pick(&self.db, ids, bold, italic)?;
        self.face_of(id, bold, italic)
    }

    /// The [`ResolvedFace`] view of one face ID.
    fn face_of(&self, id: fontdb::ID, bold: bool, italic: bool) -> Option<ResolvedFace> {
        let info = self.db.face(id)?;
        Some(ResolvedFace {
            key: FaceKey(id),
            family: info
                .families
                .first()
                .map_or_else(|| info.post_script_name.clone(), |(name, _)| name.clone()),
            post_script_name: info.post_script_name.clone(),
            index: info.index,
            bold,
            italic,
        })
    }

    /// The unconditional terminal fallback (bundled Liberation Sans Regular),
    /// carrying the requested style flags for downstream per-char picks.
    fn terminal_fallback(&self, bold: bool, italic: bool) -> ResolvedFace {
        self.face_of(self.fallback_regular, bold, italic)
            .expect("pinned bundled fallback face must stay enumerated")
    }
}

/// Loads the bundled final-fallback faces: the 12 Liberation text faces plus
/// the 3 Noto symbol faces (zero-copy `&'static` sources).
fn load_bundled(db: &mut Database) {
    for family in [
        LiberationFamily::Sans,
        LiberationFamily::Serif,
        LiberationFamily::Mono,
    ] {
        for bold in [false, true] {
            for italic in [false, true] {
                load_static(db, liberation_face(family, bold, italic));
            }
        }
    }
    // symbol_faces() = [Noto Sans Math, Noto Sans Symbols 2];
    // zapf_faces() = [Noto Sans Symbols 2, Noto Sans Symbols] — dedup Symbols 2.
    load_static(db, symbol_faces()[0]);
    load_static(db, zapf_faces()[0]);
    load_static(db, zapf_faces()[1]);
}

/// Loads one `&'static` font program without copying it.
fn load_static(db: &mut Database, bytes: &'static [u8]) {
    db.load_font_source(Source::Binary(Arc::new(bytes)));
}

/// Builds the folded family-name index over every localized name record.
fn build_index(db: &Database) -> HashMap<String, Vec<fontdb::ID>> {
    let mut index: HashMap<String, Vec<fontdb::ID>> = HashMap::new();
    for face in db.faces() {
        for (name, _lang) in &face.families {
            let ids = index.entry(fold(name)).or_default();
            if !ids.contains(&face.id) {
                ids.push(face.id);
            }
        }
    }
    index
}

/// Picks the best face for a bold/italic request among `ids`,
/// deterministically: style rank (exact style, then oblique-as-italic, then
/// mismatch), nearest weight, then PostScript name / TTC index tie-breaks.
fn pick(db: &Database, ids: &[fontdb::ID], bold: bool, italic: bool) -> Option<fontdb::ID> {
    let target_weight: u16 = if bold { 700 } else { 400 };
    let mut best: Option<((u8, u16, String, u32), fontdb::ID)> = None;
    for &id in ids {
        let Some(info) = db.face(id) else { continue };
        let style_rank: u8 = match (italic, info.style) {
            (true, Style::Italic) | (false, Style::Normal) => 0,
            (_, Style::Oblique) => 1,
            (true, Style::Normal) | (false, Style::Italic) => 2,
        };
        let key = (
            style_rank,
            info.weight.0.abs_diff(target_weight),
            info.post_script_name.clone(),
            info.index,
        );
        if best.as_ref().is_none_or(|(bk, _)| key < *bk) {
            best = Some((key, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Case/width-folds a family name: NFKC (full-width → ASCII, ideographic
/// space → space), lowercase, whitespace runs collapsed to single spaces.
fn fold(name: &str) -> String {
    let normalized: String = name.nfkc().collect();
    let lower = normalized.to_lowercase();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_normalizes_case_width_and_whitespace() {
        assert_eq!(fold("Liberation Sans"), "liberation sans");
        assert_eq!(fold("LIBERATION  SANS"), "liberation sans");
        assert_eq!(fold("  Liberation \t Sans "), "liberation sans");
        // Full-width Latin + ideographic space fold to ASCII.
        assert_eq!(
            fold("Ｌｉｂｅｒａｔｉｏｎ\u{3000}Ｓａｎｓ"),
            "liberation sans"
        );
        // CJK names pass through (lowercase is a no-op).
        assert_eq!(fold("宋体"), "宋体");
    }

    #[test]
    fn builtin_tables_cover_the_locked_prd_rows() {
        for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            let table = Substitutions::builtin(platform);
            for requested in ["宋体", "SimSun", "微软雅黑", "Microsoft YaHei"] {
                assert!(
                    !table.candidates(&fold(requested)).is_empty(),
                    "{platform:?} missing {requested}"
                );
            }
            assert_eq!(table.candidates(&fold("Calibri")), ["Carlito"]);
            assert_eq!(
                table.candidates(&fold("Times New Roman")),
                ["Liberation Serif"]
            );
        }
        // Platform primaries per the locked PRD table.
        let mac = Substitutions::builtin(Platform::MacOs);
        assert_eq!(mac.candidates(&fold("宋体"))[0], "Songti SC");
        assert_eq!(mac.candidates(&fold("微软雅黑"))[0], "PingFang SC");
        let win = Substitutions::builtin(Platform::Windows);
        assert_eq!(win.candidates(&fold("宋体"))[0], "SimSun");
        assert_eq!(win.candidates(&fold("微软雅黑"))[0], "Microsoft YaHei");
        let linux = Substitutions::builtin(Platform::Linux);
        assert_eq!(linux.candidates(&fold("宋体"))[0], "Noto Serif CJK SC");
        assert_eq!(linux.candidates(&fold("微软雅黑"))[0], "Noto Sans CJK SC");
    }
}
