//! Lazily-built, shape-bucketed [`tract`] runnables for the three PP-OCR models.
//!
//! `into_optimized()` is expensive (~1–2s per distinct concrete input shape), so
//! each model caches one runnable per shape bucket in a `HashMap`. The detection
//! and recognition models have dynamic input dims (symbolic `H`/`W` in the
//! shipped ONNX), so we pin a concrete fact at first use of each bucket; the
//! classifier takes a fixed 3×80×160 crop and uses a single runnable.
//!
//! The ~16 MB ONNX model files are NOT embedded in the binary (that bloated
//! every wheel, even for non-OCR users — see P0-5). Instead they are loaded from
//! a resolvable directory at runtime (offline, no network): the
//! `PDFSPINE_OCR_MODELS` environment variable when set, else the in-repo
//! `crates/pdf-ocr/models` directory (via `CARGO_MANIFEST_DIR`) so `cargo test`
//! and `maturin develop` work in a checkout with no setup. The tiny (~26 KB)
//! recognition dictionary stays embedded with [`include_str!`].

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Mutex;

use tract_onnx::prelude::*;

use crate::error::{Error, Result};

/// The optimized + runnable typed model produced by `into_optimized().into_runnable()`.
pub(crate) type Runnable = TypedRunnableModel<TypedModel>;

// --- Model files: loaded from disk at runtime (NOT embedded). ---

/// Environment variable pointing at the directory that holds the three
/// `*.onnx` model files. Set by the `pdfspine[ocr]` install (or by a user who
/// places the models elsewhere). Overrides the in-repo default.
const ENV_MODELS_DIR: &str = "PDFSPINE_OCR_MODELS";

/// PP-OCRv5 DBNet text-detection model. Input `[1,3,H,W]`, output prob map
/// `[1,1,H,W]`.
const DET_FILE: &str = "ppocrv5_det.onnx";
/// PP-OCRv5 CRNN+CTC recognition model. Input `[1,3,48,W]`, output softmax probs
/// `[1,T,18385]`.
const REC_FILE: &str = "ppocrv5_rec.onnx";
/// PP-OCRv5 text-line orientation classifier (PP-LCNet_x1_0_textline_ori). Input
/// concrete `[1,3,80,160]`, output `[1,2]` (0° / 180°).
const CLS_FILE: &str = "ppocrv5_cls.onnx";

/// The recognition dictionary, INDEX-ALIGNED to the rec output's class axis:
/// line 0 = the CTC blank, lines 1.. = characters, last line = a single space.
/// This is tiny (~26 KB) and stays embedded; only the multi-MB ONNX weights are
/// loaded from disk. We must preserve the trailing space line, so we split on
/// `'\n'` (not `lines()`, which would also be fine, but we keep this explicit)
/// and do NOT trim.
const KEYS: &str = include_str!("../../models/ppocr_keys_v5.txt");

/// Resolves the directory holding the ONNX model files. Prefers the
/// `PDFSPINE_OCR_MODELS` environment variable (the `pdfspine[ocr]` install /
/// user override); falls back to the in-repo `crates/pdf-ocr/models` directory
/// so a source checkout works with no setup. Never touches the network.
fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(ENV_MODELS_DIR) {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models")
}

/// Reads a model file from the resolved [`models_dir`], mapping a missing file
/// or read error to a clear `Unsupported` error that points the user at the
/// `pdfspine[ocr]` extra / the `PDFSPINE_OCR_MODELS` override.
fn read_model(file: &str) -> Result<Vec<u8>> {
    let path = models_dir().join(file);
    std::fs::read(&path).map_err(|e| {
        Error::Unsupported(format!(
            "paddle: OCR model {file:?} not found at {} ({e}). Install the OCR \
             models with `pip install pdfspine[ocr]`, or point \
             `{ENV_MODELS_DIR}` at the directory holding the PP-OCR `*.onnx` files.",
            path.display(),
        ))
    })
}

/// Builds an `InferenceModel` from ONNX bytes, mapping any tract error into our
/// typed `Unsupported` error (a failure here is a build/environment problem,
/// surfaced — never a panic).
fn proto(bytes: &[u8]) -> Result<InferenceModel> {
    tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(bytes))
        .map_err(|e| Error::Unsupported(format!("paddle: failed to parse ONNX model: {e}")))
}

