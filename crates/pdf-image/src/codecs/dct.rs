//! DCTDecode (JPEG) image-XObject decode — PRD §8.4 / §11.1.
//!
//! Baseline + progressive JPEG; YCbCr/CMYK incl. inverted Adobe APP14. Primary
//! decoder `zune-jpeg`, cross-checked against `jpeg-decoder`. Implemented in the
//! M5-codecs unit; this is the compiling stub.

use crate::codecs::DecodedImage;
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore};

/// Decodes a DCTDecode (JPEG) image stream to a [`DecodedImage`].
///
/// `data` is the raw JFIF/JPEG byte stream; `params` is the image stream dict
/// (`/Width /Height /ColorSpace /Decode /DecodeParms`); `doc` resolves indirect
/// references in `params`.
///
/// Stub: returns [`Error::Unsupported`] (`"DCTDecode"`) — panic-free.
pub fn decode(_doc: &DocumentStore, _data: &[u8], _params: &Dict) -> Result<DecodedImage> {
    Err(Error::Unsupported("DCTDecode"))
}
