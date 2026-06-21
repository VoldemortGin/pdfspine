//! Shared colorspace + PDF-function abstraction (PRD §8.6 / §8.10 / P3-3).
//!
//! This module is the **single** colorspace/function evaluator used by both the
//! image-decode path ([`crate`] consumers in `pdf-image`/`pdf-render`) and the
//! content-stream color path (`pdf-text`'s `cs`/`scn`). It lives in `pdf-core`
//! (the DAG root) so neither downstream crate has to reimplement function
//! evaluation or palette/tint resolution.
//!
//! Scope (P3-3): `DeviceGray`/`DeviceRGB`/`DeviceCMYK`, `CalGray`/`CalRGB`/`Lab`
//! (approximated by their device equivalents), `ICCBased` (alternate by `/N`),
//! and the three that carry a transform the renderer must run — `Indexed`
//! (palette lookup), `Separation` (1-input tint transform) and `DeviceN`
//! (N-input tint transform). **ICC-accurate** transforms are an explicit,
//! documented deviation (we fall back to the device interpretation of `/N`).
//!
//! `PdfFunction` ports the types 0 (sampled) / 2 (exponential) / 3 (stitching)
//! the renderer already supported, generalized to **multiple inputs** (DeviceN
//! tint transforms take one input per colorant). Type 4 (PostScript calculator)
//! stays deferred. `pdf-render` re-exports [`PdfFunction`]/[`ShadingColor`] so
//! there is exactly one evaluator in the workspace.

use crate::{Dict, DocumentStore, Name, Object};

/// A function's output color components (normalized to the function's `/Range`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShadingColor(pub Vec<f32>);

/// A PDF function (types 0/2/3); type 4 (PostScript) is a documented gap.
///
/// `eval` takes the **first** input (the common single-input shading/Separation
/// case, preserved bit-for-bit from the renderer's prior evaluator); `eval_n`
/// takes the full input vector (DeviceN tint transforms). For a type-2/3
/// function only the first input is consulted (they are 1-in by definition); a
/// type-0 sampled function honors every declared input axis.
#[derive(Clone, Debug, PartialEq)]
pub enum PdfFunction {
    /// Type 2 — exponential interpolation between `c0` and `c1`:
    /// `c0[i] + t^n · (c1[i] − c0[i])` (`t` clamped to `domain`).
    Exponential {
        /// The 1-input domain `[t0, t1]`.
        domain: [f32; 2],
        /// Output at `t = 0`.
        c0: Vec<f32>,
        /// Output at `t = 1`.
        c1: Vec<f32>,
        /// The interpolation exponent `N`.
        n: f32,
    },
    /// Type 3 — stitching: selects a sub-function by `bounds`, remapping `t` into
    /// each sub-function's domain via `encode`.
    Stitching {
        /// The 1-input domain `[t0, t1]`.
        domain: [f32; 2],
        /// The `k` sub-functions.
        functions: Vec<PdfFunction>,
        /// The `k − 1` interior bounds partitioning the domain.
        bounds: Vec<f32>,
        /// The `k` `[lo, hi]` encode pairs (one per sub-function).
        encode: Vec<[f32; 2]>,
    },
    /// Type 0 — sampled: `m`-input, `n_outputs`-output table linearly
    /// interpolated. `bits_per_sample` ∈ {1,2,4,8,16}.
    Sampled {
        /// The per-input domain `[lo, hi]` pairs (`m` entries).
        domain: Vec<[f32; 2]>,
        /// Number of samples along each input axis (`m` entries).
        size: Vec<usize>,
        /// Bits per stored sample (1/2/4/8/16).
        bits_per_sample: u8,
        /// Number of output components per sample.
        n_outputs: usize,
        /// The per-input `[lo, hi]` input→sample-index encode ranges.
        encode: Vec<[f32; 2]>,
        /// The per-output `[lo, hi]` decode ranges.
        decode: Vec<[f32; 2]>,
        /// Packed samples (MSB-first), `(∏ size) · n_outputs` values.
        samples: Vec<u8>,
    },
}

impl PdfFunction {
    /// Evaluates the function at single input `t` (the shading/Separation case).
    #[must_use]
    pub fn eval(&self, t: f32) -> ShadingColor {
        ShadingColor(self.eval_n(&[t]))
    }

