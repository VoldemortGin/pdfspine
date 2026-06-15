# `pdf-image` — M5 module architecture & parallel-implementation contract

This crate delivers M5's image path (PRD §8.10 / §8.4 / §8.4.1): image-document
support, image-XObject codecs, and the `Pixmap` decoded-raster type. First-party
code is `#![forbid(unsafe_code)]`; the codec dependencies are untrusted leaves
wrapped by the §8.4.1 degradation contract (every decode failure is a **typed
error for that image only**, never a panic, and never aborts text extraction).

This document is the **contract** for the four parallel M5 units. Each unit owns
**one** module and fills its stubs **without changing the public signatures
below**. Cross-module shapes (`error::Error`/`Result`, `codecs::DecodedImage`,
`codecs::ColorSpaceHint`, `pixmap::Pixmap`/`Colorspace`) are **frozen** — extend
them only additively (`#[non_exhaustive]` enums; new methods, not changed
required fields) and announce it if a parallel unit depends on the addition.

## Module ownership

| Module | File | Owner unit | Task |
|---|---|---|---|
| `error` | `src/error.rs` | shared (this scaffold) | typed errors — frozen |
| `codecs` (incl. `dct`/`ccitt`/`jbig2`/`jpx`) | `src/codecs/` | **M5-codecs** | DCT/CCITT/JBIG2/JPX decode |
| `imagedoc` | `src/imagedoc.rs` | **M5-imagedoc** | loader + `convert_to_pdf` |
| `pixmap` | `src/pixmap.rs` | **M5-pixmap** | `Pixmap` + save/buffer; also closes M5 (`get_pixmap`/`extract_image`/PyO3) |

`COMPAT.toml` / shim hardening (**M5-compat**) lives outside this crate (Python
shim + `compat-symbol-guard`); it does not edit files here.

> The scaffold stubs all return `Err(error::Error::Unsupported(...))` (or empty
> placeholders) — **never `todo!()`/`unimplemented!()`** — so the workspace
> builds and any smoke run is panic-free.

## Frozen cross-module types

### `error` (frozen)

```rust
pub enum Error {                       // #[non_exhaustive], thiserror
    Unsupported(&'static str),         // panic-free stub placeholder + documented gaps
    Decode { codec: &'static str, msg: &'static str },   // §8.4.1 "this image failed"
    InvalidArgument(&'static str),     // e.g. non-image input to convert_to_pdf
    LimitExceeded(&'static str),       // resource cap (decompression-bomb guard)
    Core(#[from] pdf_core::Error),     // propagated pdf-core failures
}
impl Error {
    pub fn decode(codec: &'static str, msg: &'static str) -> Self;
    pub fn kind(&self) -> &'static str;   // "unsupported"|"decode"|"invalid-argument"|"limit-exceeded"|"core"
}
pub type Result<T> = std::result::Result<T, Error>;
```

### `codecs` shared output (frozen)

```rust
pub enum ColorSpaceHint { Gray, Rgb, Cmyk, Unknown }   // #[non_exhaustive]

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub components: u8,      // 1=gray, 3=rgb, 4=cmyk
    pub bits: u8,            // 1,2,4,8,16
    pub colorspace: ColorSpaceHint,
    pub data: Vec<u8>,       // interleaved, row-major, no row padding; 16bpc => big-endian
}
impl DecodedImage {
    pub fn new(width: u32, height: u32, components: u8, bits: u8,
               colorspace: ColorSpaceHint, data: Vec<u8>) -> Self;
}
```

### `pixmap` (frozen)

```rust
pub enum Colorspace { Gray, Rgb, Cmyk }   // #[non_exhaustive]
impl Colorspace { pub fn components(self) -> u8; }

pub struct Pixmap {     // field layout per PRD §8.10
    pub width: u32, pub height: u32,
    pub n: u8,          // components incl. alpha
    pub alpha: bool,
    pub stride: usize,  // width * n (no padding in v1)
    pub samples: Vec<u8>,
    pub colorspace: Colorspace,
}
impl Pixmap {
    pub fn new(width: u32, height: u32, colorspace: Colorspace, alpha: bool,
               samples: Vec<u8>) -> Self;   // computes n + stride
    pub fn samples(&self) -> &[u8];
    pub fn save_png(&self, out: &mut Vec<u8>) -> Result<()>;
}
```

## Stub signatures to implement (do not change)

**`codecs/mod.rs`** — dispatcher (M5-codecs):

```rust
pub fn decode_image_xobject(
    doc: &DocumentStore, filter: &str, data: &[u8], params: &Dict,
) -> Result<DecodedImage>;
```
`filter` is the canonical PDF name from `pdf_core::DecodeOutcome::ImageEncoded`
(`"DCTDecode"`/`"CCITTFaxDecode"`/`"JBIG2Decode"`/`"JPXDecode"`; DCT/CCF
abbreviations accepted). `data` is the codec payload past any preceding
Flate/LZW/ASCII filters. `params` is the image stream dict; `doc` resolves
indirect refs in it (`/JBIG2Globals`, indexed `/ColorSpace`, …).

