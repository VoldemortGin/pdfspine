//! Graphics + text state for the content interpreter (M2b, ISO §8.4 / §9.3).
//!
//! The `q`/`Q` stack saves and restores the *graphics* state (CTM + color +
//! text-state parameters). The text matrices `Tm`/`Tlm` live in the text object
//! and are **not** part of the saved graphics state (ISO §9.4.1) — they are
//! initialized to identity at every `BT`.

use pdf_core::geom::Matrix;

/// The text-state parameters (ISO §9.3) that persist across `BT`/`ET` and are
/// part of the saved graphics state.
#[derive(Clone, Debug, PartialEq)]
pub struct TextState {
    /// Character spacing `Tc` (unscaled text-space units).
    pub char_spacing: f64,
    /// Word spacing `Tw` (unscaled text-space units; applies to code 0x20).
    pub word_spacing: f64,
    /// Horizontal scaling `Th` as a fraction (`Tz`/100); default 1.0.
    pub h_scale: f64,
    /// Leading `TL` (unscaled text-space units).
    pub leading: f64,
    /// Font size `Tfs`.
    pub font_size: f64,
    /// Text rise `Ts` (unscaled text-space units).
    pub rise: f64,
    /// Render mode `Tr` (0..=7).
    pub render_mode: u8,
    /// The current font resource name (e.g. `F1`), set by `Tf`.
    pub font_name: Option<smol_str::SmolStr>,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            font_size: 0.0,
            rise: 0.0,
            render_mode: 0,
            font_name: None,
        }
    }
}

/// The full graphics state pushed/popped by `q`/`Q` (the subset M2b needs).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsState {
    /// The current transformation matrix (device-independent; user→page).
    pub ctm: Matrix,
    /// The current fill color packed as `0x00RRGGBB`.
    pub fill_color: u32,
    /// The current stroke color packed as `0x00RRGGBB`.
    pub stroke_color: u32,
    /// The current line width `w` (user-space units; default 1.0).
    pub line_width: f64,
    /// The current dash-pattern string (`"[…] phase"`), empty when solid.
    pub dashes: String,
    /// The text-state parameters.
    pub text: TextState,
}

impl GraphicsState {
    /// A fresh graphics state with the given initial CTM (the page/form base
    /// transform). Fill/stroke default to black; text state to its defaults.
    #[must_use]
    pub fn new(ctm: Matrix) -> Self {
        GraphicsState {
            ctm,
            fill_color: 0x00_00_00_00,
            stroke_color: 0x00_00_00_00,
            line_width: 1.0,
            dashes: String::new(),
            text: TextState::default(),
        }
    }
}