    /// Evaluates the function at the full input vector (DeviceN tint transforms),
    /// returning the raw output components.
    #[must_use]
    pub fn eval_n(&self, inputs: &[f32]) -> Vec<f32> {
        let t = inputs.first().copied().unwrap_or(0.0);
        match self {
            PdfFunction::Exponential { domain, c0, c1, n } => {
                let t = clamp(t, domain[0], domain[1]);
                let tn = if *n == 1.0 { t } else { t.powf(*n) };
                c0.iter()
                    .zip(c1.iter())
                    .map(|(a, b)| a + tn * (b - a))
                    .collect()
            }
            PdfFunction::Stitching {
                domain,
                functions,
                bounds,
                encode,
            } => {
                let t = clamp(t, domain[0], domain[1]);
                if functions.is_empty() {
                    return vec![0.0];
                }
                let mut k = 0usize;
                while k < bounds.len() && t >= bounds[k] {
                    k += 1;
                }
                k = k.min(functions.len() - 1);
                let lo = if k == 0 { domain[0] } else { bounds[k - 1] };
                let hi = if k < bounds.len() {
                    bounds[k]
                } else {
                    domain[1]
                };
                let enc = encode.get(k).copied().unwrap_or([0.0, 1.0]);
                let e = interpolate(t, lo, hi, enc[0], enc[1]);
                functions[k].eval_n(&[e])
            }
            PdfFunction::Sampled {
                domain,
                size,
                bits_per_sample,
                n_outputs,
                encode,
                decode,
                samples,
            } => eval_sampled(
                inputs,
                domain,
                size,
                *bits_per_sample,
                *n_outputs,
                encode,
                decode,
                samples,
            ),
        }
    }
}

/// Evaluates a type-0 sampled function at the given multi-input vector, with
/// nearest-sample lookup per axis (multilinear interpolation only on the single
/// input axis, which covers Separation/shading; multi-axis DeviceN tables fall
/// back to nearest, a documented simplification).
#[allow(clippy::too_many_arguments)]
fn eval_sampled(
    inputs: &[f32],
    domain: &[[f32; 2]],
    size: &[usize],
    bits_per_sample: u8,
    n_outputs: usize,
    encode: &[[f32; 2]],
    decode: &[[f32; 2]],
    samples: &[u8],
) -> Vec<f32> {
    let m = size.len();
    if m == 0 || n_outputs == 0 || size.contains(&0) {
        return vec![0.0; n_outputs.max(1)];
    }
    let max = ((1u32 << bits_per_sample) - 1) as f32;

    // Single-input fast path: linear interpolation (shading/Separation).
    if m == 1 {
        let dom = domain.first().copied().unwrap_or([0.0, 1.0]);
        let enc = encode
            .first()
            .copied()
            .unwrap_or([0.0, (size[0].max(1) - 1) as f32]);
        let t = clamp(inputs.first().copied().unwrap_or(0.0), dom[0], dom[1]);
        let e = interpolate(t, dom[0], dom[1], enc[0], enc[1]);
        let e = clamp(e, 0.0, (size[0] - 1) as f32);
        let i0 = e.floor() as usize;
        let i1 = (i0 + 1).min(size[0] - 1);
        let frac = e - i0 as f32;
        return (0..n_outputs)
            .map(|o| {
                let s0 = read_norm(samples, i0, o, n_outputs, bits_per_sample, max);
                let s1 = read_norm(samples, i1, o, n_outputs, bits_per_sample, max);
                let s = s0 + frac * (s1 - s0);
                let d = decode.get(o).copied().unwrap_or([0.0, 1.0]);
                d[0] + s * (d[1] - d[0])
            })
            .collect();
    }

    // Multi-input: nearest-sample per axis (DeviceN). Row-major flat index.
    let mut flat = 0usize;
    for (axis, &axis_size) in size.iter().enumerate() {
        let dom = domain.get(axis).copied().unwrap_or([0.0, 1.0]);
        let enc = encode
            .get(axis)
            .copied()
            .unwrap_or([0.0, (axis_size.max(1) - 1) as f32]);
        let t = clamp(inputs.get(axis).copied().unwrap_or(0.0), dom[0], dom[1]);
        let e = interpolate(t, dom[0], dom[1], enc[0], enc[1]);
        let idx = clamp(e, 0.0, (axis_size - 1) as f32).round() as usize;
        flat = flat * axis_size + idx;
    }
    (0..n_outputs)
        .map(|o| {
            let s = read_norm(samples, flat, o, n_outputs, bits_per_sample, max);
            let d = decode.get(o).copied().unwrap_or([0.0, 1.0]);
            d[0] + s * (d[1] - d[0])
        })
        .collect()
}

