//! CCITTFaxDecode (Group 3 / Group 4 fax) image-XObject decode — PRD §8.4.
//!
//! Honors `/K /Columns(1728) /Rows /BlackIs1 /EncodedByteAlign` from
//! `/DecodeParms`. Primary decoder `fax` (alt: `hayro-ccitt`). Output is 1 bpc,
//! 1 component. Implemented in the M5-codecs unit; this is the compiling stub.

use crate::codecs::DecodedImage;
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore};

/// Decodes a CCITTFaxDecode image stream to a 1-bpc [`DecodedImage`].
///
/// `data` is the encoded fax byte stream; `params` is the image stream dict
/// (its `/DecodeParms` carries `/K /Columns /Rows /BlackIs1 /EncodedByteAlign`);
/// `doc` resolves indirect references in `params`.
///
/// Stub: returns [`Error::Unsupported`] (`"CCITTFaxDecode"`) — panic-free.
pub fn decode(_doc: &DocumentStore, _data: &[u8], _params: &Dict) -> Result<DecodedImage> {
    Err(Error::Unsupported("CCITTFaxDecode"))
}
