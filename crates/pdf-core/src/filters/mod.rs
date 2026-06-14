//! Stream filters / codecs — ISO 32000-1 §7.4, PRD §8.3.
//!
//! A PDF stream's bytes are encoded through the filter chain named in its
//! `/Filter` entry (a single name or an array applied left-to-right), each with
//! optional parameters in the parallel `/DecodeParms`. This module implements
//! the decode (and, where useful, encode) side of every M1 filter plus the
//! PNG/TIFF predictors, and a [`decode_stream`] dispatcher that walks the chain.
//!
//! # Totality (PRD §8.1 / §9.6)
//!
//! Every codec is **total**: arbitrary, truncated or corrupt input yields a
//! typed [`crate::Error`] (`Decode` / `Filter` / `LimitExceeded`), never a
//! panic. Every decoder threads a [`Limits`] and refuses to allocate past
//! [`Limits::max_decompressed_stream`] — the decompression-bomb guard.
//!
//! # Policies (PRD §8.3)
//!
//! - **Trailing garbage** (Flate): decode the valid prefix and stop at the end
//!   of the deflate stream; bytes after it are ignored (real-world PDFs pad
//!   streams). See [`flate`].
//! - **Missing zlib header** (Flate): if the zlib (RFC 1950) wrapper is absent
//!   or its header is invalid, retry as raw deflate (RFC 1951). See [`flate`].
//! - **Image-only filters** (`DCTDecode` / `JPXDecode` / `CCITTFaxDecode` /
//!   `JBIG2Decode`): not an error and not a panic — the chain stops and the
//!   still-encoded bytes are returned as [`DecodeOutcome::ImageEncoded`],
//!   tagged with the filter name. These are decoded in M5.

pub mod ascii85;
pub mod ascii_hex;
pub mod flate;
pub mod lzw;
pub mod predictor;
pub mod run_length;

use crate::error::{Error, Result};
use crate::limits::Limits;
use crate::object::{Dict, Name, Object};

use predictor::PredictorParams;

/// The result of [`decode_stream`].
///
/// A fully-decodable chain yields [`DecodeOutcome::Decoded`]. A chain that ends
/// at an image-only filter yields [`DecodeOutcome::ImageEncoded`] carrying the
/// bytes still encoded by that filter plus its name — this is the "leave
/// encoded" policy (PRD §8.3), not an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeOutcome {
    /// The full filter chain was applied; these are the decoded bytes.
    Decoded(Vec<u8>),
    /// The chain stopped at an image-only filter (decoded in M5). `bytes` are
    /// still encoded by `filter` (and any later filters in the chain).
    ImageEncoded {
        /// The image filter that halted decoding (canonical name, e.g.
        /// `"DCTDecode"`).
        filter: &'static str,
        /// The bytes still encoded by `filter` onward.
        bytes: Vec<u8>,
    },
}

impl DecodeOutcome {
    /// The decoded bytes, or an [`Error::Unsupported`] if the chain stopped at
    /// an image filter. Convenience for callers that only handle text/data
    /// streams (xref / object streams / content streams).
    pub fn into_decoded(self) -> Result<Vec<u8>> {
        match self {
            DecodeOutcome::Decoded(b) => Ok(b),
            DecodeOutcome::ImageEncoded { filter, .. } => Err(Error::Unsupported(filter)),
        }
    }
}

/// One filter step resolved from `/Filter` + `/DecodeParms`.
struct Step {
    kind: FilterKind,
    parms: Option<Dict>,
}

/// The filters we recognize by canonical name or abbreviation (PRD §8.3).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum FilterKind {
    Flate,
    Lzw,
    AsciiHex,
    Ascii85,
    RunLength,
    /// An image-only filter handled by the "leave encoded" policy. The field is
    /// the canonical name reported in [`DecodeOutcome::ImageEncoded`].
    Image(&'static str),
}

impl FilterKind {
    /// Maps a `/Filter` name (canonical or the inline-image abbreviation) to a
    /// [`FilterKind`], or `None` for an unrecognized name.
    fn from_name(name: &Name) -> Option<FilterKind> {
        let s = name.as_str()?;
        Some(match s {
            "FlateDecode" | "Fl" => FilterKind::Flate,
            "LZWDecode" | "LZW" => FilterKind::Lzw,
            "ASCIIHexDecode" | "AHx" => FilterKind::AsciiHex,
            "ASCII85Decode" | "A85" => FilterKind::Ascii85,
            "RunLengthDecode" | "RL" => FilterKind::RunLength,
            "DCTDecode" | "DCT" => FilterKind::Image("DCTDecode"),
            "JPXDecode" => FilterKind::Image("JPXDecode"),
            "CCITTFaxDecode" | "CCF" => FilterKind::Image("CCITTFaxDecode"),
            "JBIG2Decode" => FilterKind::Image("JBIG2Decode"),
            _ => return None,
        })
    }

    /// The canonical name, for error messages.
    fn name(self) -> &'static str {
        match self {
            FilterKind::Flate => "FlateDecode",
            FilterKind::Lzw => "LZWDecode",
            FilterKind::AsciiHex => "ASCIIHexDecode",
            FilterKind::Ascii85 => "ASCII85Decode",
            FilterKind::RunLength => "RunLengthDecode",
            FilterKind::Image(n) => n,
        }
    }
}

