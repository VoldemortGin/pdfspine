#![forbid(unsafe_code)]
//! `pdf-image` — image-document support, image-XObject decode/encode, `Pixmap`.
//!
//! First-party code here is `#![forbid(unsafe_code)]`; the codec dependencies
//! (`zune-jpeg`, `image`, `hayro-*`, `fax`, …) may contain `unsafe` internally —
//! they are untrusted leaves, resource-capped and wrapped by the §8.4.1
//! degradation contract (a decode failure is a typed [`error::Error`], never a
//! panic, and never aborts the document — text extraction continues).
//!
//! # M5 layout (PRD §8.10 / §8.4)
//!
//! - [`error`] — the crate [`error::Error`]/[`error::Result`] (typed, panic-free).
//! - [`codecs`] — image-XObject decode: [`codecs::dct`] (DCTDecode/JPEG),
//!   [`codecs::ccitt`] (CCITTFaxDecode G3/G4), [`codecs::jbig2`] (JBIG2, subset
//!   §8.4.1), [`codecs::jpx`] (JPXDecode/JPEG2000, subset §8.4.1); plus the
//!   shared [`codecs::DecodedImage`] output and the
//!   [`codecs::decode_image_xobject`] dispatcher.
//! - [`imagedoc`] — image-document loader ([`imagedoc::open_image_document`])
//!   and image-input → PDF ([`imagedoc::convert_to_pdf`]).
//! - [`pixmap`] — the [`pixmap::Pixmap`] decoded-raster type (for image docs and
//!   image-only PDF pages, PRD §3.3).
//!
//! This is the **M5 scaffold**: every public item below is a compiling stub that
//! returns [`error::Error::Unsupported`] (or an empty placeholder) so the
//! workspace builds and stays panic-free. The four parallel M5 units each own
//! exactly one module and fill the stubs **without changing the signatures**
//! documented here and in `ARCHITECTURE.md`.

pub mod codecs;
pub mod error;
pub mod imagedoc;
pub mod pixmap;

pub use error::{Error, Result};
