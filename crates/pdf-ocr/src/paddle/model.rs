//! Lazily-built, shape-bucketed [`tract`] runnables for the three PP-OCR models.
//!
//! `into_optimized()` is expensive (~1–2s per distinct concrete input shape), so
//! each model caches one runnable per shape bucket in a `HashMap`. The detection
//! and recognition models have dynamic input dims (symbolic `H`/`W` in the
//! shipped ONNX), so we pin a concrete fact at first use of each bucket; the
//! classifier is already fully concrete (3×48×192) and uses a single runnable.
//!
//! Model bytes are embedded with [`include_bytes!`] and the recognition
//! dictionary with [`include_str!`], so the engine needs no runtime file paths
//! (the wheel ships nothing on disk).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;

use tract_onnx::prelude::*;

use crate::error::{Error, Result};

/// The optimized + runnable typed model produced by `into_optimized().into_runnable()`.
pub(crate) type Runnable = TypedRunnableModel<TypedModel>;

// --- Embedded assets (no runtime file paths). ---

/// DBNet text-detection model. Input `[1,3,H,W]`, output prob map `[1,1,H,W]`.
const DET_ONNX: &[u8] = include_bytes!("../../models/ppocrv4_det.onnx");
/// CRNN+CTC recognition model. Input `[1,3,48,W]`, output logits `[1,T,6625]`.
const REC_ONNX: &[u8] = include_bytes!("../../models/ppocrv4_rec.onnx");
/// 180° angle classifier. Input concrete `[1,3,48,192]`, output `[1,2]`.
const CLS_ONNX: &[u8] = include_bytes!("../../models/ppocrv2_cls.onnx");

/// The recognition dictionary, INDEX-ALIGNED to the rec output's class axis:
/// line 0 = the CTC blank, lines 1.. = characters, last line = a single space.
/// We must preserve the trailing space line, so we split on `'\n'` (not
/// `lines()`, which would also be fine, but we keep this explicit) and do NOT
/// trim.
const KEYS: &str = include_str!("../../models/ppocr_keys_v4.txt");

/// Builds an `InferenceModel` from embedded ONNX bytes, mapping any tract error
/// into our typed `Unsupported` error (the model is shipped, so a failure here
/// is a build/environment problem, surfaced — never a panic).
fn proto(bytes: &'static [u8]) -> Result<InferenceModel> {
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

    /// The number of classes (should equal the rec model's output width, 6625).
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.table.len()
    }
}

/// Holds the three models' lazily-built runnables and the recognition dict.
///
/// Detection caches per `(h, w)` and recognition per padded width (`(48, w)`),
/// both keyed by the same `(h, w)` map. The classifier is concrete, so it builds
/// exactly one runnable. Caches are behind a `Mutex` so `recognize(&self, ..)`
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
        let runnable = std::sync::Arc::new(build_runnable(proto(DET_ONNX)?, h, w)?);
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
        let runnable = std::sync::Arc::new(build_runnable(proto(REC_ONNX)?, 48, w)?);
        self.rec.lock().unwrap().insert(key, runnable.clone());
        Ok(runnable)
    }

    /// The (single, concrete) classifier runnable, built on first use.
    pub(crate) fn cls(&self) -> Result<std::sync::Arc<Runnable>> {
        if let Some(r) = self.cls.get() {
            return Ok(r.clone());
        }
        let runnable = std::sync::Arc::new(build_runnable(proto(CLS_ONNX)?, 48, 192)?);
        // OnceLock: if a concurrent caller won the race, use theirs.
        let _ = self.cls.set(runnable);
        Ok(self.cls.get().expect("just set").clone())
    }
}