/// Reads `/Filter` (single [`Name`] or array of names) and the parallel
/// `/DecodeParms` (single dict, array of dicts/nulls, or absent), and applies
/// the filter chain in order to `raw`, applying any predictor named in a
/// filter's parms (PRD §8.3, ISO 32000-1 §7.4.1).
///
/// Returns [`DecodeOutcome::Decoded`] for a fully-decodable chain, or
/// [`DecodeOutcome::ImageEncoded`] when the chain reaches an image-only filter
/// (the "leave encoded" policy — **not** an error). Returns an `Err` only for a
/// genuinely malformed chain (unknown filter, bad parms, decode failure, limit
/// exceeded).
///
/// `limits.max_decompressed_stream` bounds every decode step.
pub fn decode_stream(dict: &Dict, raw: &[u8], limits: &Limits) -> Result<DecodeOutcome> {
    let steps = resolve_steps(dict)?;

    let mut data = raw.to_vec();
    for step in &steps {
        if let FilterKind::Image(name) = step.kind {
            // Leave-encoded policy: stop here, hand back the still-encoded bytes.
            return Ok(DecodeOutcome::ImageEncoded {
                filter: name,
                bytes: data,
            });
        }

        let decoded = match step.kind {
            FilterKind::Flate => flate::decode(&data, limits)?,
            FilterKind::Lzw => lzw::decode(&data, early_change(step.parms.as_ref()), limits)?,
            FilterKind::AsciiHex => ascii_hex::decode(&data, limits)?,
            FilterKind::Ascii85 => ascii85::decode(&data, limits)?,
            FilterKind::RunLength => run_length::decode(&data, limits)?,
            FilterKind::Image(_) => unreachable!("handled above"),
        };

        // Apply this filter's predictor (Flate/LZW only carry one; others have
        // no /Predictor so this is a no-op).
        data = match PredictorParams::from_parms(step.parms.as_ref(), step.kind.name())? {
            Some(params) => predictor::unpredict(&decoded, &params, limits)?,
            None => decoded,
        };
    }

    Ok(DecodeOutcome::Decoded(data))
}

/// Reads the PDF `/EarlyChange` LZW parameter (default 1) from a parms dict.
fn early_change(parms: Option<&Dict>) -> bool {
    parms
        .and_then(|d| d.get(&Name::new("EarlyChange")))
        .and_then(Object::as_i64)
        .map(|v| v != 0)
        .unwrap_or(true)
}

/// Pairs each `/Filter` entry with its `/DecodeParms` entry (handling the
/// single-vs-array and null cases), validating filter names.
fn resolve_steps(dict: &Dict) -> Result<Vec<Step>> {
    let filters = filter_names(dict)?;
    let parms = decode_parms(dict, filters.len())?;

    let mut steps = Vec::with_capacity(filters.len());
    for (name, p) in filters.into_iter().zip(parms) {
        let kind =
            FilterKind::from_name(&name).ok_or(Error::filter("stream", "unknown /Filter name"))?;
        steps.push(Step { kind, parms: p });
    }
    Ok(steps)
}

/// Collects the `/Filter` entry as an ordered list of names (single name or
/// array). Absent `/Filter` → empty list. References are not resolved here (the
/// dispatcher operates on an already-resolved dict).
fn filter_names(dict: &Dict) -> Result<Vec<Name>> {
    match dict.get(&Name::new("Filter")) {
        None | Some(Object::Null) => Ok(Vec::new()),
        Some(Object::Name(n)) => Ok(vec![n.clone()]),
        Some(Object::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for o in a {
                match o {
                    Object::Name(n) => out.push(n.clone()),
                    _ => return Err(Error::filter("stream", "/Filter array entry not a name")),
                }
            }
            Ok(out)
        }
        Some(_) => Err(Error::filter("stream", "/Filter is not a name or array")),
    }
}

/// Collects `/DecodeParms` (also accepting the `/DP` inline abbreviation),
/// aligned to `n` filters: a single dict applies to a single filter; an array
/// pairs positionally; `null` / missing entries become `None`.
fn decode_parms(dict: &Dict, n: usize) -> Result<Vec<Option<Dict>>> {
    let parms = dict
        .get(&Name::new("DecodeParms"))
        .or_else(|| dict.get(&Name::new("DP")));

    match parms {
        None | Some(Object::Null) => Ok(vec![None; n]),
        Some(Object::Dictionary(d)) => {
            // A lone dict is valid only for a single-filter chain; for safety we
            // apply it to the first filter and leave the rest unparametrized.
            let mut v = vec![None; n];
            if n >= 1 {
                v[0] = Some(d.clone());
            }
            Ok(v)
        }
        Some(Object::Array(a)) => {
            let mut v = Vec::with_capacity(n);
            for o in a.iter() {
                match o {
                    Object::Null => v.push(None),
                    Object::Dictionary(d) => v.push(Some(d.clone())),
                    _ => return Err(Error::filter("stream", "/DecodeParms entry not dict/null")),
                }
            }
            // Pad/truncate to n so the positional zip is well-defined.
            v.resize(n, None);
            Ok(v)
        }
        Some(_) => Err(Error::filter("stream", "/DecodeParms not dict/array/null")),
    }
}
