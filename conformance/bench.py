#!/usr/bin/env python3
"""Performance benchmark harness: pdfspine vs pypdf vs pypdfium2.

Times three operations over the public-domain corpus in ``fixtures/corpus/*.pdf``
for each PDF library and emits an honest comparison table to
``conformance/BENCH.md``.

Operations (warm, median of N runs per document):

* ``open``   -- open the file and read ``page_count``.
* ``text``   -- extract text over the *whole* document.
* ``render`` -- rasterize page 1 at 150 dpi (pypdf has no renderer -> skipped).

Architecture (mirrors ``oracle_extract.py`` / ``pdfspine_worker.py``):

The competitors (pypdf MIT, pypdfium2 BSD/Apache) are bench-only and live in a
separate, gitignored ``.venv-bench``. They must never be imported into the same
interpreter as our Apache-2.0 build, so every measurement runs in a SUBPROCESS:

* ``pdfspine``     -> the current project interpreter (``sys.executable``, in .venv)
* ``pypdf``     -> ``.venv-bench/bin/python``
* ``pypdfium2`` -> ``.venv-bench/bin/python``

Subprocess isolation also means a crash/hang/abort on one document for one
library is contained: the parent applies a wall-clock timeout and records that
single (library, document) cell as a failure instead of dying.

Run from the repo ROOT::

    env -u CONDA_PREFIX .venv/bin/python conformance/bench.py

Internal re-exec (do not call directly)::

    python conformance/bench.py --worker <lib> <pdf> [--runs N] [--dpi D]
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORPUS_DIR = ROOT / "fixtures" / "corpus"
BENCH_VENV = ROOT / ".venv-bench"
OUT_MD = ROOT / "conformance" / "BENCH.md"
OUT_JSON = ROOT / "conformance" / "bench-results.json"

LIBS = ("pdfspine", "pypdf", "pypdfium2")
OPS = ("open", "text", "render")
DEFAULT_RUNS = 5
DEFAULT_DPI = 150
# Generous per-document, per-library wall-clock budget for one worker process
# (covers all ops + warmup). A timeout marks that one cell as failed.
WORKER_TIMEOUT_S = 300.0


# --------------------------------------------------------------------------- #
# Worker side: runs INSIDE the appropriate interpreter for one library.
# --------------------------------------------------------------------------- #
def _median_time(fn, runs: int) -> tuple[float, object]:
    """Run ``fn`` once to warm caches, then ``runs`` timed reps; return median."""
    result = fn()  # warmup (also validates the op works at all)
    samples: list[float] = []
    for _ in range(runs):
        t0 = time.perf_counter()
        result = fn()
        samples.append(time.perf_counter() - t0)
    return statistics.median(samples), result


def _bench_pdfspine(path: str, runs: int, dpi: int) -> dict:
    import pdfspine

    out: dict = {"ops": {}, "page_count": None, "text_chars": None}

    def _open():
        doc = pdfspine.open(path)
        n = doc.page_count
        doc.close()
        return n

    t_open, n = _median_time(_open, runs)
    out["ops"]["open"] = t_open
    out["page_count"] = int(n)

    def _text():
        doc = pdfspine.open(path)
        try:
            if getattr(doc, "needs_pass", False):
                try:
                    doc.authenticate("")
                except Exception:  # noqa: BLE001
                    pass
            chars = 0
            for i in range(doc.page_count):
                chars += len(doc.load_page(i).get_text())
            return chars
        finally:
            doc.close()

    t_text, chars = _median_time(_text, runs)
    out["ops"]["text"] = t_text
    out["text_chars"] = int(chars)

    def _render():
        doc = pdfspine.open(path)
        try:
            pm = doc.load_page(0).get_pixmap(dpi=dpi)
            return (pm.width, pm.height)
        finally:
            doc.close()

    t_render, dims = _median_time(_render, runs)
    out["ops"]["render"] = t_render
    out["render_dims"] = list(dims)
    return out


def _bench_pypdf(path: str, runs: int, dpi: int) -> dict:
    from pypdf import PdfReader

    out: dict = {"ops": {}, "page_count": None, "text_chars": None}

    def _open():
        reader = PdfReader(path)
        return len(reader.pages)

    t_open, n = _median_time(_open, runs)
    out["ops"]["open"] = t_open
    out["page_count"] = int(n)

    def _text():
        reader = PdfReader(path)
        if reader.is_encrypted:
            try:
                reader.decrypt("")
            except Exception:  # noqa: BLE001
                pass
        chars = 0
        for page in reader.pages:
            chars += len(page.extract_text())
        return chars

    t_text, chars = _median_time(_text, runs)
    out["ops"]["text"] = t_text
    out["text_chars"] = int(chars)

    # pypdf is pure-Python and has no rasterizer.
    out["ops"]["render"] = None
    out["render_dims"] = None
    out["render_note"] = "unsupported (pypdf has no renderer)"
    return out


def _bench_pypdfium2(path: str, runs: int, dpi: int) -> dict:
    import pypdfium2 as pdfium

    out: dict = {"ops": {}, "page_count": None, "text_chars": None}

    def _open():
        pdf = pdfium.PdfDocument(path)
        n = len(pdf)
        pdf.close()
        return n

    t_open, n = _median_time(_open, runs)
    out["ops"]["open"] = t_open
    out["page_count"] = int(n)

    def _text():
        pdf = pdfium.PdfDocument(path)
        try:
            chars = 0
            for i in range(len(pdf)):
                page = pdf[i]
                textpage = page.get_textpage()
                chars += len(textpage.get_text_bounded())
                textpage.close()
                page.close()
            return chars
        finally:
            pdf.close()

    t_text, chars = _median_time(_text, runs)
    out["ops"]["text"] = t_text
    out["text_chars"] = int(chars)

    def _render():
        pdf = pdfium.PdfDocument(path)
        try:
            page = pdf[0]
            bitmap = page.render(scale=dpi / 72.0)
            dims = (bitmap.width, bitmap.height)
            bitmap.close()
            page.close()
            return dims
        finally:
            pdf.close()

    t_render, dims = _median_time(_render, runs)
    out["ops"]["render"] = t_render
    out["render_dims"] = list(dims)
    return out


_WORKERS = {
    "pdfspine": _bench_pdfspine,
    "pypdf": _bench_pypdf,
    "pypdfium2": _bench_pypdfium2,
}


def _run_worker(lib: str, path: str, runs: int, dpi: int) -> int:
    out: dict = {"lib": lib, "path": path, "ok": False, "error": None}
    try:
        out.update(_WORKERS[lib](path, runs, dpi))
        out["ok"] = True
    except Exception as exc:  # noqa: BLE001
        out["error"] = f"{type(exc).__name__}: {exc}"
    sys.stdout.write(json.dumps(out))
    return 0


# --------------------------------------------------------------------------- #
# Worker version probe (printed by the orchestrator into BENCH.md).
# --------------------------------------------------------------------------- #
def _probe_versions() -> int:
    info: dict = {"python": platform.python_version()}
    try:
        import pdfspine

        try:
            info["pdfspine"] = pdfspine.version()
        except Exception:  # noqa: BLE001
            info["pdfspine"] = "0.0.0"
    except Exception:  # noqa: BLE001
        pass
    import importlib.metadata as _md

    try:
        import pypdf  # noqa: F401

        info["pypdf"] = _md.version("pypdf")
    except Exception:  # noqa: BLE001
        pass
    try:
        import pypdfium2 as pdfium

        info["pypdfium2"] = _md.version("pypdfium2")
        try:
            info["pdfium"] = str(pdfium.version.PDFIUM_INFO)  # core C engine build
        except Exception:  # noqa: BLE001
            pass
    except Exception:  # noqa: BLE001
        pass
    sys.stdout.write(json.dumps(info))
    return 0


# --------------------------------------------------------------------------- #
# Orchestrator side.
# --------------------------------------------------------------------------- #
def _interpreter_for(lib: str) -> str:
    if lib == "pdfspine":
        return sys.executable
    bench_py = BENCH_VENV / "bin" / "python"
    if not bench_py.exists():
        bench_py = BENCH_VENV / "Scripts" / "python.exe"  # Windows
    return str(bench_py)


def _clean_env() -> dict:
    env = dict(os.environ)
    env.pop("CONDA_PREFIX", None)
    return env


def _measure(lib: str, pdf: Path, runs: int, dpi: int) -> dict:
    cmd = [
        _interpreter_for(lib),
        str(Path(__file__).resolve()),
        "--worker",
        lib,
        str(pdf),
        "--runs",
        str(runs),
        "--dpi",
        str(dpi),
    ]
    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=WORKER_TIMEOUT_S,
            env=_clean_env(),
            cwd=str(ROOT),
        )
    except subprocess.TimeoutExpired:
        return {"lib": lib, "path": str(pdf), "ok": False, "error": "timeout"}
    if proc.returncode != 0:
        msg = (proc.stderr or "").strip().splitlines()
        tail = msg[-1] if msg else f"exit {proc.returncode}"
        return {"lib": lib, "path": str(pdf), "ok": False, "error": f"crash: {tail}"}
    try:
        return json.loads(proc.stdout)
    except Exception as exc:  # noqa: BLE001
        return {"lib": lib, "path": str(pdf), "ok": False, "error": f"bad-json: {exc}"}


def _probe_bench_versions() -> dict:
    info: dict = {}
    for interp in (sys.executable, _interpreter_for("pypdf")):
        cmd = [interp, str(Path(__file__).resolve()), "--versions"]
        try:
            proc = subprocess.run(
                cmd, capture_output=True, text=True, timeout=60, env=_clean_env()
            )
            info.update(json.loads(proc.stdout))
        except Exception:  # noqa: BLE001
            continue
    return info


def _ratio_text(pdfspine_v: float | None, other_v: float | None) -> str:
    """Return 'Nx faster'/'Nx slower' describing pdfspine relative to ``other``."""
    if not pdfspine_v or not other_v:
        return "n/a"
    if pdfspine_v <= other_v:
        return f"{other_v / pdfspine_v:.2f}x faster"
    return f"{pdfspine_v / other_v:.2f}x slower"


def _fmt_sec(v: float | None) -> str:
    if v is None:
        return "n/a"
    if v < 1e-3:
        return f"{v * 1e6:.0f} us"
    if v < 1.0:
        return f"{v * 1e3:.2f} ms"
    return f"{v:.3f} s"


def run(runs: int, dpi: int, limit: int | None) -> dict:
    pdfs = sorted(CORPUS_DIR.glob("*.pdf"))
    if limit:
        pdfs = pdfs[:limit]
    if not pdfs:
        raise SystemExit(f"no PDFs found under {CORPUS_DIR}")

    bench_py = _interpreter_for("pypdf")
    if not Path(bench_py).exists():
        raise SystemExit(
            f"bench venv interpreter not found: {bench_py}\n"
            "Create it with:\n"
            "  env -u CONDA_PREFIX <python3.x> -m venv .venv-bench\n"
            "  env -u CONDA_PREFIX .venv-bench/bin/python -m pip install pypdf pypdfium2"
        )

    versions = _probe_bench_versions()
    brand = _cpu_brand()
    machine = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": brand or platform.processor() or "unknown",
        "cpu_count": os.cpu_count(),
        "python_orchestrator": platform.python_version(),
    }

    per_doc: dict[str, dict] = {}
    for pdf in pdfs:
        name = pdf.name
        per_doc[name] = {}
        for lib in LIBS:
            res = _measure(lib, pdf, runs, dpi)
            per_doc[name][lib] = res
            status = "ok" if res.get("ok") else f"FAIL ({res.get('error')})"
            sys.stderr.write(f"  {name:<28} {lib:<10} {status}\n")
        sys.stderr.write(f"[done] {name}\n")

    return {
        "machine": machine,
        "versions": versions,
        "config": {"runs": runs, "dpi": dpi, "corpus": len(pdfs)},
        "per_doc": per_doc,
    }


def _cpu_brand() -> str:
    """macOS CPU brand via sysctl; empty string elsewhere (let caller fall back)."""
    if sys.platform != "darwin":
        return ""
    try:
        out = subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        return out.stdout.strip()
    except Exception:  # noqa: BLE001
        return ""


# --------------------------------------------------------------------------- #
# Aggregation + report.
# --------------------------------------------------------------------------- #
def _aggregate(data: dict) -> dict:
    """Per-op, per-lib: list of per-doc medians where the cell succeeded."""
    agg: dict[str, dict[str, list[float]]] = {op: {lib: [] for lib in LIBS} for op in OPS}
    docs_failed: dict[str, list[str]] = {lib: [] for lib in LIBS}
    for name, libs in data["per_doc"].items():
        for lib in LIBS:
            res = libs.get(lib, {})
            if not res.get("ok"):
                docs_failed[lib].append(name)
                continue
            for op in OPS:
                v = res.get("ops", {}).get(op)
                if v is not None:
                    agg[op][lib].append(v)
    return {"per_op": agg, "docs_failed": docs_failed}


def _median(values: list[float]) -> float | None:
    return statistics.median(values) if values else None


def write_report(data: dict) -> None:
    agg = _aggregate(data)
    per_op = agg["per_op"]
    docs_failed = agg["docs_failed"]
    cfg = data["config"]
    ver = data["versions"]
    mach = data["machine"]

    # Per-op median-of-per-doc-medians.
    op_med: dict[str, dict[str, float | None]] = {
        op: {lib: _median(per_op[op][lib]) for lib in LIBS} for op in OPS
    }

    lines: list[str] = []
    lines.append("# pdfspine performance benchmark")
    lines.append("")
    lines.append(
        "Honest, reproducible comparison of **pdfspine** against two popular "
        "Python PDF libraries on a shared public-domain corpus. Generated by "
        "`conformance/bench.py`."
    )
    lines.append("")

    # ---- Summary headline -------------------------------------------------- #
    lines.append("## Summary")
    lines.append("")
    summary_bits: list[str] = []
    for op, label in (("open", "open"), ("text", "text extraction"), ("render", "render @150dpi")):
        ox = op_med[op]["pdfspine"]
        pf = op_med[op]["pypdf"]
        pm = op_med[op]["pypdfium2"]
        parts = []
        if ox is not None and pf is not None:
            parts.append(f"vs pypdf: pdfspine {_ratio_text(ox, pf)}")
        if ox is not None and pm is not None:
            parts.append(f"vs pypdfium2: pdfspine {_ratio_text(ox, pm)}")
        if op == "render" and pf is None:
            parts.append("pypdf: unsupported")
        if parts:
            summary_bits.append(f"- **{label}** — " + "; ".join(parts) + ".")
    lines.extend(summary_bits)
    lines.append("")
    lines.append(
        "Read the ratios as *pdfspine relative to the competitor*: \"faster\" means "
        "pdfspine took less wall-clock time per document. All numbers are warm "
        "medians (see Methodology)."
    )
    lines.append("")

    # ---- Environment ------------------------------------------------------- #
    lines.append("## Environment")
    lines.append("")
    lines.append(f"- **CPU**: {mach.get('processor', 'unknown')} ({mach.get('cpu_count')} cores)")
    lines.append(f"- **OS / arch**: {mach.get('platform')} / {mach.get('machine')}")
    lines.append(f"- **Python (orchestrator/pdfspine)**: {mach.get('python_orchestrator')}")
    if "python" in ver:
        lines.append(f"- **Python (.venv-bench / competitors)**: {ver.get('python')}")
    lines.append("")
    lines.append("Library versions:")
    lines.append("")
    lines.append(f"- **pdfspine**: {ver.get('pdfspine', 'unknown')} (pure Rust, Apache-2.0)")
    lines.append(f"- **pypdf**: {ver.get('pypdf', 'unknown')} (pure Python, MIT)")
    pdfium_core = f", PDFium {ver['pdfium']}" if "pdfium" in ver else ""
    lines.append(
        f"- **pypdfium2**: {ver.get('pypdfium2', 'unknown')} "
        f"(C-engine binding, BSD-3/Apache-2.0{pdfium_core})"
    )
    lines.append("")
    lines.append(
        f"Corpus: **{cfg['corpus']}** public-domain PDFs in `fixtures/corpus/` "
        "(US gov / IRS / NASA / NIST / USGS / CDC documents)."
    )
    lines.append("")

    # ---- Main table -------------------------------------------------------- #
    lines.append("## Results (median seconds per document)")
    lines.append("")
    lines.append(
        "| Operation | pdfspine | pypdf | pypdfium2 | pdfspine vs pypdf | pdfspine vs pypdfium2 |"
    )
    lines.append("|---|---|---|---|---|---|")
    op_titles = {
        "open": "open + page_count",
        "text": "get_text (whole doc)",
        "render": f"render page 1 @ {cfg['dpi']}dpi",
    }
    for op in OPS:
        ox = op_med[op]["pdfspine"]
        pf = op_med[op]["pypdf"]
        pm = op_med[op]["pypdfium2"]
        pf_cell = _fmt_sec(pf) if pf is not None else "n/a (unsupported)"
        lines.append(
            f"| {op_titles[op]} | {_fmt_sec(ox)} | {pf_cell} | {_fmt_sec(pm)} "
            f"| pdfspine {_ratio_text(ox, pf)} | pdfspine {_ratio_text(ox, pm)} |"
        )
    lines.append("")
    lines.append(
        "*Cell = median across the per-document medians (each document timed as "
        f"the median of {cfg['runs']} warm runs). \"render\" is page 1 only.*"
    )
    lines.append("")

    # ---- Per-op totals ----------------------------------------------------- #
    lines.append("## Totals across corpus (sum of per-doc medians)")
    lines.append("")
    lines.append("| Operation | pdfspine | pypdf | pypdfium2 | #docs (pdfspine) |")
    lines.append("|---|---|---|---|---|")
    for op in OPS:
        sums = {lib: (sum(per_op[op][lib]) if per_op[op][lib] else None) for lib in LIBS}
        n_ox = len(per_op[op]["pdfspine"])
        lines.append(
            f"| {op_titles[op]} | {_fmt_sec(sums['pdfspine'])} | "
            f"{_fmt_sec(sums['pypdf']) if sums['pypdf'] is not None else 'n/a'} | "
            f"{_fmt_sec(sums['pypdfium2'])} | {n_ox} |"
        )
    lines.append("")

    # ---- Failures ---------------------------------------------------------- #
    lines.append("## Documents a library failed to process")
    lines.append("")
    any_fail = any(docs_failed[lib] for lib in LIBS)
    if not any_fail:
        lines.append("None — every library opened and processed all corpus documents.")
    else:
        for lib in LIBS:
            failed = docs_failed[lib]
            if failed:
                lines.append(f"- **{lib}**: {len(failed)} failed:")
                for name in failed:
                    res = data["per_doc"][name][lib]
                    lines.append(f"  - `{name}` — {res.get('error')}")
            else:
                lines.append(f"- **{lib}**: none")
    lines.append("")

    # ---- Methodology ------------------------------------------------------- #
    lines.append("## Methodology")
    lines.append("")
    lines.append(
        f"- **Warm medians.** Each (library, document, operation) is run once to "
        f"warm OS/file caches and validate the op, then timed {cfg['runs']} times; "
        "the reported per-document number is the median of those timed runs."
    )
    lines.append(
        "- **Subprocess isolation.** Every measurement runs in a fresh worker "
        "process via the correct interpreter — pdfspine in the project `.venv`, "
        "pypdf/pypdfium2 in a separate gitignored `.venv-bench`. This keeps the "
        "AGPL/3rd-party deps out of our build's interpreter and contains "
        "crashes/hangs (a per-document wall-clock timeout marks a cell failed)."
    )
    lines.append(
        "- **Operations.** `open` = open + read `page_count`; `text` = extract "
        "text over the *whole* document (pdfspine `page.get_text()`, pypdf "
        "`page.extract_text()`, pypdfium2 `textpage.get_text_bounded()`); "
        f"`render` = rasterize page 1 at {cfg['dpi']} dpi (pdfspine "
        f"`page.get_pixmap(dpi={cfg['dpi']})`, pypdfium2 "
        f"`page.render(scale={cfg['dpi']}/72)`)."
    )
    lines.append(
        "- **Process-creation overhead** is excluded: workers are spawned once "
        "per (library, document) and time only the in-process op loop, never the "
        "interpreter startup."
    )
    lines.append("")

    # ---- Caveats ----------------------------------------------------------- #
    lines.append("## Caveats (read these)")
    lines.append("")
    lines.append(
        "- **Apples to different things.** pypdf is **pure Python** (no native "
        "code, no rasterizer); pypdfium2 wraps **PDFium**, the mature C/C++ "
        "engine from Chromium. pdfspine is **pure Rust**. A pure-Rust library "
        "beating a hand-tuned C engine is not the expectation — where pypdfium2 "
        "wins, that is the C engine's maturity showing."
    )
    lines.append(
        "- **pdfspine's rasterizer is young.** The `render` path is a from-scratch "
        "Rust rasterizer; treat its render numbers as a snapshot of an evolving "
        "component, not a final state."
    )
    lines.append(
        "- **Text extraction is not normalized for accuracy here.** This harness "
        "measures *speed*, not output quality. Extraction accuracy vs the fitz "
        "oracle is tracked separately in `conformance/REPORT.md`."
    )
    lines.append(
        "- **Single machine, single run.** Numbers are from one Apple-silicon "
        "laptop and will differ on other hardware; re-run `bench.py` to "
        "reproduce locally."
    )
    lines.append("")

    OUT_MD.write_text("\n".join(lines), encoding="utf-8")


# --------------------------------------------------------------------------- #
# Entry point.
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--worker", nargs=2, metavar=("LIB", "PDF"), default=None,
                    help="internal: run one library on one PDF and print JSON")
    ap.add_argument("--versions", action="store_true",
                    help="internal: print available library versions as JSON")
    ap.add_argument("--runs", type=int, default=DEFAULT_RUNS,
                    help=f"timed runs per document (default {DEFAULT_RUNS})")
    ap.add_argument("--dpi", type=int, default=DEFAULT_DPI,
                    help=f"render DPI (default {DEFAULT_DPI})")
    ap.add_argument("--limit", type=int, default=None,
                    help="only process the first N corpus PDFs (smoke test)")
    args = ap.parse_args(argv)

    if args.versions:
        return _probe_versions()
    if args.worker is not None:
        lib, pdf = args.worker
        if lib not in _WORKERS:
            sys.stdout.write(json.dumps({"ok": False, "error": f"unknown lib {lib}"}))
            return 0
        return _run_worker(lib, pdf, args.runs, args.dpi)

    sys.stderr.write(
        f"Benchmarking {len(LIBS)} libs x corpus "
        f"(runs={args.runs}, dpi={args.dpi})...\n"
    )
    data = run(args.runs, args.dpi, args.limit)
    OUT_JSON.write_text(json.dumps(data, indent=2), encoding="utf-8")
    write_report(data)
    sys.stderr.write(f"\nWrote {OUT_MD}\n")
    sys.stderr.write(f"Wrote {OUT_JSON} (gitignored)\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
