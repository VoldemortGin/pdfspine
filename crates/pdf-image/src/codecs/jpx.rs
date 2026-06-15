//! JPXDecode (JPEG 2000) image-XObject decode — PRD §8.4.1 (documented subset).
//!
//! Subset target: baseline JP2/J2K with common colorspaces (gray, sRGB, YCC) at
//! 8/16-bit; exotic component transforms / extended capabilities are best-effort.
//! Decoder `hayro-jpeg2000` (untrusted, resource-capped). The optional OpenJPEG
//! (C) fallback is deliberately NOT wired (PRD §8.4.1: it breaks the pure-Rust /
//! no-unsafe / WASM-clean guarantees). Per the §8.4.1 degradation contract, a
//! decode failure returns a typed error for *this image only*.
//! Implemented in the M5-codecs unit; this is the compiling stub.

use crate::codecs::DecodedImage;
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore};

/// Decodes a JPXDecode (JPEG 2000) image stream to a [`DecodedImage`].
///
/// `data` is the JP2/J2K codestream; `params` is the image stream dict (for JPX,
/// `/ColorSpace` may be omitted and taken from the codestream); `doc` resolves
/// indirect references in `params`.
///
/// Stub: returns [`Error::Unsupported`] (`"JPXDecode"`) — panic-free; this is
/// also the documented-gap return until the M5 subset lands.
pub fn decode(_doc: &DocumentStore, _data: &[u8], _params: &Dict) -> Result<DecodedImage> {
    Err(Error::Unsupported("JPXDecode"))
}
