//! JBIG2Decode image-XObject decode — PRD §8.4.1 (documented subset).
//!
//! Subset target: generic region + symbol dictionary + text region (the bulk of
//! scanned PDFs); halftone/refinement regions are best-effort. Decoder
//! `hayro-jbig2` (untrusted, resource-capped). Per the §8.4.1 degradation
//! contract, an unsupported region / decode failure returns a typed error for
//! *this image only* — it never panics and never aborts text extraction.
//! Implemented in the M5-codecs unit; this is the compiling stub.

use crate::codecs::DecodedImage;
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore};

/// Decodes a JBIG2Decode image stream to a 1-bpc [`DecodedImage`].
///
/// `data` is the JBIG2 embedded-stream payload; `params` is the image stream
/// dict (its `/DecodeParms` may carry an indirect `/JBIG2Globals` stream, which
/// `doc` resolves).
///
/// Stub: returns [`Error::Unsupported`] (`"JBIG2Decode"`) — panic-free; this is
/// also the documented-gap return until the M5 subset lands.
pub fn decode(_doc: &DocumentStore, _data: &[u8], _params: &Dict) -> Result<DecodedImage> {
    Err(Error::Unsupported("JBIG2Decode"))
}