/// Reads sample `(idx, out)` from a packed table and normalizes to `0..=1`.
fn read_norm(samples: &[u8], idx: usize, out: usize, n_outputs: usize, bps: u8, max: f32) -> f32 {
    let bit = (idx * n_outputs + out) * bps as usize;
    read_bits(samples, bit, bps as usize) as f32 / max
}

/// A resolved PDF colorspace able to map its native components to 8-bit sRGB.
///
/// `Indexed`/`Separation`/`DeviceN` carry the transform the renderer must run;
/// the rest are direct device mappings (ICC-accurate transforms are out of
/// scope — a documented deviation).
#[derive(Clone, Debug)]
pub enum ColorSpace {
    /// 1 component, gray.
    DeviceGray,
    /// 3 components, RGB.
    DeviceRgb,
    /// 4 components, CMYK.
    DeviceCmyk,
    /// CIE L*a*b* (approximated → sRGB).
    Lab,
    /// Indexed/palette: a `base` space, max index `hival`, and a packed lookup
    /// table of `(hival + 1) · base.n_components()` bytes.
    Indexed {
        /// The base (palette-entry) colorspace.
        base: Box<ColorSpace>,
        /// The maximum valid index.
        hival: usize,
        /// Packed base-space samples, `(hival + 1) · base.n` bytes (0..=255).
        lookup: Vec<u8>,
    },
    /// Separation: a single tint mapped through `tint` into `alt`.
    Separation {
        /// The alternate colorspace the tint transform outputs into.
        alt: Box<ColorSpace>,
        /// The 1-input tint transform.
        tint: Box<PdfFunction>,
    },
    /// DeviceN: `n` tints mapped through `tint` into `alt`.
    DeviceN {
        /// Number of colorants (input tints).
        n: usize,
        /// The alternate colorspace the tint transform outputs into.
        alt: Box<ColorSpace>,
        /// The N-input tint transform.
        tint: Box<PdfFunction>,
    },
}

impl ColorSpace {
    /// Number of input components for this colorspace (what an image's samples or
    /// an `scn` operand list carry per pixel).
    #[must_use]
    pub fn n_components(&self) -> usize {
        match self {
            ColorSpace::DeviceGray => 1,
            ColorSpace::DeviceRgb | ColorSpace::Lab => 3,
            ColorSpace::DeviceCmyk => 4,
            ColorSpace::Indexed { .. } | ColorSpace::Separation { .. } => 1,
            ColorSpace::DeviceN { n, .. } => *n,
        }
    }

    /// Maps native components (each `0..=1`) to 8-bit sRGB. Indexed expects a
    /// single index in `comps[0]` scaled `0..=1` *only* via [`ColorSpace::index_to_rgb`];
    /// for the component form callers pass already-normalized tints/components.
    #[must_use]
    pub fn to_rgb8(&self, comps: &[f32]) -> [u8; 3] {
        match self {
            ColorSpace::DeviceGray => {
                let g = quant(comps.first().copied().unwrap_or(0.0));
                [g, g, g]
            }
            ColorSpace::DeviceRgb => [
                quant(comps.first().copied().unwrap_or(0.0)),
                quant(comps.get(1).copied().unwrap_or(0.0)),
                quant(comps.get(2).copied().unwrap_or(0.0)),
            ],
            ColorSpace::DeviceCmyk => {
                let c = comps.first().copied().unwrap_or(0.0);
                let m = comps.get(1).copied().unwrap_or(0.0);
                let y = comps.get(2).copied().unwrap_or(0.0);
                let k = comps.get(3).copied().unwrap_or(0.0);
                cmyk_to_rgb(c, m, y, k)
            }
            ColorSpace::Lab => lab_to_rgb(
                comps.first().copied().unwrap_or(0.0),
                comps.get(1).copied().unwrap_or(0.0),
                comps.get(2).copied().unwrap_or(0.0),
            ),
            ColorSpace::Indexed {
                base,
                hival,
                lookup,
            } => {
                // The component form receives a *normalized* index in [0,1]
                // spanning [0, hival]; round to the nearest palette entry.
                let idx = (comps.first().copied().unwrap_or(0.0) * *hival as f32)
                    .round()
                    .clamp(0.0, *hival as f32) as usize;
                index_to_rgb(base, *hival, lookup, idx)
            }
            ColorSpace::Separation { alt, tint } => {
                let out = tint.eval_n(&[comps.first().copied().unwrap_or(0.0)]);
                alt.to_rgb8(&out)
            }
            ColorSpace::DeviceN { n, alt, tint } => {
                let mut inputs = Vec::with_capacity(*n);
                for i in 0..*n {
                    inputs.push(comps.get(i).copied().unwrap_or(0.0));
                }
                let out = tint.eval_n(&inputs);
                alt.to_rgb8(&out)
            }
        }
    }

