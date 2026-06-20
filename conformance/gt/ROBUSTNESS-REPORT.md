# pdfspine — Never-Panic Robustness Report

_Generated: 2026-06-20T01:30:35.876263+00:00_

Each PDF's `open` + per-page `get_text("text")` runs in an **isolated subprocess** under a wall-clock timeout (`conformance/pdfspine_worker.py`), so a Rust panic/abort or hang on one input is detected (non-zero exit / SIGABRT / timeout) and flagged rather than crashing the runner. The corpus is fetch-only and **never committed** (`conformance/gt/corpus-*/` is gitignored, regenerable via `fetch_robustness.py`); only this report is.

## 1. Corpus

- **43** PDFs, 36.3 MB total — sources: govdocs1 (43).
- Per-file timeout: 120s.

> **N shortfall (network-bound).** Target was 250 GovDocs1/SafeDocs PDFs, but the
> local proxy throttles/breaks TLS to the corpus hosts (the fetchers already strip
> proxy env via `ProxyHandler({})`), so `fetch_robustness.py` stalled after
> extracting the ≤8 MB members of GovDocs1 thread 0 (~40 files). Re-run
> `fetch_robustness.py --n 250` (optionally `--source safedocs`) on an unthrottled
> link to grow N, then re-run `run_robustness.py`. The never-panic result below is
> over the **43** PDFs reachable here.

## 2. Never-panic / Robustness

- **No panics, no aborts, no hangs across all 43 inputs.** Every open+extract exited cleanly (exit 0) in its isolated subprocess.

## 3. Open rate

- Opened: **43/43 (100.0%)**.

---

_Methodology: never-panic only (no oracle differential here). A panic/abort surfaces as a non-zero subprocess exit; a hang as a timeout. Run `conformance/gt/run_robustness.py` from repo root after fetching the corpus._
