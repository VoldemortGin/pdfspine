//! A minimal RGB color for content insertion (PRD §8.8). PyMuPDF colors are
//! `(r, g, b)` floats in `0.0..=1.0`; we emit them as `rg`/`RG` operators.

/// An RGB color in the `0.0..=1.0` range per component.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Color {
    /// Red component, clamped to `0.0..=1.0` on use.
    pub r: f64,
    /// Green component.
    pub g: f64,
    /// Blue component.
    pub b: f64,
}

impl Color {
    /// Black `(0, 0, 0)`.
    pub const BLACK: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };

    /// Builds a color from components (not clamped here; clamped at emit time).
    #[must_use]
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Color { r, g, b }
    }

    /// Builds a color from a packed `0xRRGGBB` sRGB integer.
    #[must_use]
    pub fn from_rgb(rgb: u32) -> Self {
        Color {
            r: f64::from((rgb >> 16) & 0xff) / 255.0,
            g: f64::from((rgb >> 8) & 0xff) / 255.0,
            b: f64::from(rgb & 0xff) / 255.0,
        }
    }

    /// The `r g b rg` (fill) operator text for this color.
    #[must_use]
    pub fn fill_op(&self) -> String {
        format!("{} {} {} rg", c(self.r), c(self.g), c(self.b))
    }

    /// The `r g b RG` (stroke) operator text for this color.
    #[must_use]
    pub fn stroke_op(&self) -> String {
        format!("{} {} {} RG", c(self.r), c(self.g), c(self.b))
    }
}

/// Formats one clamped color component for a content operator.
fn c(v: f64) -> String {
    crate::content::fmt_num(v.clamp(0.0, 1.0))
}