/// Pins a concrete `[1,3,h,w]` f32 input fact, optimizes, and makes the model
/// runnable. This is the per-bucket cost we cache.
fn build_runnable(model: InferenceModel, h: usize, w: usize) -> Result<Runnable> {
    model
        .with_input_fact(0, f32::fact([1, 3, h, w]).into())
        .and_then(|m| m.into_optimized())
        .and_then(|m| m.into_runnable())
        .map_err(|e| Error::Unsupported(format!("paddle: failed to optimize model: {e}")))
}

/// The recognition character table (decoded once at construction).
///
/// `table[i]` is the string emitted for class index `i`. Index 0 (the CTC
/// blank) is stored as an empty string so the decoder can index uniformly; it is
/// also skipped explicitly during decode.
pub(crate) struct CharTable {
    table: Vec<String>,
}

impl CharTable {
    fn load() -> Self {
        // Split on '\n' preserving every line, including a trailing space line.
        // A final empty element from a trailing newline is dropped (the file
        // ends with the space line + '\n'); the space line itself is kept.
        let mut table: Vec<String> = KEYS.split('\n').map(|s| s.to_string()).collect();
        if table.last().map(|s| s.is_empty()).unwrap_or(false) {
            table.pop();
        }
        // Index 0 is the blank: blank out its label so it never contributes text.
        if let Some(first) = table.first_mut() {
            first.clear();
        }
        CharTable { table }
    }

    /// The label for class index `i` (`""` for the blank or out-of-range).
    #[inline]
    pub(crate) fn get(&self, i: usize) -> &str {
        self.table.get(i).map(String::as_str).unwrap_or("")
    }

    /// The number of classes (should equal the rec model's output width, 18385).
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.table.len()
    }
}

/// Holds the three models' lazily-built runnables and the recognition dict.
///
/// Detection caches per `(h, w)` and recognition per padded width (`(48, w)`),
/// both keyed by the same `(h, w)` map. The classifier is concrete (80×160), so
/// it builds exactly one runnable. Caches are behind a `Mutex` so `recognize(&self, ..)`
/// stays `&self` (the `OcrEngine` contract) while still memoizing across calls.
pub(crate) struct Models {
    det: Mutex<HashMap<(usize, usize), std::sync::Arc<Runnable>>>,
    rec: Mutex<HashMap<(usize, usize), std::sync::Arc<Runnable>>>,
    cls: std::sync::OnceLock<std::sync::Arc<Runnable>>,
    pub(crate) chars: CharTable,
}

impl Models {
    /// Constructs the model holder. This does NOT optimize any model yet (the
    /// expensive `into_optimized()` happens lazily per shape bucket), so it is
    /// cheap; only the dictionary is decoded eagerly.
    pub(crate) fn new() -> Result<Self> {
        Ok(Models {
            det: Mutex::new(HashMap::new()),
            rec: Mutex::new(HashMap::new()),
            cls: std::sync::OnceLock::new(),
            chars: CharTable::load(),
        })
    }

    /// The detection runnable for input height `h`, width `w` (cached).
    pub(crate) fn det(&self, h: usize, w: usize) -> Result<std::sync::Arc<Runnable>> {
        if let Some(r) = self.det.lock().unwrap().get(&(h, w)) {
            return Ok(r.clone());
        }
        let runnable = std::sync::Arc::new(build_runnable(proto(&read_model(DET_FILE)?)?, h, w)?);
        self.det.lock().unwrap().insert((h, w), runnable.clone());
        Ok(runnable)
    }

    /// The recognition runnable for a padded crop of height 48 and width `w`
    /// (cached per width).
    pub(crate) fn rec(&self, w: usize) -> Result<std::sync::Arc<Runnable>> {
        let key = (48usize, w);
        if let Some(r) = self.rec.lock().unwrap().get(&key) {
            return Ok(r.clone());
        }
        let runnable = std::sync::Arc::new(build_runnable(proto(&read_model(REC_FILE)?)?, 48, w)?);
        self.rec.lock().unwrap().insert(key, runnable.clone());
        Ok(runnable)
    }

    /// The (single, concrete) classifier runnable, built on first use.
    pub(crate) fn cls(&self) -> Result<std::sync::Arc<Runnable>> {
        if let Some(r) = self.cls.get() {
            return Ok(r.clone());
        }
        let runnable =
            std::sync::Arc::new(build_runnable(proto(&read_model(CLS_FILE)?)?, 80, 160)?);
        // OnceLock: if a concurrent caller won the race, use theirs.
        let _ = self.cls.set(runnable);
        Ok(self.cls.get().expect("just set").clone())
    }
}