    /// Maps a raw integer palette `index` (for an `Indexed` space) to 8-bit sRGB.
    /// For a non-`Indexed` space this is the same as feeding the index as the
    /// sole component (rare; the image path only calls it for `Indexed`).
    #[must_use]
    pub fn index_to_rgb(&self, index: usize) -> [u8; 3] {
        match self {
            ColorSpace::Indexed {
                base,
                hival,
                lookup,
            } => index_to_rgb(base, *hival, lookup, index),
            other => other.to_rgb8(&[index as f32]),
        }
    }

    /// Resolves a `/ColorSpace` object into a [`ColorSpace`].
    ///
    /// `resources` is the page/form `/Resources` dict so a *name* operand (the
    /// `cs`/`CS` case, e.g. `/Cs1`) can be looked up in `/Resources /ColorSpace`.
    /// Returns `None` for an unresolvable / unsupported (e.g. `Pattern`) space.
    #[must_use]
    pub fn resolve(doc: &DocumentStore, obj: &Object, resources: Option<&Dict>) -> Option<Self> {
        Self::resolve_depth(doc, obj, resources, 0)
    }

    fn resolve_depth(
        doc: &DocumentStore,
        obj: &Object,
        resources: Option<&Dict>,
        depth: u32,
    ) -> Option<Self> {
        if depth > 8 {
            return None;
        }
        let obj = resolve_obj(doc, obj)?;
        match obj.as_ref() {
            Object::Name(n) => {
                let name = n.as_str()?;
                if let Some(cs) = device_by_name(name) {
                    return Some(cs);
                }
                // A resource name (e.g. `/Cs1`): look it up in /Resources/ColorSpace.
                let res = resources?;
                let csd = res.get(&Name::new("ColorSpace")).and_then(Object::as_dict);
                let entry = match csd {
                    Some(d) => d.get(&Name::new(name)).cloned(),
                    None => resolve_obj(doc, res.get(&Name::new("ColorSpace"))?)
                        .and_then(|o| o.as_dict().and_then(|d| d.get(&Name::new(name)).cloned())),
                }?;
                Self::resolve_depth(doc, &entry, resources, depth + 1)
            }
            Object::Array(arr) => Self::resolve_array(doc, arr, resources, depth),
            _ => None,
        }
    }

