//! Font selection, per-char fallback and text measurement for the layouter.
//!
//! Three font sources cooperate (PRD §9, CJK Option A):
//!
//! - **Base-14** faces (Helvetica family for body/headings, Courier for code):
//!   zero embedding, WinAnsi single-byte encoding, advances from
//!   [`pdf_fonts::std_widths`].
//! - **`Options::font`** — an optional user TTF that replaces the Base-14 body /
//!   heading faces (embedded once as Type0/Identity-H via
//!   [`pdf_edit::EmbeddedFont`]).
//! - **`Options::cjk_font`** — an optional *per-character fallback* TTF: any
//!   character the active face cannot encode (outside WinAnsi for Base-14, no
//!   `cmap` entry for the user TTF) switches to this face for that character.
//!
//! Without a fallback, unencodable characters degrade to `?` (Base-14) or
//! `.notdef` (user TTF) — never a panic. Measurement and drawing use the same
//! resolution, so line breaks always match the drawn glyphs.

use std::cell::RefCell;
use std::collections::BTreeMap;

use pdf_core::error::Result;
use pdf_edit::EmbeddedFont;
use pdf_fonts::std_widths;

use crate::model::Style;
use crate::Options;

/// One drawable face. The five Base-14 variants carry their canonical standard
/// name; `User` / `Cjk` refer to the optional embedded fonts in [`FontSet`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Face {
    /// Helvetica (body).
    Helv,
    /// Helvetica-Bold (headings, `**bold**`).
    HelvBold,
    /// Helvetica-Oblique (`*italic*`).
    HelvOblique,
    /// Helvetica-BoldOblique (bold + italic).
    HelvBoldOblique,
    /// Courier (inline code + code blocks).
    Courier,
    /// The user TTF from `Options::font`.
    User,
    /// The fallback TTF from `Options::cjk_font`.
    Cjk,
}

impl Face {
    /// Every face, in the fixed document-assembly order.
    pub(crate) const ALL: [Face; 7] = [
        Face::Helv,
        Face::HelvBold,
        Face::HelvOblique,
        Face::HelvBoldOblique,
        Face::Courier,
        Face::User,
        Face::Cjk,
    ];

    /// Stable index into per-face bookkeeping arrays.
    pub(crate) fn index(self) -> usize {
        match self {
            Face::Helv => 0,
            Face::HelvBold => 1,
            Face::HelvOblique => 2,
            Face::HelvBoldOblique => 3,
            Face::Courier => 4,
            Face::User => 5,
            Face::Cjk => 6,
        }
    }

    /// The fixed `/Resources /Font` name for this face.
    pub(crate) fn res_name(self) -> &'static str {
        ["F0", "F1", "F2", "F3", "F4", "F5", "F6"][self.index()]
    }

    /// The canonical Base-14 standard-font name, or `None` for embedded faces.
    pub(crate) fn std_name(self) -> Option<&'static str> {
        match self {
            Face::Helv => Some("Helvetica"),
            Face::HelvBold => Some("Helvetica-Bold"),
            Face::HelvOblique => Some("Helvetica-Oblique"),
            Face::HelvBoldOblique => Some("Helvetica-BoldOblique"),
            Face::Courier => Some("Courier"),
            Face::User | Face::Cjk => None,
        }
    }

    /// Whether this face is one of the embedded (Type0) fonts.
    pub(crate) fn is_embedded(self) -> bool {
        matches!(self, Face::User | Face::Cjk)
    }
}

/// An embedded font plus a per-char glyph-ID cache (avoids re-parsing the font
/// program for every character during measurement and emission).
pub(crate) struct EmbFace {
    /// The parsed font program (parsed **once** per document).
    pub(crate) font: EmbeddedFont,
    gids: RefCell<BTreeMap<char, u16>>,
}

impl EmbFace {
    fn new(program: &[u8]) -> Result<Self> {
        Ok(EmbFace {
            font: EmbeddedFont::parse(program)?,
            gids: RefCell::new(BTreeMap::new()),
        })
    }

