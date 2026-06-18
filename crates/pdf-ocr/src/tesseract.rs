//! The default OCR adapter: shell out to the system **Tesseract** CLI.
//!
//! Tesseract is **not bundled** (it is GPL/Apache C++ with large language data);
//! exactly like PyMuPDF, the user is expected to have `tesseract` installed. The
//! adapter keeps the pdfspine wheel pure-Rust and clean.
//!
//! Pipeline: the input [`Pixmap`] is encoded to a temporary PNG, then
//! `tesseract <png> stdout --psm <psm> -l <lang> tsv` is run. The TSV output
//! carries one row per layout node; the word rows (`level == 5`) give
//! `left/top/width/height/conf/text`, which parse straight into [`OcrWord`]s in
//! image pixel space. The temp file is always removed.
//!
//! Two environment overrides exist (tests + power users):
//! - `OXIDE_TESSERACT` — path to the `tesseract` binary (default: `tesseract`,
//!   resolved via `PATH`).
//! - `OXIDE_TESSDATA` — value for `TESSDATA_PREFIX` (the language-data dir).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use pdf_core::geom::Rect;

use crate::engine::{OcrEngine, OcrWord};
use crate::error::{Error, Result};
use pdf_image::pixmap::Pixmap;

/// Environment override for the `tesseract` binary path.
pub const ENV_BINARY: &str = "OXIDE_TESSERACT";
/// Environment override for the tessdata (language-data) directory.
pub const ENV_TESSDATA: &str = "OXIDE_TESSDATA";

/// The default Tesseract page-segmentation mode (`3` = fully automatic, the
/// PyMuPDF default for `get_textpage_ocr`).
pub const DEFAULT_PSM: u32 = 3;

/// The default OCR adapter: drives the system `tesseract` CLI.
#[derive(Clone, Debug)]
pub struct TesseractCli {
    binary: String,
    tessdata: Option<PathBuf>,
    psm: u32,
}

impl Default for TesseractCli {
    fn default() -> Self {
        TesseractCli {
            binary: std::env::var(ENV_BINARY).unwrap_or_else(|_| "tesseract".to_string()),
            tessdata: std::env::var_os(ENV_TESSDATA).map(PathBuf::from),
            psm: DEFAULT_PSM,
        }
    }
}