    fn resolve_array(
        doc: &DocumentStore,
        arr: &[Object],
        resources: Option<&Dict>,
        depth: u32,
    ) -> Option<Self> {
        let head = arr
            .first()
            .and_then(Object::as_name)
            .and_then(Name::as_str)?;
        match head {
            "ICCBased" => {
                let n = arr
                    .get(1)
                    .and_then(|o| resolve_obj(doc, o))
                    .and_then(|o| {
                        o.as_stream()
                            .and_then(|s| s.dict.get(&Name::new("N")).cloned())
                    })
                    .and_then(|o| o.as_i64());
                Some(match n {
                    Some(1) => ColorSpace::DeviceGray,
                    Some(4) => ColorSpace::DeviceCmyk,
                    _ => ColorSpace::DeviceRgb,
                })
            }
            "CalGray" => Some(ColorSpace::DeviceGray),
            "CalRGB" => Some(ColorSpace::DeviceRgb),
            "Lab" => Some(ColorSpace::Lab),
            "ICC" => Some(ColorSpace::DeviceRgb),
            "Indexed" | "I" => {
                let base = Self::resolve_depth(doc, arr.get(1)?, resources, depth + 1)?;
                let hival = arr
                    .get(2)
                    .and_then(Object::as_i64)
                    .map(|v| v.max(0) as usize)?;
                let lookup = read_lookup(doc, arr.get(3)?)?;
                Some(ColorSpace::Indexed {
                    base: Box::new(base),
                    hival,
                    lookup,
                })
            }
            "Separation" => {
                let alt = Self::resolve_depth(doc, arr.get(2)?, resources, depth + 1)?;
                let tint = parse_function(doc, arr.get(3)?)?;
                Some(ColorSpace::Separation {
                    alt: Box::new(alt),
                    tint: Box::new(tint),
                })
            }
            "DeviceN" => {
                let n = arr.get(1).and_then(Object::as_array).map(|a| a.len())?;
                let alt = Self::resolve_depth(doc, arr.get(2)?, resources, depth + 1)?;
                let tint = parse_function(doc, arr.get(3)?)?;
                Some(ColorSpace::DeviceN {
                    n: n.max(1),
                    alt: Box::new(alt),
                    tint: Box::new(tint),
                })
            }
            // A single-element wrapper like `[/DeviceRGB]`.
            _ => device_by_name(head),
        }
    }
}

fn device_by_name(name: &str) -> Option<ColorSpace> {
    match name {
        "DeviceGray" | "G" | "CalGray" => Some(ColorSpace::DeviceGray),
        "DeviceRGB" | "RGB" | "CalRGB" => Some(ColorSpace::DeviceRgb),
        "DeviceCMYK" | "CMYK" => Some(ColorSpace::DeviceCmyk),
        "Lab" => Some(ColorSpace::Lab),
        _ => None,
    }
}

/// Reads an `Indexed` `/lookup` (a string literal or a stream) to packed bytes.
fn read_lookup(doc: &DocumentStore, obj: &Object) -> Option<Vec<u8>> {
    let obj = resolve_obj(doc, obj)?;
    match obj.as_ref() {
        Object::String(s) => Some(s.as_bytes().to_vec()),
        Object::Stream(s) => doc
            .decode_stream(s)
            .ok()
            .and_then(|o| o.into_decoded().ok()),
        _ => None,
    }
}

/// Maps a palette `index` to sRGB by reading the `base`-space components from the
/// `lookup` table and converting them.
fn index_to_rgb(base: &ColorSpace, hival: usize, lookup: &[u8], index: usize) -> [u8; 3] {
    let nc = base.n_components();
    let index = index.min(hival);
    let off = index * nc;
    if off + nc > lookup.len() {
        return [0, 0, 0];
    }
    let comps: Vec<f32> = lookup[off..off + nc]
        .iter()
        .map(|&b| b as f32 / 255.0)
        .collect();
    base.to_rgb8(&comps)
}

/// Parses a `/Function` object (dict, stream, or array of single-output
/// functions) into a [`PdfFunction`]. Types 0/2/3; type 4 (PostScript) is
/// deferred (`None`).
#[must_use]
pub fn parse_function(doc: &DocumentStore, obj: &Object) -> Option<PdfFunction> {
    let obj = resolve_obj(doc, obj)?;
    match obj.as_ref() {
        Object::Array(arr) => combine_function_array(doc, arr),
        Object::Dictionary(d) => function_from_dict(doc, d, None),
        Object::Stream(s) => {
            let data = doc.decode_stream(s).and_then(|o| o.into_decoded()).ok();
            function_from_dict(doc, &s.dict, data.as_deref())
        }
        _ => None,
    }
}

/// Combines an array of single-output `/Function`s (e.g. `[f_r f_g f_b]`) into
/// one multi-output [`PdfFunction`]. Merges a vector of type-2 exponentials into
/// one (concatenated outputs); else takes the first.
fn combine_function_array(doc: &DocumentStore, arr: &[Object]) -> Option<PdfFunction> {
    if arr.is_empty() {
        return None;
    }
    let funcs: Vec<PdfFunction> = arr.iter().filter_map(|o| parse_function(doc, o)).collect();
    if funcs.is_empty() {
        return None;
    }
    let mut c0 = Vec::new();
    let mut c1 = Vec::new();
    let mut domain = [0.0f32, 1.0];
    let mut n = 1.0f32;
    let mut all_exp = true;
    for f in &funcs {
        if let PdfFunction::Exponential {
            domain: d,
            c0: a,
            c1: b,
            n: e,
        } = f
        {
            domain = *d;
            n = *e;
            c0.extend_from_slice(a);
            c1.extend_from_slice(b);
        } else {
            all_exp = false;
            break;
        }
    }
    if all_exp && !c0.is_empty() {
        return Some(PdfFunction::Exponential { domain, c0, c1, n });
    }
    funcs.into_iter().next()
}