    /// The glyph ID for `ch` (0 = `.notdef`), memoized.
    pub(crate) fn gid(&self, ch: char) -> u16 {
        if let Some(&g) = self.gids.borrow().get(&ch) {
            return g;
        }
        let g = self.font.glyph_id(ch);
        self.gids.borrow_mut().insert(ch, g);
        g
    }

    /// The advance of `ch` at `size` points.
    fn advance(&self, ch: char, size: f64) -> f64 {
        self.font.advance(self.gid(ch)) * size / 1000.0
    }
}

/// The document's font environment: the two optional embedded fonts plus the
/// resolution / measurement entry points used by layout and rendering.
pub(crate) struct FontSet {
    pub(crate) user: Option<EmbFace>,
    pub(crate) cjk: Option<EmbFace>,
}

impl FontSet {
    /// Parses the optional user / fallback TTFs from `opts`.
    ///
    /// # Errors
    ///
    /// Propagates the typed [`EmbeddedFont::parse`] error for an unparseable
    /// font program (never panics).
    pub(crate) fn new(opts: &Options) -> Result<Self> {
        let user = opts.font.as_deref().map(EmbFace::new).transpose()?;
        let cjk = opts.cjk_font.as_deref().map(EmbFace::new).transpose()?;
        Ok(FontSet { user, cjk })
    }

    /// The *preferred* face for an inline style: code → Courier; otherwise the
    /// user TTF when present, else the Helvetica variant for bold/italic.
    pub(crate) fn face_for(&self, style: Style) -> Face {
        if style.code {
            return Face::Courier;
        }
        if self.user.is_some() {
            return Face::User;
        }
        match (style.bold, style.italic) {
            (false, false) => Face::Helv,
            (true, false) => Face::HelvBold,
            (false, true) => Face::HelvOblique,
            (true, true) => Face::HelvBoldOblique,
        }
    }

    /// Resolves one character against the preferred face, applying the per-char
    /// fallback: returns the face to draw with and the *effective* character
    /// (identical to `ch` unless a Base-14 face degrades it to `?`).
    pub(crate) fn resolve(&self, pref: Face, ch: char) -> (Face, char) {
        match pref {
            Face::User | Face::Cjk => {
                let own = if pref == Face::User {
                    self.user.as_ref()
                } else {
                    self.cjk.as_ref()
                };
                let Some(own) = own else {
                    // Defensive: a missing embedded face degrades to Helvetica.
                    return self.resolve(Face::Helv, ch);
                };
                if own.gid(ch) != 0 {
                    return (pref, ch);
                }
                if pref == Face::User {
                    if let Some(cjk) = &self.cjk {
                        if cjk.gid(ch) != 0 {
                            return (Face::Cjk, ch);
                        }
                    }
                }
                (pref, ch) // .notdef — degrade, never panic
            }
            _ => {
                if winansi_byte(ch).is_some() {
                    return (pref, ch);
                }
                if let Some(cjk) = &self.cjk {
                    if cjk.gid(ch) != 0 {
                        return (Face::Cjk, ch);
                    }
                }
                (pref, '?')
            }
        }
    }

    /// The advance of `ch` on `face` at `size` points. `ch` must already be the
    /// effective character from [`FontSet::resolve`].
    pub(crate) fn advance(&self, face: Face, ch: char, size: f64) -> f64 {
        match face {
            Face::User => self.user.as_ref().map_or(0.0, |f| f.advance(ch, size)),
            Face::Cjk => self.cjk.as_ref().map_or(0.0, |f| f.advance(ch, size)),
            _ => {
                let name = face.std_name().unwrap_or("Helvetica");
                std_widths::string_advance(name, ch.encode_utf8(&mut [0u8; 4]), size)
            }
        }
    }
}

/// The WinAnsi byte for `ch` under the same simple mapping `pdf-edit` uses for
/// Base-14 text: printable ASCII and the Latin-1 supplement map directly; other
/// characters are not encodable.
pub(crate) fn winansi_byte(ch: char) -> Option<u8> {
    let cp = ch as u32;
    if (0x20..=0x7e).contains(&cp) || (0xa0..=0xff).contains(&cp) {
        Some(cp as u8)
    } else {
        None
    }
}