**`codecs/{dct,ccitt,jbig2,jpx}.rs`** — one per codec (M5-codecs):

```rust
pub fn decode(doc: &DocumentStore, data: &[u8], params: &Dict) -> Result<DecodedImage>;
```
- `dct`: `zune-jpeg` primary, `jpeg-decoder` cross-check; YCbCr/CMYK + Adobe APP14.
- `ccitt`: `fax` (alt `hayro-ccitt`); honor `/K /Columns /Rows /BlackIs1 /EncodedByteAlign`; output 1 bpc / 1 component.
- `jbig2`: `hayro-jbig2`; §8.4.1 subset (generic + symbol-dict + text region); unsupported region ⇒ `Error::Unsupported("JBIG2Decode")`.
- `jpx`: `hayro-jpeg2000`; §8.4.1 subset (baseline JP2/J2K, gray/sRGB/YCC, 8/16-bit); OpenJPEG-C fallback intentionally NOT wired.

**`imagedoc.rs`** (M5-imagedoc):

```rust
pub enum ImageFormat { Png, Jpeg, Tiff, Gif, Bmp, Webp }   // #[non_exhaustive]
impl ImageFormat { pub fn sniff(bytes: &[u8]) -> Option<ImageFormat>; }

pub struct ImageDocument { pub format: ImageFormat, pub pages: Vec<Pixmap> }
impl ImageDocument { pub fn page_count(&self) -> usize; }

pub fn open_image_document(bytes: &[u8], format: Option<ImageFormat>) -> Result<ImageDocument>;
pub fn convert_to_pdf(bytes: &[u8], format: Option<ImageFormat>) -> Result<Vec<u8>>;  // PDF bytes
```
`convert_to_pdf`: JPEG → `/DCTDecode` passthrough; PNG/TIFF → decode → Flate;
alpha → `/SMask`; palette → `/Indexed`; CMYK → `/DeviceCMYK` + Adobe `/Decode`;
16-bit → BPC 16; honor EXIF/TIFF orientation. **Non-image input ⇒
`Error::InvalidArgument`** (Python `PdfUnsupportedError`), never a panic.

> If a unit genuinely needs a frozen signature changed, change it in the
> scaffold first and re-publish this contract — do **not** diverge in a worktree,
> or the parallel merge breaks.

## Dependencies (verified on crates.io, all permissive)

All in `[workspace.dependencies]`, referenced by `crates/pdf-image/Cargo.toml`.
Licenses verified against `deny.toml` (no GPL/AGPL/LGPL/MPL/IJG in the graph):

| Crate | Version | License | Role |
|---|---|---|---|
| `image` | 0.25 (default-features off; png/jpeg/tiff/gif/bmp/webp) | MIT OR Apache-2.0 | image-document decode/encode hub |
| `zune-jpeg` | 0.5 | MIT/Apache/Zlib | DCTDecode primary |
| `jpeg-decoder` | 0.3 | MIT OR Apache-2.0 | DCTDecode cross-check |
| `fax` | 0.2 | MIT | CCITTFax decode/encode |
| `hayro-ccitt` | 0.3 | Apache-2.0 OR MIT | CCITTFax alt decoder |
| `hayro-jbig2` | 0.3 | Apache-2.0 OR MIT | JBIG2 subset (§8.4.1) |
| `hayro-jpeg2000` | 0.4 | Apache-2.0 OR MIT | JPX subset (§8.4.1) |
| `flate2`, `weezl` | reuse | MIT/Apache | Flate/LZW image streams |

**JBIG2/JPX existence:** `hayro-jbig2 0.3.0` and `hayro-jpeg2000 0.4.0` **both
exist on crates.io** (PRD-named versions confirmed) — these are **not** gaps.
The documented gap is **JPEG encode**: `jpeg-encoder 0.7.0` exists but is
`(MIT OR Apache-2.0) AND IJG`; `IJG` is not in the `deny.toml` allowlist, so it
is **not wired in the scaffold**. The M5-codecs/imagedoc owner who needs JPEG
re-encode (DCT save-transcode) must add `jpeg-encoder` **and** add an `IJG`
license exception to `deny.toml` (PRD §11.1 "reproduce IJG notice"), or transcode
to Flate instead. The optional OpenJPEG-C JPX fallback stays off (PRD §8.4.1).
Note: `image`'s `gif` feature pulls `weezl 0.1` transitively alongside the
workspace `weezl 0.2` — a `multiple-versions = "warn"` (not deny) condition.