/// Builds a [`PdfFunction`] from a function dict + optional decoded stream data
/// (for a type-0 sampled function). `None` for type 4 or a malformed dict.
fn function_from_dict(doc: &DocumentStore, d: &Dict, data: Option<&[u8]>) -> Option<PdfFunction> {
    let ftype = d.get(&Name::new("FunctionType")).and_then(Object::as_i64)?;
    match ftype {
        2 => {
            let domain = read_pair(d, "Domain").unwrap_or([0.0, 1.0]);
            let c0 = read_floats(d, "C0").unwrap_or_else(|| vec![0.0]);
            let c1 = read_floats(d, "C1").unwrap_or_else(|| vec![1.0]);
            let n = d
                .get(&Name::new("N"))
                .and_then(Object::as_f64)
                .unwrap_or(1.0) as f32;
            Some(PdfFunction::Exponential { domain, c0, c1, n })
        }
        3 => {
            let domain = read_pair(d, "Domain").unwrap_or([0.0, 1.0]);
            let sub = d.get(&Name::new("Functions")).and_then(Object::as_array)?;
            let functions: Vec<PdfFunction> =
                sub.iter().filter_map(|o| parse_function(doc, o)).collect();
            if functions.is_empty() {
                return None;
            }
            let bounds = read_floats(d, "Bounds").unwrap_or_default();
            let encode = read_pairs(d, "Encode");
            Some(PdfFunction::Stitching {
                domain,
                functions,
                bounds,
                encode,
            })
        }
        0 => {
            let data = data?;
            let domain = read_pairs(d, "Domain");
            let size: Vec<usize> = d
                .get(&Name::new("Size"))
                .and_then(Object::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|o| o.as_i64().map(|v| v.max(0) as usize))
                        .collect()
                })
                .unwrap_or_default();
            if size.is_empty() {
                return None;
            }
            let bits_per_sample = d
                .get(&Name::new("BitsPerSample"))
                .and_then(Object::as_i64)? as u8;
            let decode = read_pairs(d, "Decode");
            let range = read_pairs(d, "Range");
            let n_outputs = if !decode.is_empty() {
                decode.len()
            } else if !range.is_empty() {
                range.len()
            } else {
                1
            };
            let domain = if domain.is_empty() {
                vec![[0.0, 1.0]; size.len()]
            } else {
                domain
            };
            let encode = {
                let e = read_pairs(d, "Encode");
                if e.is_empty() {
                    size.iter().map(|&s| [0.0, (s.max(1) - 1) as f32]).collect()
                } else {
                    e
                }
            };
            let decode = if !decode.is_empty() {
                decode
            } else if !range.is_empty() {
                range
            } else {
                vec![[0.0, 1.0]; n_outputs]
            };
            Some(PdfFunction::Sampled {
                domain,
                size,
                bits_per_sample,
                n_outputs,
                encode,
                decode,
                samples: data.to_vec(),
            })
        }
        _ => None,
    }
}

// === shared numeric helpers ===============================================

fn read_pair(d: &Dict, key: &str) -> Option<[f32; 2]> {
    let a = d.get(&Name::new(key)).and_then(Object::as_array)?;
    if a.len() < 2 {
        return None;
    }
    Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32])
}

fn read_floats(d: &Dict, key: &str) -> Option<Vec<f32>> {
    let a = d.get(&Name::new(key)).and_then(Object::as_array)?;
    Some(
        a.iter()
            .filter_map(|o| o.as_f64().map(|v| v as f32))
            .collect(),
    )
}

fn read_pairs(d: &Dict, key: &str) -> Vec<[f32; 2]> {
    let Some(a) = d.get(&Name::new(key)).and_then(Object::as_array) else {
        return Vec::new();
    };
    let flat: Vec<f32> = a
        .iter()
        .filter_map(|o| o.as_f64().map(|v| v as f32))
        .collect();
    flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
}

