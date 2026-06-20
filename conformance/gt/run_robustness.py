#!/usr/bin/env python3
"""Never-panic robustness runner for pdfspine over the GovDocs1/SafeDocs corpus.

Fetch the corpus first (gitignored, regenerable)::

    env -u CONDA_PREFIX python conformance/gt/fetch_robustness.py \
        --out conformance/gt/corpus-robustness --n 250

Then run the never-panic check (from repo root)::

    env -u CONDA_PREFIX python conformance/gt/run_robustness.py

Each PDF's ``open`` + per-page ``get_text("text")`` runs in an **isolated
subprocess** (``conformance/pdfspine_worker.py``) under a wall-clock timeout, so
a Rust panic/abort or hang on one input is detected (non-zero exit / SIGABRT /
timeout) and flagged rather than crashing the runner. The exit status is
non-zero iff any input panics/aborts/times out, so this doubles as a CI-style
never-panic gate. The corpus PDFs are NEVER committed (``corpus-*`` gitignored);
only the refreshed ``ROBUSTNESS-REPORT.md`` is.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKER = REPO_ROOT / "conformance" / "pdfspine_worker.py"
DEFAULT_CORPUS = REPO_ROOT / "conformance" / "gt" / "corpus-robustness"
DEFAULT_REPORT = REPO_ROOT / "conformance" / "gt" / "ROBUSTNESS-REPORT.md"


def run_one(python: str, pdf: Path, timeout: float) -> dict:
    """Open+extract one PDF in an isolated subprocess. Returns a record dict."""
    rec: dict = {
        "file": pdf.name,
        "size": pdf.stat().st_size,
        "robustness": "ok",
        "opened": False,
        "page_count": None,
        "error": None,
    }
    try:
        proc = subprocess.run(
            [python, str(WORKER), str(pdf)],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        rec["robustness"] = "timeout"
        rec["error"] = f"timeout after {timeout}s"
        return rec

    if proc.returncode != 0:
        # A non-zero exit (incl. SIGABRT -> negative code) means a Rust
        # panic/abort escaped the worker's own exception handling.
        rec["robustness"] = "abort"
        rec["error"] = f"exit {proc.returncode}: {(proc.stderr or '')[:200]}"
        return rec

    try:
        data = json.loads(proc.stdout)
    except json.JSONDecodeError:
        rec["robustness"] = "abort"
        rec["error"] = f"unparseable worker output: {proc.stdout[:200]!r}"
        return rec

    rec["opened"] = bool(data.get("opened"))
    rec["page_count"] = data.get("page_count")
    rec["error"] = data.get("error")
    return rec


def write_report(path: Path, records: list[dict], python: str, timeout: float) -> None:
    total = len(records)
    aborts = [r for r in records if r["robustness"] == "abort"]
    timeouts = [r for r in records if r["robustness"] == "timeout"]
    opened = sum(1 for r in records if r["opened"])
    total_bytes = sum(r["size"] for r in records)
    sources = Counter(r["file"].split("-")[0] for r in records)
    src_str = ", ".join(f"{k} ({v})" for k, v in sorted(sources.items()))
    ts = datetime.now(timezone.utc).isoformat()

    lines: list[str] = []
    a = lines.append
    a("# pdfspine — Never-Panic Robustness Report")
    a("")
    a(f"_Generated: {ts}_")
    a("")
    a(
        "Each PDF's `open` + per-page `get_text(\"text\")` runs in an **isolated "
        "subprocess** under a wall-clock timeout (`conformance/pdfspine_worker.py`), "
        "so a Rust panic/abort or hang on one input is detected (non-zero exit / "
        "SIGABRT / timeout) and flagged rather than crashing the runner. The corpus "
        "is fetch-only and **never committed** (`conformance/gt/corpus-*/` is "
        "gitignored, regenerable via `fetch_robustness.py`); only this report is."
    )
    a("")
    a("## 1. Corpus")
    a("")
    a(f"- **{total}** PDFs, {total_bytes / 1e6:.1f} MB total — sources: {src_str}.")
    a(f"- Per-file timeout: {timeout:.0f}s.")
    a("")
    a("## 2. Never-panic / Robustness")
    a("")
    if not aborts and not timeouts:
        a(
            f"- **No panics, no aborts, no hangs across all {total} inputs.** Every "
            "open+extract exited cleanly (exit 0) in its isolated subprocess."
        )
    else:
        a(f"- Panics/aborts: **{len(aborts)}**, Timeouts/hangs: **{len(timeouts)}**.")
        for r in aborts + timeouts:
            a(f"  - `{r['file']}` — {r['robustness']}: {r['error']}")
    a("")
    a("## 3. Open rate")
    a("")
    a(f"- Opened: **{opened}/{total} ({100.0 * opened / total:.1f}%)**.")
    a("")
    a("---")
    a("")
    a(
        "_Methodology: never-panic only (no oracle differential here). A panic/abort "
        "surfaces as a non-zero subprocess exit; a hang as a timeout. Run "
        "`conformance/gt/run_robustness.py` from repo root after fetching the corpus._"
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS, help="robustness corpus dir")
    ap.add_argument("--python", default=sys.executable, help="project venv python (with built wheel)")
    ap.add_argument("--timeout", type=float, default=120.0, help="per-PDF wall-clock timeout (s)")
    ap.add_argument("--report-out", type=Path, default=DEFAULT_REPORT)
    args = ap.parse_args(argv)

    pdfs = sorted(args.corpus.glob("*.pdf"))
    if not pdfs:
        print(f"ERROR: no PDFs in {args.corpus} (fetch via fetch_robustness.py)", file=sys.stderr)
        return 1

    print(f"Never-panic check over {len(pdfs)} PDFs in {args.corpus}", flush=True)
    records: list[dict] = []
    for i, pdf in enumerate(pdfs, 1):
        rec = run_one(args.python, pdf, args.timeout)
        flag = "" if rec["robustness"] == "ok" else f"  <<< {rec['robustness'].upper()}"
        print(f"[{i}/{len(pdfs)}] {pdf.name} -> {rec['robustness']}{flag}", flush=True)
        records.append(rec)

    write_report(args.report_out, records, args.python, args.timeout)
    aborts = sum(1 for r in records if r["robustness"] == "abort")
    timeouts = sum(1 for r in records if r["robustness"] == "timeout")
    print("\n==================== SUMMARY ====================")
    print(f"Total PDFs : {len(records)}")
    print(f"Never-panic: aborts={aborts} timeouts={timeouts}")
    print(f"Report     : {args.report_out}")
    print("================================================")
    return 1 if (aborts or timeouts) else 0


if __name__ == "__main__":
    sys.exit(main())