impl TesseractCli {
    /// A new adapter honoring the `OXIDE_TESSERACT` / `OXIDE_TESSDATA`
    /// environment overrides (same as [`TesseractCli::default`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the `tesseract` binary path (e.g. an absolute path, or a
    /// nonexistent path in the "engine absent" tests).
    #[must_use]
    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }

    /// Overrides the tessdata (language-data) directory.
    #[must_use]
    pub fn with_tessdata(mut self, dir: impl Into<PathBuf>) -> Self {
        self.tessdata = Some(dir.into());
        self
    }

    /// Overrides the page-segmentation mode (`--psm`).
    #[must_use]
    pub fn with_psm(mut self, psm: u32) -> Self {
        self.psm = psm;
        self
    }

    /// Whether the configured `tesseract` binary is runnable (`tesseract
    /// --version` exits successfully). Cheap probe used to skip OCR tests on a
    /// machine without Tesseract; never errors.
    #[must_use]
    pub fn is_available(&self) -> bool {
        Command::new(&self.binary)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Runs `tesseract` on `png_path`, returning its TSV stdout.
    fn run_tsv(&self, png_path: &std::path::Path, lang: &str) -> Result<String> {
        let mut cmd = Command::new(&self.binary);
        cmd.arg(png_path)
            .arg("stdout")
            .arg("--psm")
            .arg(self.psm.to_string())
            .arg("-l")
            .arg(lang)
            .arg("tsv");
        if let Some(dir) = &self.tessdata {
            cmd.env("TESSDATA_PREFIX", dir);
        }
        let output = cmd.output().map_err(|e| {
            Error::Unsupported(format!(
                "tesseract not available (binary {:?}): {e}",
                self.binary
            ))
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Unsupported(format!(
                "tesseract failed ({}): {}",
                output.status,
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Parses Tesseract TSV (`tesseract … tsv`) into per-word boxes in pixel space.
///
/// The TSV has a header row whose columns are
/// `level page_num block_num par_num line_num word_num left top width height
/// conf text`. Word rows are `level == 5`. Rows with a non-positive confidence,
/// an empty/whitespace token, or a zero-area box are skipped (Tesseract emits a
/// `conf == -1` row for every non-leaf layout node).
fn parse_tsv(tsv: &str) -> Vec<OcrWord> {
    let mut words = Vec::new();
    let mut lines = tsv.lines();
    let Some(header) = lines.next() else {
        return words;
    };
    // Resolve column indices from the header so the parser tolerates a future
    // column reordering / addition.
    let cols: Vec<&str> = header.split('\t').collect();
    let idx = |name: &str| cols.iter().position(|c| *c == name);
    let (
        Some(i_level),
        Some(i_left),
        Some(i_top),
        Some(i_w),
        Some(i_h),
        Some(i_conf),
        Some(i_text),
    ) = (
        idx("level"),
        idx("left"),
        idx("top"),
        idx("width"),
        idx("height"),
        idx("conf"),
        idx("text"),
    )
    else {
        return words;
    };

    for line in lines {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        // `text` may contain tabs in pathological cases; require at least the
        // text column to exist and treat the remainder as the token.
        if f.len() <= i_text {
            continue;
        }
        if f.get(i_level).copied() != Some("5") {
            continue; // not a word-level row
        }
        let conf: f32 = f[i_conf].trim().parse().unwrap_or(-1.0);
        if conf < 0.0 {
            continue;
        }
        let text = f[i_text..].join("\t");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let (Ok(left), Ok(top), Ok(w), Ok(h)) = (
            f[i_left].trim().parse::<f64>(),
            f[i_top].trim().parse::<f64>(),
            f[i_w].trim().parse::<f64>(),
            f[i_h].trim().parse::<f64>(),
        ) else {
            continue;
        };
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        words.push(OcrWord {
            text: text.to_string(),
            bbox: Rect::new(left, top, left + w, top + h),
            confidence: conf,
        });
    }
    words
}

/// An RAII temp PNG: writes the bytes on construction, deletes the file on drop
/// (so a recognition error or panic still cleans up).
struct TempPng {
    path: PathBuf,
}

impl TempPng {
    fn write(png: &[u8]) -> Result<Self> {
        // A unique-enough name in the OS temp dir: pid + a process-monotonic
        // counter. No external `tempfile` dep (pure-std, cross-platform).
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("pdfspine_ocr_{}_{}.png", std::process::id(), n));
        let mut file = std::fs::File::create(&path)?;
        file.write_all(png)?;
        file.flush()?;
        Ok(TempPng { path })
    }
}

impl Drop for TempPng {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl OcrEngine for TesseractCli {
    fn recognize(&self, image: &Pixmap, lang: &str, _dpi: f32) -> Result<Vec<OcrWord>> {
        let png = image.to_png_bytes()?;
        let temp = TempPng::write(&png)?;
        let tsv = self.run_tsv(&temp.path, lang)?;
        Ok(parse_tsv(&tsv))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tsv_extracts_words_skips_noise() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   1\t1\t0\t0\t0\t0\t0\t0\t900\t200\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t37\t56\t284\t67\t94.9\tHELLO\n\
                   5\t1\t1\t1\t1\t2\t354\t56\t195\t67\t95.7\tOCR\n\
                   5\t1\t1\t1\t1\t3\t576\t56\t324\t67\t-1\t \n\
                   5\t1\t1\t1\t1\t4\t10\t10\t0\t0\t80\tZERO";
        let words = parse_tsv(tsv);
        assert_eq!(words.len(), 2, "only the two real word rows survive");
        assert_eq!(words[0].text, "HELLO");
        assert_eq!(words[0].bbox, Rect::new(37.0, 56.0, 321.0, 123.0));
        assert!((words[0].confidence - 94.9).abs() < 1e-3);
        assert_eq!(words[1].text, "OCR");
    }

    #[test]
    fn missing_binary_is_typed_unsupported_not_panic() {
        let eng = TesseractCli::new().with_binary("/nonexistent/tesseract-xyz");
        assert!(!eng.is_available());
        let pix = Pixmap::blank(8, 8, pdf_image::pixmap::Colorspace::Rgb, false, 255).unwrap();
        let err = eng.recognize(&pix, "eng", 72.0).unwrap_err();
        assert_eq!(err.kind(), "unsupported");
    }
}