fn resolve_obj(doc: &DocumentStore, obj: &Object) -> Option<std::sync::Arc<Object>> {
    match obj {
        Object::Reference(r) => doc.resolve(*r).ok(),
        other => Some(std::sync::Arc::new(other.clone())),
    }
}

#[inline]
fn quant(v: f32) -> u8 {
    (clamp(v, 0.0, 1.0) * 255.0 + 0.5) as u8
}

/// CMYK (each `0..=1`) → 8-bit sRGB.
///
/// Analytic (non-ICC) multiplicative conversion with a SWOP-like **black
/// point**: the naive `(1-ink)*(1-k)` model drives pure process black
/// (`0 0 0 1 k`) to `(0,0,0)`, but a color-managed renderer (PyMuPDF/fitz, via
/// US Web Coated SWOP) never reaches pure black from CMYK — its darkest K is
/// `~(34,31,31)`. We model that by remapping the K axis from white down to a
/// per-channel ink **floor** instead of to zero, then multiplying by the CMY
/// ink complement. With a zero floor this reduces exactly to the old naive
/// transform, so only the neutral/black region shifts toward fitz.
///
/// This matches fitz's pure-K ramp (incl. the `(34,31,31)` floor), registration
/// black (`1 1 1 1 k → (0,0,0)`) and white; the saturated CMY primaries still
/// differ (a true ICC profile is required to capture per-ink absorption — that
/// remains deferred). Cross-checked against `.venv-oracle` `get_pixmap`.
fn cmyk_to_rgb(c: f32, m: f32, y: f32, k: f32) -> [u8; 3] {
    let (c, m, y, k) = (
        c.clamp(0.0, 1.0),
        m.clamp(0.0, 1.0),
        y.clamp(0.0, 1.0),
        k.clamp(0.0, 1.0),
    );
    // Per-channel K floor (fitz SWOP darkest-K `(34,31,31)`, normalized).
    let ch = |ink: f32, floor: f32| {
        let k_axis = floor + (1.0 - floor) * (1.0 - k);
        quant(k_axis * (1.0 - ink))
    };
    [
        ch(c, 34.0 / 255.0),
        ch(m, 31.0 / 255.0),
        ch(y, 31.0 / 255.0),
    ]
}

/// CIE L*a*b* → sRGB (D50 white point, approximate). `l` is `0..=1` (scaled from
/// `0..=100`); `a`/`b` are `0..=1` scaled from the typical `-128..=127` range.
fn lab_to_rgb(l: f32, a: f32, b: f32) -> [u8; 3] {
    // Undo the normalization the image path applies (l*100, a/b → [-128,127]).
    let l = l * 100.0;
    let a = a * 255.0 - 128.0;
    let b = b * 255.0 - 128.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let g = |t: f32| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    let (xn, yn, zn) = (0.9642, 1.0, 0.8249);
    let (x, y, z) = (xn * g(fx), yn * g(fy), zn * g(fz));
    let r = 3.1338 * x - 1.6168 * y - 0.4906 * z;
    let gg = -0.9787 * x + 1.9161 * y + 0.0334 * z;
    let bb = 0.0719 * x - 0.2289 * y + 1.4052 * z;
    let gamma = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    [quant(gamma(r)), quant(gamma(gg)), quant(gamma(bb))]
}

#[inline]
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Linear remap of `v` from `[a0, a1]` onto `[b0, b1]`. A zero-width source maps
/// to `b0`.
#[inline]
fn interpolate(v: f32, a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    if (a1 - a0).abs() < f32::EPSILON {
        b0
    } else {
        b0 + (v - a0) * (b1 - b0) / (a1 - a0)
    }
}

