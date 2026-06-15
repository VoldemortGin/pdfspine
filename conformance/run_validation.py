#!/usr/bin/env python3
"""Real-corpus validation harness for oxide-pdf.

Given a directory of PDFs, measures — honestly — how oxide_pdf behaves on
real-world files and how close its text extraction is to PyMuPDF (the "fitz"
reference) and pdfminer.six (secondary):

1. Open/repair rate     — oxide_pdf.open(path): opened-ok / repaired / failed.
2. Never-panic / robustness — each open+extract runs in an isolated subprocess
   under a wall-clock timeout; a Rust panic surfaces as a Python exception, and
   a Rust abort / hang surfaces as a non-zero subprocess exit or a timeout. Any
   such event is flagged (an ABORT is the serious one).
3. Structural validity  — for a sample, doc.save() -> ``qpdf --check`` pass rate.
4. Differential text accuracy — per page, oxide_pdf get_text("text") vs the
   oracles. Per-document similarity = normalized Levenshtein ratio AND token
   Jaccard, computed on whitespace-normalized text. Reports mean/median and the
   worst cases with a short reason.

Outputs ``conformance/results.json`` (machine-readable, gitignored) and
``conformance/REPORT.md`` (human-readable, committed).

Run from repo root::

    env -u CONDA_PREFIX .venv/bin/python conformance/run_validation.py \
        --corpus fixtures/corpus \
        --oracle-python .venv-oracle/bin/python \
        --qpdf-sample 12

License posture: PyMuPDF (AGPL) runs only as a local subprocess oracle; its
output is used for live scoring and is NEVER committed (no oracle text is written
into results.json or REPORT.md — only similarity scores and short structural
diff reasons derived from them).
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import statistics
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
CONFORMANCE = REPO_ROOT / "conformance"
WORKER = CONFORMANCE / "oxide_worker.py"
ORACLE = CONFORMANCE / "oracle_extract.py"


# --------------------------------------------------------------------------- #
# Text normalization + similarity
# --------------------------------------------------------------------------- #
_WS = re.compile(r"\s+")
_TOKEN = re.compile(r"\w+", re.UNICODE)


def normalize(text: str) -> str:
    """Collapse all whitespace runs to single spaces; strip. Lowercase-agnostic."""
    return _WS.sub(" ", text or "").strip()


# Token-list cap: bounds the sequence matcher's working set on huge documents.
_TOKEN_CAP = 60_000


def levenshtein_ratio(a: str, b: str) -> float:
    """Normalized sequence-similarity in [0,1] over whitespace tokens; 1.0 == identical.

    Uses difflib.SequenceMatcher on the *token lists* (not raw characters). This
    is a true normalized edit-similarity (matching-block based, symmetric) that
    is fast even on large inputs because SequenceMatcher exploits shared blocks —
    unlike a character-level match, which hung on a 4 MB document. Token lists are
    capped at ``_TOKEN_CAP`` as a hard ceiling.
    """
    import difflib

    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    ta = a.split()[:_TOKEN_CAP]
    tb = b.split()[:_TOKEN_CAP]
    if not ta and not tb:
        return 1.0
    if not ta or not tb:
        return 0.0
    return difflib.SequenceMatcher(None, ta, tb, autojunk=False).ratio()


def jaccard(a: str, b: str) -> float:
    """Token (word) Jaccard similarity in [0,1], case-insensitive."""
    ta = set(_TOKEN.findall(a.lower()))
    tb = set(_TOKEN.findall(b.lower()))
    if not ta and not tb:
        return 1.0
    if not ta or not tb:
        return 0.0
    return len(ta & tb) / len(ta | tb)


def diff_reason(ours: str, theirs: str) -> str:
    """Heuristic, content-free reason for a low similarity score.

    Deliberately does NOT echo oracle text — only structural observations.
    """
    on, tn = normalize(ours), normalize(theirs)
    olen, tlen = len(on), len(tn)
    if tlen == 0 and olen == 0:
        return "both extracted empty text"
    if tlen == 0:
        return "oracle empty, oxide non-empty (oracle could not extract)"
    if olen == 0:
        return "oxide extracted empty text while oracle got content (likely scanned/image-only or unsupported font program)"
    ratio = olen / tlen if tlen else 0.0
    reasons: list[str] = []
    if ratio < 0.5:
        reasons.append(f"oxide text much shorter ({olen} vs {tlen} chars, {ratio:.0%}) — missing content")
    elif ratio > 1.8:
        reasons.append(f"oxide text much longer ({olen} vs {tlen} chars) — duplicated/extra content")
    ot = set(_TOKEN.findall(on.lower()))
    tt = set(_TOKEN.findall(tn.lower()))
    only_theirs = len(tt - ot)
    only_ours = len(ot - tt)
    if tt:
        if only_theirs / len(tt) > 0.3:
            reasons.append(f"{only_theirs}/{len(tt)} oracle tokens absent from oxide (dropped glyphs/words)")
    if ot:
        if only_ours / len(ot) > 0.3:
            reasons.append(f"{only_ours}/{len(ot)} oxide tokens absent from oracle (spurious/mis-decoded)")
    # word-order / spacing: high jaccard but low levenshtein => layout/order diff
    lev = levenshtein_ratio(on, tn)
    jac = jaccard(on, tn)
    if jac - lev > 0.15:
        reasons.append("similar vocabulary but different ordering/spacing (reading-order or word-break difference)")
    if not reasons:
        reasons.append(f"moderate divergence (lev {lev:.2f}, jaccard {jac:.2f})")
    return "; ".join(reasons)


# --------------------------------------------------------------------------- #
# Subprocess runners
# --------------------------------------------------------------------------- #
def run_oxide(py: str, pdf: Path, timeout: float, save_path: Path | None) -> dict:
    """Run the isolated oxide worker. Returns a result dict with robustness flags."""
    cmd = [py, str(WORKER), str(pdf)]
    if save_path is not None:
        cmd += ["--save", str(save_path)]
    rec: dict = {"robustness": "ok", "exit_code": None, "raw": None}
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        rec.update(robustness="timeout", opened=False, error=f"timeout after {timeout}s")
        return rec
    rec["exit_code"] = proc.returncode
    if proc.returncode != 0:
        # Worker traps handled exceptions and exits 0; a non-zero exit means a
        # Rust panic/abort (e.g. SIGABRT -> negative code) escaped the interpreter.
        sig = -proc.returncode if proc.returncode < 0 else None
        rec.update(
            robustness="abort",
            opened=False,
            error=f"worker exited {proc.returncode}"
            + (f" (signal {sig})" if sig else "")
            + (f": {proc.stderr.strip()[:300]}" if proc.stderr.strip() else ""),
        )
        return rec
    try:
        data = json.loads(proc.stdout)
    except json.JSONDecodeError:
        rec.update(robustness="abort", opened=False, error=f"unparseable worker output: {proc.stdout[:200]!r}")
        return rec
    data["robustness"] = "ok"
    data["exit_code"] = 0
    return data


def run_oracle(oracle_py: str, pdf: Path, timeout: float) -> dict:
    """Run the oracle extractor in .venv-oracle. Returns {pymupdf:..., pdfminer:...}."""
    try:
        proc = subprocess.run(
            [oracle_py, str(ORACLE), str(pdf)],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return {"pymupdf": {"ok": False, "error": "timeout", "pages": []},
                "pdfminer": {"ok": False, "error": "timeout", "pages": []}}
    if proc.returncode != 0:
        return {"pymupdf": {"ok": False, "error": f"oracle exit {proc.returncode}", "pages": []},
                "pdfminer": {"ok": False, "error": f"oracle exit {proc.returncode}", "pages": []}}
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"pymupdf": {"ok": False, "error": "bad json", "pages": []},
                "pdfminer": {"ok": False, "error": "bad json", "pages": []}}


def qpdf_check(path: Path) -> tuple[bool, str]:
    """Return (passes, short-message) from ``qpdf --check``."""
    qpdf = shutil.which("qpdf")
    if not qpdf:
        return False, "qpdf not found"
    try:
        proc = subprocess.run([qpdf, "--check", str(path)], capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return False, "qpdf --check timeout"
    # qpdf exit: 0 = no errors/warnings; 3 = warnings only; 2 = errors.
    out = (proc.stdout + proc.stderr).strip()
    if proc.returncode in (0, 3):
        last = out.splitlines()[-1] if out else ""
        return True, ("clean" if proc.returncode == 0 else f"warnings: {last[:160]}")
    return False, (out.splitlines()[-1][:200] if out else f"qpdf exit {proc.returncode}")


# --------------------------------------------------------------------------- #
# Main
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--corpus", type=Path, action="append", required=True,
                    help="directory of PDFs (repeatable; first = Tier-1, others = Tier-2)")
    ap.add_argument("--python", default=sys.executable, help="project venv python (with built wheel)")
    ap.add_argument("--oracle-python", default=str(REPO_ROOT / ".venv-oracle" / "bin" / "python"))
    ap.add_argument("--timeout", type=float, default=120.0, help="per-PDF wall-clock timeout (s)")
    ap.add_argument("--qpdf-sample", type=int, default=12, help="how many opened PDFs to re-save+qpdf-check")
    ap.add_argument("--json-out", type=Path, default=CONFORMANCE / "results.json")
    ap.add_argument("--report-out", type=Path, default=CONFORMANCE / "REPORT.md")
    ap.add_argument("--tmp", type=Path, default=Path("/tmp/oxide-conformance"))
    args = ap.parse_args(argv)

    args.tmp.mkdir(parents=True, exist_ok=True)
    oracle_available = Path(args.oracle_python).exists()

    # Gather PDFs, tagging tier by which --corpus dir they came from.
    pdfs: list[tuple[Path, str]] = []
    for idx, d in enumerate(args.corpus):
        tier = "tier1" if idx == 0 else "tier2"
        if not d.exists():
            print(f"WARN corpus dir missing: {d}", file=sys.stderr)
            continue
        for p in sorted(d.glob("*.pdf")):
            pdfs.append((p, tier))

    if not pdfs:
        print("ERROR: no PDFs found in corpus dirs", file=sys.stderr)
        return 1

    print(f"Found {len(pdfs)} PDFs. Oracle available: {oracle_available}")
    records: list[dict] = []
    qpdf_done = 0

    for i, (pdf, tier) in enumerate(pdfs, 1):
        print(f"[{i}/{len(pdfs)}] {tier} {pdf.name}", flush=True)
        save_path: Path | None = None
        do_qpdf = qpdf_done < args.qpdf_sample
        if do_qpdf:
            save_path = args.tmp / (pdf.stem + ".resaved.pdf")

        ox = run_oxide(args.python, pdf, args.timeout, save_path if do_qpdf else None)

        rec: dict = {
            "file": pdf.name,
            "tier": tier,
            "size": pdf.stat().st_size,
            "robustness": ox.get("robustness", "ok"),
            "opened": bool(ox.get("opened")),
            "repaired": ox.get("repaired"),
            "page_count": ox.get("page_count"),
            "error": ox.get("error"),
            "qpdf": None,
            "sim": None,
        }

        # qpdf check on re-saved output (only when opened + saved ok)
        if do_qpdf:
            if ox.get("opened") and ox.get("save_ok") and save_path and save_path.exists():
                ok, msg = qpdf_check(save_path)
                rec["qpdf"] = {"checked": True, "pass": ok, "msg": msg}
                qpdf_done += 1
            else:
                rec["qpdf"] = {"checked": False, "pass": None,
                               "msg": "not opened/saved" if not ox.get("save_ok") else "no output"}

        # Differential text similarity (only for Tier-1 + opened + oracle present)
        if oracle_available and ox.get("opened"):
            oracle = run_oracle(args.oracle_python, pdf, args.timeout)
            our_text = normalize("\n".join(ox.get("pages") or []))
            sim: dict = {}
            for name in ("pymupdf", "pdfminer"):
                o = oracle.get(name, {})
                if o.get("ok"):
                    their_text = normalize("\n".join(o.get("pages") or []))
                    lev = levenshtein_ratio(our_text, their_text)
                    jac = jaccard(our_text, their_text)
                    sim[name] = {
                        "ok": True,
                        "levenshtein": round(lev, 4),
                        "jaccard": round(jac, 4),
                        "our_chars": len(our_text),
                        "their_chars": len(their_text),
                        "reason": diff_reason(our_text, their_text) if lev < 0.92 else "",
                    }
                else:
                    sim[name] = {"ok": False, "error": o.get("error", "oracle failed")}
            rec["sim"] = sim

        records.append(rec)

    summary = _summarize(records, oracle_available)
    payload = {
        "generated": datetime.now(timezone.utc).isoformat(),
        "corpus_dirs": [str(d) for d in args.corpus],
        "oracle_available": oracle_available,
        "qpdf_version": _qpdf_version(),
        "summary": summary,
        "records": records,
    }
    args.json_out.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(f"\nWrote {args.json_out}")

    report = _render_report(payload)
    args.report_out.write_text(report, encoding="utf-8")
    print(f"Wrote {args.report_out}")
    _print_console_summary(summary)
    return 0


def _qpdf_version() -> str:
    qpdf = shutil.which("qpdf")
    if not qpdf:
        return "n/a"
    try:
        out = subprocess.run([qpdf, "--version"], capture_output=True, text=True, timeout=10).stdout
        return out.splitlines()[0].strip()
    except Exception:  # noqa: BLE001
        return "n/a"


def _summarize(records: list[dict], oracle: bool) -> dict:
    total = len(records)
    opened = [r for r in records if r["opened"]]
    repaired = [r for r in opened if r.get("repaired")]
    failed = [r for r in records if not r["opened"]]
    aborts = [r for r in records if r["robustness"] == "abort"]
    timeouts = [r for r in records if r["robustness"] == "timeout"]

    qchecked = [r for r in records if r.get("qpdf") and r["qpdf"].get("checked")]
    qpass = [r for r in qchecked if r["qpdf"].get("pass")]

    def _scores(metric: str, oracle_name: str) -> list[float]:
        out = []
        for r in records:
            s = (r.get("sim") or {}).get(oracle_name)
            if s and s.get("ok"):
                out.append(s[metric])
        return out

    sim_summary: dict = {}
    if oracle:
        for oracle_name in ("pymupdf", "pdfminer"):
            lev = _scores("levenshtein", oracle_name)
            jac = _scores("jaccard", oracle_name)
            sim_summary[oracle_name] = {
                "n": len(lev),
                "levenshtein_mean": round(statistics.mean(lev), 4) if lev else None,
                "levenshtein_median": round(statistics.median(lev), 4) if lev else None,
                "jaccard_mean": round(statistics.mean(jac), 4) if jac else None,
                "jaccard_median": round(statistics.median(jac), 4) if jac else None,
                "ge_0_95": sum(1 for x in lev if x >= 0.95) if lev else 0,
                "ge_0_80": sum(1 for x in lev if x >= 0.80) if lev else 0,
                "lt_0_50": sum(1 for x in lev if x < 0.50) if lev else 0,
            }

    return {
        "total": total,
        "opened": len(opened),
        "open_rate": round(len(opened) / total, 4) if total else 0,
        "repaired": len(repaired),
        "failed": len(failed),
        "failed_files": [{"file": r["file"], "error": r["error"]} for r in failed],
        "aborts": len(aborts),
        "abort_files": [{"file": r["file"], "error": r["error"]} for r in aborts],
        "timeouts": len(timeouts),
        "timeout_files": [r["file"] for r in timeouts],
        "qpdf_checked": len(qchecked),
        "qpdf_pass": len(qpass),
        "qpdf_pass_rate": round(len(qpass) / len(qchecked), 4) if qchecked else None,
        "similarity": sim_summary,
    }


def _worst_cases(records: list[dict], oracle_name: str, k: int = 8) -> list[dict]:
    scored = []
    for r in records:
        s = (r.get("sim") or {}).get(oracle_name)
        if s and s.get("ok"):
            scored.append((s["levenshtein"], r, s))
    scored.sort(key=lambda t: t[0])
    out = []
    for lev, r, s in scored[:k]:
        out.append({
            "file": r["file"],
            "levenshtein": s["levenshtein"],
            "jaccard": s["jaccard"],
            "our_chars": s["our_chars"],
            "their_chars": s["their_chars"],
            "reason": s.get("reason") or "(close)",
        })
    return out


def _render_report(payload: dict) -> str:
    s = payload["summary"]
    L: list[str] = []
    a = L.append
    a("# oxide-pdf — Real-Corpus Validation Report")
    a("")
    a(f"_Generated: {payload['generated']} • qpdf: {payload['qpdf_version']} • "
      f"oracle (PyMuPDF/pdfminer) available: {payload['oracle_available']}_")
    a("")
    a("This is the project's first accuracy/robustness measurement on **real-world** "
      "PDFs (prior numbers used self-generated fixtures only). Oracles run locally as "
      "diff references only; **no PyMuPDF/oracle output is committed** — only similarity "
      "scores and content-free structural diff reasons.")
    a("")

    # Corpus composition
    a("## 1. Corpus")
    a("")
    by_tier: dict[str, list[dict]] = {}
    for r in payload["records"]:
        by_tier.setdefault(r["tier"], []).append(r)
    for tier in ("tier1", "tier2"):
        rs = by_tier.get(tier, [])
        if not rs:
            continue
        kind = "committable, public-domain" if tier == "tier1" else "fetch-only, NOT committed (CC BY-SA)"
        total_mb = sum(r["size"] for r in rs) / 1e6
        a(f"- **{tier}** ({kind}): {len(rs)} files, {total_mb:.1f} MB total")
    a("")
    a("Tier-1 provenance: all files are US-federal-government works (public domain, "
      "17 U.S.C. §105) from IRS, GovInfo, CDC MMWR, NASA NTRS, USGS, and NIST — each "
      "recorded in `fixtures/MANIFEST.toml` (source/license/sha256/cleared_by/cleared_date). "
      "Tier-2 (PDF Association `pdf20examples`, CC BY-SA 4.0) is used for robustness only.")
    a("")

    # Open / repair / fail
    a("## 2. Open / Repair / Fail rate")
    a("")
    a(f"- Opened: **{s['opened']}/{s['total']} ({s['open_rate']:.1%})**")
    a(f"- Reported as repaired: {s['repaired']}")
    a(f"- Failed to open: {s['failed']}")
    if s["failed_files"]:
        a("")
        a("Failures:")
        for f in s["failed_files"]:
            a(f"  - `{f['file']}` — {f['error']}")
    a("")

    # Robustness
    a("## 3. Never-panic / Robustness")
    a("")
    if s["aborts"] == 0 and s["timeouts"] == 0:
        a(f"- **No aborts, no panics, no hangs** across all {s['total']} inputs. "
          f"Every open+extract ran in an isolated subprocess under a wall-clock timeout; "
          f"all exited cleanly (exit 0).")
    else:
        a(f"- Aborts/panics: **{s['aborts']}**, Timeouts/hangs: **{s['timeouts']}**")
        for f in s["abort_files"]:
            a(f"  - ABORT `{f['file']}` — {f['error']}")
        for f in s["timeout_files"]:
            a(f"  - TIMEOUT `{f}`")
    a("")

    # qpdf
    a("## 4. Structural validity (qpdf --check on re-saved output)")
    a("")
    if s["qpdf_checked"]:
        rate = s["qpdf_pass_rate"]
        a(f"- Sampled {s['qpdf_checked']} opened PDFs → `doc.save()` → `qpdf --check`: "
          f"**{s['qpdf_pass']}/{s['qpdf_checked']} pass ({rate:.1%})** "
          f"(pass = qpdf reports no structural errors; warnings allowed).")
        a("")
        a("| file | qpdf result |")
        a("|---|---|")
        for r in payload["records"]:
            q = r.get("qpdf")
            if q and q.get("checked"):
                verdict = "PASS" if q["pass"] else "FAIL"
                a(f"| `{r['file']}` | {verdict} — {q['msg']} |")
    else:
        a("- No qpdf checks performed.")
    a("")

    # Text similarity — headline
    a("## 5. Differential text accuracy vs PyMuPDF (headline) & pdfminer")
    a("")
    sim = s.get("similarity", {})
    if not sim:
        a("- Oracle not available; no differential text comparison performed.")
    else:
        a("Per-document similarity of `oxide_pdf` `get_text(\"text\")` vs each oracle, "
          "on whitespace-normalized full-document text. Levenshtein = normalized edit "
          "similarity (sequence-level); Jaccard = word-set overlap (vocabulary-level).")
        a("")
        a("| oracle | docs | Levenshtein mean | Lev. median | Jaccard mean | Jacc. median | ≥0.95 | ≥0.80 | <0.50 |")
        a("|---|---|---|---|---|---|---|---|---|")
        for name in ("pymupdf", "pdfminer"):
            d = sim.get(name)
            if not d or not d["n"]:
                a(f"| {name} | 0 | — | — | — | — | — | — | — |")
                continue
            a(f"| **{name}** | {d['n']} | **{d['levenshtein_mean']}** | {d['levenshtein_median']} "
              f"| {d['jaccard_mean']} | {d['jaccard_median']} | {d['ge_0_95']} | {d['ge_0_80']} | {d['lt_0_50']} |")
        a("")
        pm = sim.get("pymupdf") or {}
        if pm.get("levenshtein_mean") is not None:
            a(f"**Headline (vs PyMuPDF / fitz):** mean Levenshtein **{pm['levenshtein_mean']:.3f}**, "
              f"median **{pm['levenshtein_median']:.3f}**, mean Jaccard **{pm['jaccard_mean']:.3f}** "
              f"over {pm['n']} documents.")
        a("")

        # Worst cases
        for name in ("pymupdf", "pdfminer"):
            d = sim.get(name)
            if not d or not d["n"]:
                continue
            a(f"### Worst-case divergences vs {name}")
            a("")
            a("| file | Lev | Jacc | our chars | their chars | why they differ |")
            a("|---|---|---|---|---|---|")
            for w in _worst_cases(payload["records"], name, k=8):
                a(f"| `{w['file']}` | {w['levenshtein']:.3f} | {w['jaccard']:.3f} "
                  f"| {w['our_chars']} | {w['their_chars']} | {w['reason']} |")
            a("")

    # Divergence causes
    a("## 6. Prioritized divergence causes (future diff-oracle fix tasks)")
    a("")
    causes = _aggregate_causes(payload["records"])
    if causes:
        for n, (reason, count, examples) in enumerate(causes, 1):
            ex = ", ".join(f"`{e}`" for e in examples[:3])
            a(f"{n}. **{reason}** — {count} doc(s). e.g. {ex}")
    else:
        a("- No notable divergence causes (all documents close to oracle).")
    a("")
    a("---")
    a("")
    a("_Methodology: each PDF is opened+extracted in an isolated subprocess (timeout "
      f"per file) so a Rust panic/abort cannot crash the harness. qpdf {payload['qpdf_version']}. "
      "Oracles: PyMuPDF (AGPL, local-only) primary; pdfminer.six (MIT) secondary. "
      "Similarity computed on normalized text via difflib SequenceMatcher (Levenshtein "
      "proxy) and token Jaccard._")
    a("")
    return "\n".join(L)


def _aggregate_causes(records: list[dict]) -> list[tuple[str, int, list[str]]]:
    """Bucket worst-case reasons (vs PyMuPDF) into coarse categories for the fix list."""
    buckets: dict[str, list[str]] = {}
    for r in records:
        s = (r.get("sim") or {}).get("pymupdf")
        if not (s and s.get("ok")) or s["levenshtein"] >= 0.92:
            continue
        reason = s.get("reason") or ""
        if "empty text while oracle" in reason:
            key = "Empty extraction where fitz has text (scanned/image-only pages or unsupported font program)"
        elif "much shorter" in reason or "absent from oxide" in reason:
            key = "Missing content — oxide drops glyphs/words fitz extracts (CMap/ToUnicode, ligatures, or embedded-font decoding gaps)"
        elif "much longer" in reason or "absent from oracle" in reason:
            key = "Extra/spurious content — oxide emits text fitz does not (mis-decoded glyphs or duplicated content)"
        elif "ordering/spacing" in reason or "reading-order" in reason:
            key = "Reading-order / word-spacing differences (column/line segmentation vs fitz)"
        else:
            key = "Moderate divergence (mixed spacing/encoding)"
        buckets.setdefault(key, []).append(r["file"])
    ranked = sorted(buckets.items(), key=lambda kv: len(kv[1]), reverse=True)
    return [(k, len(v), v) for k, v in ranked]


def _print_console_summary(s: dict) -> None:
    print("\n==================== SUMMARY ====================")
    print(f"Total PDFs            : {s['total']}")
    print(f"Open rate             : {s['opened']}/{s['total']} ({s['open_rate']:.1%})  repaired={s['repaired']} failed={s['failed']}")
    print(f"Robustness            : aborts={s['aborts']} timeouts={s['timeouts']}")
    if s["qpdf_checked"]:
        print(f"qpdf validity (sample): {s['qpdf_pass']}/{s['qpdf_checked']} ({s['qpdf_pass_rate']:.1%})")
    for name in ("pymupdf", "pdfminer"):
        d = (s.get("similarity") or {}).get(name)
        if d and d["n"]:
            print(f"Similarity vs {name:<9}: n={d['n']} lev mean={d['levenshtein_mean']} median={d['levenshtein_median']} "
                  f"jaccard mean={d['jaccard_mean']}")
    print("================================================")


if __name__ == "__main__":
    sys.exit(main())