/// Reads `count` (≤16) MSB-first bits at bit-offset `bit` from `data`.
fn read_bits(data: &[u8], bit: usize, count: usize) -> u32 {
    let mut v = 0u32;
    for i in 0..count {
        let b = bit + i;
        let byte = data.get(b / 8).copied().unwrap_or(0);
        let shift = 7 - (b % 8);
        v = (v << 1) | ((byte >> shift) & 1) as u32;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_single_input() {
        let f = PdfFunction::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0, 0.0, 0.0],
            c1: vec![0.2, 0.1, 0.3],
            n: 1.0,
        };
        // Dark spot ink: tint 1.0 → the dark alternate color, not white.
        assert_eq!(f.eval_n(&[1.0]), vec![0.2, 0.1, 0.3]);
        assert_eq!(f.eval_n(&[0.0]), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn separation_dark_ink_to_rgb() {
        // A Separation whose tint transform maps 1.0 → a dark CMYK ink.
        let cs = ColorSpace::Separation {
            alt: Box::new(ColorSpace::DeviceCmyk),
            tint: Box::new(PdfFunction::Exponential {
                domain: [0.0, 1.0],
                c0: vec![0.0, 0.0, 0.0, 0.0],
                c1: vec![0.0, 0.0, 0.0, 1.0], // full black
                n: 1.0,
            }),
        };
        // Full process black: the SWOP-like black point lands at fitz's
        // darkest-K `(34,31,31)`, not pure `(0,0,0)` (P3-3r).
        assert_eq!(cs.to_rgb8(&[1.0]), [34, 31, 31]);
        assert_eq!(cs.to_rgb8(&[0.0]), [255, 255, 255]);
    }

    #[test]
    fn cmyk_black_point_matches_fitz() {
        let cmyk = ColorSpace::DeviceCmyk;
        // Pure process black `0 0 0 1 k` → fitz SWOP darkest-K, not (0,0,0).
        assert_eq!(cmyk.to_rgb8(&[0.0, 0.0, 0.0, 1.0]), [34, 31, 31]);
        // Registration black `1 1 1 1 k` (full ink) still → (0,0,0).
        assert_eq!(cmyk.to_rgb8(&[1.0, 1.0, 1.0, 1.0]), [0, 0, 0]);
        // No ink → white (unchanged).
        assert_eq!(cmyk.to_rgb8(&[0.0, 0.0, 0.0, 0.0]), [255, 255, 255]);
        // Pure CMY primaries: the K axis is untouched, so the floor does not
        // shift them — they keep the naive complement (matches fitz to ±1 on
        // the inkless channels; saturated-ink channels need ICC, deferred).
        assert_eq!(cmyk.to_rgb8(&[1.0, 0.0, 0.0, 0.0]), [0, 255, 255]);
        assert_eq!(cmyk.to_rgb8(&[0.0, 0.0, 1.0, 0.0]), [255, 255, 0]);
        // Pure-K midtone tracks fitz's near-linear gray ramp (fitz: 147/149/151).
        let mid = cmyk.to_rgb8(&[0.0, 0.0, 0.0, 0.5]);
        assert!(mid.iter().all(|&v| (143..=151).contains(&v)), "mid={mid:?}");
    }

    #[test]
    fn indexed_palette_lookup() {
        // 2-entry RGB palette: index 0 = red, index 1 = green.
        let cs = ColorSpace::Indexed {
            base: Box::new(ColorSpace::DeviceRgb),
            hival: 1,
            lookup: vec![255, 0, 0, 0, 255, 0],
        };
        assert_eq!(cs.index_to_rgb(0), [255, 0, 0]);
        assert_eq!(cs.index_to_rgb(1), [0, 255, 0]);
        assert_eq!(cs.n_components(), 1);
    }

    #[test]
    fn devicen_two_inputs() {
        let cs = ColorSpace::DeviceN {
            n: 2,
            alt: Box::new(ColorSpace::DeviceRgb),
            tint: Box::new(PdfFunction::Sampled {
                domain: vec![[0.0, 1.0], [0.0, 1.0]],
                size: vec![2, 2],
                bits_per_sample: 8,
                n_outputs: 3,
                encode: vec![[0.0, 1.0], [0.0, 1.0]],
                decode: vec![[0.0, 1.0]; 3],
                // 2x2x3 table: [0,0]=black, [0,1]=red, [1,0]=green, [1,1]=white
                samples: vec![
                    0, 0, 0, 255, 0, 0, // row 0
                    0, 255, 0, 255, 255, 255, // row 1
                ],
            }),
        };
        assert_eq!(cs.n_components(), 2);
        assert_eq!(cs.to_rgb8(&[0.0, 0.0]), [0, 0, 0]);
        assert_eq!(cs.to_rgb8(&[1.0, 1.0]), [255, 255, 255]);
        assert_eq!(cs.to_rgb8(&[0.0, 1.0]), [255, 0, 0]);
    }
}
