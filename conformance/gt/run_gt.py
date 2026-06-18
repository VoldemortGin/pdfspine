#!/usr/bin/env python3
"""Ground-truth accuracy orchestrator — score pdfspine vs fitz vs pdfminer.

Unlike ``conformance/run_validation.py`` (which scores our extraction against
PyMuPDF as a *pseudo*-oracle), this scores THREE extractors — ``pdfspine``,
``pymupdf``/fitz, and ``pdfminer.six`` — against an OBJECTIVE ground truth
(``gt_text`` shipped with the corpus, or text derived from a JATS ``nxml``
fulltext). That makes "match or exceed fitz" an objective claim: for every
document we can count where pdfspine's reading-order score meets or beats fitz's
*against the same truth*.

Pipeline per manifest entry:
  1. Resolve ground truth: ``gt_text`` if present, else ``nxml`` -> jats_text.
  2. Extract pdfspine text in an isolated subprocess (project ``.venv``,
     reusing ``conformance/pdfspine_worker.py``) under a wall-clock timeout — a
     Rust panic cannot kill the run.
  3. Extract fitz + pdfminer via ``conformance/oracle_extract.py`` under the
     oracle venv (``.venv-oracle``); PyMuPDF is never imported into this
     interpreter.
  4. Score each extractor's full-document text vs the ground truth with
     ``conformance/gt/score.py`` ``score_all()``.

Emits a machine-readable JSON and a human GT-REPORT.md.

Run from repo root::

    .venv/bin/python conformance/gt/run_gt.py \
        --manifest conformance/gt/born_manifest.json \
        --manifest conformance/gt/pmc_manifest.json \
        --report conformance/gt/GT-REPORT.md \
        --json conformance/gt/gt-results.json \
        --oracle-python .venv-oracle/bin/python

Self-test (no wheel, no network, no oracle needed)::

    python conformance/gt/run_gt.py --selftest

License posture: PyMuPDF (AGPL) runs only as a local subprocess oracle; its
output is used for live scoring only and is NEVER committed — only scores and
content-free structural notes are written.
"""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CONFORMANCE = REPO_ROOT / "conformance"
GT_DIR = CONFORMANCE / "gt"
WORKER = CONFORMANCE / "pdfspine_worker.py"
ORACLE = CONFORMANCE / "oracle_extract.py"

# Make sibling modules (score.py, jats_text.py) importable when run as a script.
if str(GT_DIR) not in sys.path:
    sys.path.insert(0, str(GT_DIR))

# The metrics we surface for every extractor, in display order. score_all() is
# expected to return at least these keys; missing keys degrade to None.
METRICS = ("lev", "f1", "jaccard", "order")
# The three extractors we score, in display order. "pdfspine" is ours.
EXTRACTORS = ("pdfspine", "pymupdf", "pdfminer")


# --------------------------------------------------------------------------- #
# Ground truth + scoring (lazy imports so --selftest needs no siblings)
# --------------------------------------------------------------------------- #
def _import_score_all():
    """Import score.py::score_all lazily; raise a clear error if absent."""
    try:
        from score import score_all  # type: ignore
    except Exception as exc:  # noqa: BLE001
        raise RuntimeError(
            f"could not import conformance/gt/score.py::score_all ({exc}); "
            "it is a sibling module built separately"
        ) from exc
    return score_all


def _import_nxml_to_text():
    """Import jats_text::nxml_to_text lazily; raise a clear error if absent."""
    try:
        from jats_text import nxml_to_text  # type: ignore
    except Exception as exc:  # noqa: BLE001
        raise RuntimeError(
            f"could not import conformance/gt/jats_text::nxml_to_text ({exc}); "
            "it is a sibling module built separately"
        ) from exc
    return nxml_to_text


def resolve_ground_truth(entry: dict, manifest_dir: Path) -> str:
    """Return the ground-truth text for a manifest entry.

    Priority: explicit ``gt_text`` > ``nxml`` file -> jats_text.nxml_to_text.
    Paths in the manifest are resolved relative to the manifest file's dir if
    not absolute.
    """
    if entry.get("gt_text") is not None:
        return str(entry["gt_text"])
    nxml = entry.get("nxml")
    if nxml:
        p = Path(nxml)
        if not p.is_absolute():
            p = manifest_dir / p
        nxml_to_text = _import_nxml_to_text()
        return nxml_to_text(p.read_bytes())
    raise ValueError("manifest entry has neither 'gt_text' nor 'nxml'")


def normalize_scores(raw: dict | None) -> dict:
    """Coerce a score_all() result into our fixed {metric: float|None} shape."""
    raw = raw or {}
    out: dict = {}
    for m in METRICS:
        v = raw.get(m)
        out[m] = round(float(v), 4) if isinstance(v, (int, float)) else None
    return out


# --------------------------------------------------------------------------- #
# Extraction (subprocess-isolated, mirrors run_validation.py)
# --------------------------------------------------------------------------- #
def extract_pdfspine(py: str, pdf: Path, timeout: float) -> tuple[str | None, str | None]:
    """Run the isolated pdfspine worker; return (joined_text, error).

    A Rust panic/abort/hang surfaces as a non-zero exit or timeout and is
    reported as an error rather than crashing the harness.
    """
    cmd = [py, str(WORKER), str(pdf)]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, f"pdfspine timeout after {timeout}s"
    if proc.returncode != 0:
        sig = -proc.returncode if proc.returncode < 0 else None
        msg = f"pdfspine worker exited {proc.returncode}"
        if sig:
            msg += f" (signal {sig})"
        if proc.stderr.strip():
            msg += f": {proc.stderr.strip()[:200]}"
        return None, msg
    try:
        data = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return None, f"pdfspine unparseable output: {proc.stdout[:200]!r}"
    if not data.get("opened"):
        return None, f"pdfspine could not open: {data.get('error')}"
    pages = data.get("pages") or []
    return "\n".join(pages), data.get("error")


def extract_oracles(
    oracle_py: str, pdf: Path, timeout: float
) -> dict[str, tuple[str | None, str | None]]:
    """Run oracle_extract.py under the oracle venv.

    Returns {"pymupdf": (text|None, err), "pdfminer": (text|None, err)}.
    """
    blank = {
        "pymupdf": (None, "oracle unavailable"),
        "pdfminer": (None, "oracle unavailable"),
    }
    if not Path(oracle_py).exists():
        return blank
    try:
        proc = subprocess.run(
            [oracle_py, str(ORACLE), str(pdf)],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return {
            "pymupdf": (None, f"oracle timeout after {timeout}s"),
            "pdfminer": (None, f"oracle timeout after {timeout}s"),
        }
    if proc.returncode != 0:
        err = f"oracle exit {proc.returncode}"
        return {"pymupdf": (None, err), "pdfminer": (None, err)}
    try:
        data = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"pymupdf": (None, "oracle bad json"), "pdfminer": (None, "oracle bad json")}
    out: dict[str, tuple[str | None, str | None]] = {}
    for name in ("pymupdf", "pdfminer"):
        o = data.get(name, {})
        if o.get("ok"):
            out[name] = ("\n".join(o.get("pages") or []), None)
        else:
            out[name] = (None, o.get("error", "oracle failed"))
    return out


# --------------------------------------------------------------------------- #
# Per-manifest processing
# --------------------------------------------------------------------------- #
def load_manifest(path: Path) -> tuple[str, list[dict]]:
    """Load a manifest JSON. Returns (subset_label, entries).

    Accepts either a bare list of entries, or an object with an ``entries`` (or
    ``documents``) list and an optional ``subset``/``name`` label. The label
    distinguishes born-digital vs pmc subsets in the report.
    """
    data = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(data, list):
        entries = data
        label = path.stem
    else:
        entries = data.get("entries") or data.get("documents") or []
        label = data.get("subset") or data.get("name") or path.stem
    return str(label), list(entries)


def process_entry(
    entry: dict,
    subset: str,
    manifest_dir: Path,
    py: str,
    oracle_py: str,
    timeout: float,
    score_all,
) -> dict:
    """Extract with 3 extractors, score each vs ground truth, return a record."""
    pdf_raw = entry.get("pdf") or entry.get("path") or entry.get("file")
    doc_id = entry.get("id") or (Path(pdf_raw).name if pdf_raw else "?")
    rec: dict = {
        "id": doc_id,
        "subset": subset,
        "pdf": pdf_raw,
        "gt_chars": None,
        "scores": {ex: None for ex in EXTRACTORS},
        "errors": {},
        "skipped": False,
    }

    # Ground truth (skip the whole entry if we can't resolve it).
    try:
        gt_text = resolve_ground_truth(entry, manifest_dir)
    except Exception as exc:  # noqa: BLE001
        rec["skipped"] = True
        rec["errors"]["gt"] = f"{type(exc).__name__}: {exc}"
        return rec
    rec["gt_chars"] = len(gt_text or "")

    if not pdf_raw:
        rec["skipped"] = True
        rec["errors"]["pdf"] = "manifest entry has no 'pdf'/'path'/'file'"
        return rec
    pdf = Path(pdf_raw)
    if not pdf.is_absolute():
        pdf = manifest_dir / pdf
    if not pdf.exists():
        rec["skipped"] = True
        rec["errors"]["pdf"] = f"pdf not found: {pdf}"
        return rec

    # Extract all three.
    texts: dict[str, str | None] = {}
    ox_text, ox_err = extract_pdfspine(py, pdf, timeout)
    texts["pdfspine"] = ox_text
    if ox_err:
        rec["errors"]["pdfspine"] = ox_err
    oracles = extract_oracles(oracle_py, pdf, timeout)
    for name in ("pymupdf", "pdfminer"):
        t, e = oracles[name]
        texts[name] = t
        if e:
            rec["errors"][name] = e

    # Score each available extractor vs ground truth.
    for ex in EXTRACTORS:
        t = texts.get(ex)
        if t is None:
            continue
        try:
            rec["scores"][ex] = normalize_scores(score_all(gt_text, t))
        except Exception as exc:  # noqa: BLE001
            rec["errors"][ex] = (rec["errors"].get(ex, "") + f" | score: {exc}").strip(" |")
    return rec


# --------------------------------------------------------------------------- #
# Aggregation
# --------------------------------------------------------------------------- #
def _collect(records: list[dict], extractor: str, metric: str) -> list[float]:
    out = []
    for r in records:
        s = (r.get("scores") or {}).get(extractor)
        if s and s.get(metric) is not None:
            out.append(s[metric])
    return out


def aggregate(records: list[dict]) -> dict:
    """Compute per-extractor mean/median for each metric over a record set."""
    agg: dict = {}
    for ex in EXTRACTORS:
        per_metric: dict = {}
        n = 0
        for m in METRICS:
            vals = _collect(records, ex, m)
            n = max(n, len(vals))
            per_metric[m] = {
                "mean": round(statistics.mean(vals), 4) if vals else None,
                "median": round(statistics.median(vals), 4) if vals else None,
            }
        agg[ex] = {"n": n, "metrics": per_metric}
    return agg


def head_to_head(records: list[dict], metric: str = "order") -> dict:
    """Objective match/exceed: pdfspine vs fitz on `metric` vs the same ground truth.

    Returns counts over docs where BOTH pdfspine and fitz produced a score.
    """
    pdfspine_ge = pdfspine_gt = fitz_gt = comparable = 0
    wins: list[dict] = []
    losses: list[dict] = []
    for r in records:
        os_ = (r.get("scores") or {}).get("pdfspine")
        fs_ = (r.get("scores") or {}).get("pymupdf")
        if not (os_ and fs_):
            continue
        ov, fv = os_.get(metric), fs_.get(metric)
        if ov is None or fv is None:
            continue
        comparable += 1
        if ov >= fv:
            pdfspine_ge += 1
        if ov > fv:
            pdfspine_gt += 1
            wins.append({"id": r["id"], "pdfspine": ov, "fitz": fv, "delta": round(ov - fv, 4)})
        if fv > ov:
            fitz_gt += 1
            losses.append({"id": r["id"], "pdfspine": ov, "fitz": fv, "delta": round(ov - fv, 4)})
    wins.sort(key=lambda d: d["delta"], reverse=True)
    losses.sort(key=lambda d: d["delta"])
    return {
        "metric": metric,
        "comparable": comparable,
        "pdfspine_ge_fitz": pdfspine_ge,  # match-or-exceed
        "pdfspine_gt_fitz": pdfspine_gt,  # strictly beats
        "fitz_gt_pdfspine": fitz_gt,
        "wins": wins,
        "losses": losses,
    }


# --------------------------------------------------------------------------- #
# Report rendering
# --------------------------------------------------------------------------- #
def _fmt(v) -> str:
    return f"{v:.3f}" if isinstance(v, (int, float)) else "—"


def _headline_table(agg: dict) -> list[str]:
    lines = ["| extractor | docs | lev | f1 | jaccard | order |",
             "|---|---|---|---|---|---|"]
    for ex in EXTRACTORS:
        d = agg[ex]
        cells = []
        for m in METRICS:
            mm = d["metrics"][m]
            cells.append(f"{_fmt(mm['mean'])} / {_fmt(mm['median'])}")
        label = "**pdfspine**" if ex == "pdfspine" else ex
        lines.append(f"| {label} | {d['n']} | " + " | ".join(cells) + " |")
    return lines


def render_report(payload: dict) -> str:
    L: list[str] = []
    a = L.append
    a("# pdfspine — Objective Ground-Truth Accuracy Report")
    a("")
    a(f"_Generated: {payload['generated']} • oracle (PyMuPDF/pdfminer) available: "
      f"{payload['oracle_available']}_")
    a("")
    a("Each extractor — **pdfspine**, **pymupdf** (fitz), and **pdfminer** — is scored "
      "against the SAME objective ground truth (`gt_text` or JATS `nxml` fulltext), not "
      "against another extractor. Cells show **mean / median**. Metrics: `lev` (edit "
      "similarity), `f1` (token F1), `jaccard` (word-set overlap), `order` (reading-order "
      "similarity). No PyMuPDF output is committed — only scores.")
    a("")

    # Overall headline
    a("## 1. Headline — all docs")
    a("")
    a(f"Corpus: **{payload['n_docs']}** documents "
      f"({payload['n_scored']} with at least one extractor scored, "
      f"{payload['n_skipped']} skipped).")
    a("")
    L.extend(_headline_table(payload["aggregate"]))
    a("")

    # Per-subset (born vs pmc) when multiple manifests/subsets present.
    subsets = payload.get("by_subset") or {}
    if len(subsets) > 1:
        a("## 2. By subset (born-digital vs pmc)")
        a("")
        for label in sorted(subsets):
            sub = subsets[label]
            a(f"### {label} — {sub['n']} docs")
            a("")
            L.extend(_headline_table(sub["aggregate"]))
            a("")
        sect = 3
    else:
        sect = 2

    # Head-to-head (the objective match/exceed claim)
    h = payload["head_to_head"]
    a(f"## {sect}. Objective match/exceed vs fitz (reading order)")
    a("")
    if h["comparable"] == 0:
        a("- No documents where both pdfspine and fitz produced a comparable score.")
    else:
        pct = h["pdfspine_ge_fitz"] / h["comparable"]
        a(f"Over **{h['comparable']}** documents scored by both pdfspine and fitz against "
          f"ground truth, on the `{h['metric']}` (reading-order) metric:")
        a("")
        a(f"- **pdfspine ≥ fitz (match or exceed): {h['pdfspine_ge_fitz']}/{h['comparable']} "
          f"({pct:.1%})**")
        a(f"- pdfspine strictly beats fitz: {h['pdfspine_gt_fitz']}")
        a(f"- fitz strictly beats pdfspine: {h['fitz_gt_pdfspine']}")
        a("")
        if h["wins"]:
            a("**Where pdfspine beats fitz vs ground truth:**")
            a("")
            a("| doc | pdfspine order | fitz order | Δ |")
            a("|---|---|---|---|")
            for w in h["wins"][:10]:
                a(f"| `{w['id']}` | {w['pdfspine']:.3f} | {w['fitz']:.3f} | +{w['delta']:.3f} |")
            a("")
        if h["losses"]:
            a("**Where pdfspine loses to fitz vs ground truth (fix targets):**")
            a("")
            a("| doc | pdfspine order | fitz order | Δ |")
            a("|---|---|---|---|")
            for w in h["losses"][:10]:
                a(f"| `{w['id']}` | {w['pdfspine']:.3f} | {w['fitz']:.3f} | {w['delta']:.3f} |")
            a("")
    sect += 1

    # Per-document table
    a(f"## {sect}. Per-document scores")
    a("")
    a("`lev` shown per extractor (o=pdfspine, f=fitz, p=pdfminer); `ord` = order metric.")
    a("")
    a("| doc | subset | gt chars | o lev | f lev | p lev | o ord | f ord | p ord | notes |")
    a("|---|---|---|---|---|---|---|---|---|---|")
    for r in payload["records"]:
        sc = r.get("scores") or {}

        def cell(ex: str, m: str) -> str:
            s = sc.get(ex)
            return _fmt(s.get(m)) if s else "—"

        notes = ""
        if r.get("skipped"):
            notes = "SKIPPED: " + "; ".join(f"{k}={v}" for k, v in (r.get("errors") or {}).items())
        elif r.get("errors"):
            notes = "; ".join(f"{k}: {v}" for k, v in r["errors"].items())
        notes = notes[:120]
        a(f"| `{r['id']}` | {r['subset']} | {r.get('gt_chars') if r.get('gt_chars') is not None else '—'} "
          f"| {cell('pdfspine','lev')} | {cell('pymupdf','lev')} | {cell('pdfminer','lev')} "
          f"| {cell('pdfspine','order')} | {cell('pymupdf','order')} | {cell('pdfminer','order')} "
          f"| {notes} |")
    a("")
    a("---")
    a("")
    a("_Methodology: pdfspine extracted in an isolated subprocess (project venv) under a "
      "wall-clock timeout so a Rust panic cannot crash the run; fitz + pdfminer extracted "
      "via conformance/oracle_extract.py under the oracle venv. All three scored vs the same "
      "ground truth by conformance/gt/score.py. Multi-column reading order is the known weak "
      "spot; the `order` head-to-head is the objective match/exceed signal._")
    a("")
    return "\n".join(L)


# --------------------------------------------------------------------------- #
# Orchestration
# --------------------------------------------------------------------------- #
def build_payload(records: list[dict], oracle_available: bool) -> dict:
    """Assemble the full JSON payload (scores + aggregates) from records."""
    scored = [r for r in records if not r.get("skipped")]
    skipped = [r for r in records if r.get("skipped")]

    by_subset: dict[str, dict] = {}
    subsets = sorted({r["subset"] for r in records})
    for label in subsets:
        subset_recs = [r for r in records if r["subset"] == label]
        by_subset[label] = {
            "n": len(subset_recs),
            "aggregate": aggregate(subset_recs),
        }

    return {
        "generated": datetime.now(timezone.utc).isoformat(),
        "oracle_available": oracle_available,
        "n_docs": len(records),
        "n_scored": len(scored),
        "n_skipped": len(skipped),
        "aggregate": aggregate(records),
        "by_subset": by_subset,
        "head_to_head": head_to_head(records, metric="order"),
        "records": records,
    }


def run(
    manifests: list[Path],
    py: str,
    oracle_py: str,
    timeout: float,
    limit: int | None,
) -> dict:
    """Process all manifests and return the assembled payload."""
    score_all = _import_score_all()
    oracle_available = Path(oracle_py).exists()
    records: list[dict] = []
    for mpath in manifests:
        subset, entries = load_manifest(mpath)
        manifest_dir = mpath.resolve().parent
        if limit is not None:
            entries = entries[:limit]
        print(f"[manifest] {mpath} subset={subset} entries={len(entries)}", flush=True)
        for i, entry in enumerate(entries, 1):
            rec = process_entry(
                entry, subset, manifest_dir, py, oracle_py, timeout, score_all
            )
            tag = "SKIP" if rec.get("skipped") else "ok"
            print(f"  [{i}/{len(entries)}] {rec['id']} -> {tag}", flush=True)
            records.append(rec)
    return build_payload(records, oracle_available)


# --------------------------------------------------------------------------- #
# Self-test (no wheel, no network, no oracle, no siblings)
# --------------------------------------------------------------------------- #
def _selftest() -> int:
    """Fabricate 2 fake docs with known gt_text and fake extractor outputs.

    Bypasses real extraction/siblings entirely: we build records by hand using a
    deterministic local scorer, then verify the aggregate table + head-to-head +
    markdown all compute and render correctly. Proves the plumbing without the
    wheel or network.
    """
    import re
    import tempfile

    # Local deterministic scorer standing in for score.py::score_all. Uses only
    # the stdlib so the self-test is fully isolated.
    def fake_score_all(gt: str, hyp: str) -> dict:
        import difflib

        ga, ha = gt.split(), hyp.split()
        lev = difflib.SequenceMatcher(None, ga, ha, autojunk=False).ratio() if (ga or ha) else 1.0
        sg = set(re.findall(r"\w+", gt.lower()))
        sh = set(re.findall(r"\w+", hyp.lower()))
        inter = len(sg & sh)
        jac = inter / len(sg | sh) if (sg | sh) else 1.0
        prec = inter / len(sh) if sh else 0.0
        rec = inter / len(sg) if sg else 0.0
        f1 = (2 * prec * rec / (prec + rec)) if (prec + rec) else 0.0
        # order proxy: sequence ratio (sensitive to word order)
        order = lev
        return {"lev": lev, "f1": f1, "jaccard": jac, "order": order}

    gt1 = "the quick brown fox jumps over the lazy dog"
    gt2 = "left column text right column text continues here"

    # Doc 1: pdfspine perfect, fitz slightly worse order, pdfminer worse.
    # Doc 2: pdfspine better order than fitz (multi-column win), pdfminer worst.
    fake = {
        "doc1": {
            "gt": gt1,
            "pdfspine": gt1,  # perfect
            "pymupdf": "the quick brown fox jumps the over lazy dog",  # reorder
            "pdfminer": "quick brown fox the lazy dog",  # dropped words
        },
        "doc2": {
            "gt": gt2,
            "pdfspine": gt2,  # perfect reading order
            "pymupdf": "left right column column text text continues here",  # column-mixed
            "pdfminer": "right column text left column text continues here",  # swapped
        },
    }

    records: list[dict] = []
    for doc_id, d in fake.items():
        rec = {
            "id": doc_id,
            "subset": "selftest",
            "pdf": f"{doc_id}.pdf",
            "gt_chars": len(d["gt"]),
            "scores": {ex: None for ex in EXTRACTORS},
            "errors": {},
            "skipped": False,
        }
        for ex in EXTRACTORS:
            rec["scores"][ex] = normalize_scores(fake_score_all(d["gt"], d[ex]))
        records.append(rec)

    payload = build_payload(records, oracle_available=False)

    # --- assertions on aggregation ---
    agg = payload["aggregate"]
    assert set(agg) == set(EXTRACTORS), agg.keys()
    for ex in EXTRACTORS:
        assert agg[ex]["n"] == 2, (ex, agg[ex]["n"])
        for m in METRICS:
            mm = agg[ex]["metrics"][m]
            assert mm["mean"] is not None and mm["median"] is not None, (ex, m, mm)

    # pdfspine is perfect on both docs -> mean lev/order == 1.0
    assert agg["pdfspine"]["metrics"]["lev"]["mean"] == 1.0, agg["pdfspine"]
    assert agg["pdfspine"]["metrics"]["order"]["mean"] == 1.0, agg["pdfspine"]
    # fitz strictly worse order than pdfspine on both docs
    assert agg["pymupdf"]["metrics"]["order"]["mean"] < 1.0, agg["pymupdf"]

    # mean check: recompute one cell by hand
    expected_fitz_lev = round(
        statistics.mean(
            [records[0]["scores"]["pymupdf"]["lev"], records[1]["scores"]["pymupdf"]["lev"]]
        ),
        4,
    )
    assert agg["pymupdf"]["metrics"]["lev"]["mean"] == expected_fitz_lev, (
        agg["pymupdf"]["metrics"]["lev"]["mean"],
        expected_fitz_lev,
    )

    # --- head-to-head ---
    h = payload["head_to_head"]
    assert h["comparable"] == 2, h
    assert h["pdfspine_ge_fitz"] == 2, h  # pdfspine matches-or-exceeds fitz on both
    assert h["pdfspine_gt_fitz"] == 2, h  # strictly beats on both
    assert h["fitz_gt_pdfspine"] == 0, h
    assert len(h["wins"]) == 2 and not h["losses"], h

    # --- by_subset present ---
    assert "selftest" in payload["by_subset"], payload["by_subset"].keys()
    assert payload["by_subset"]["selftest"]["n"] == 2

    # --- markdown renders + writes ---
    md = render_report(payload)
    assert "Objective Ground-Truth Accuracy Report" in md
    assert "Objective match/exceed vs fitz" in md
    assert "**pdfspine**" in md
    assert "pymupdf" in md and "pdfminer" in md
    assert "doc1" in md and "doc2" in md

    with tempfile.TemporaryDirectory() as td:
        tdp = Path(td)
        report_path = tdp / "GT-REPORT.md"
        json_path = tdp / "gt-results.json"
        report_path.write_text(md, encoding="utf-8")
        json_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
        assert report_path.exists() and report_path.stat().st_size > 0
        assert json_path.exists() and json_path.stat().st_size > 0
        reloaded = json.loads(json_path.read_text(encoding="utf-8"))
        assert reloaded["head_to_head"]["pdfspine_ge_fitz"] == 2

    print("run_gt.py self-test OK")
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--manifest", type=Path, action="append", default=[],
                    help="corpus manifest JSON (repeatable: born + pmc)")
    ap.add_argument("--report", type=Path, help="output markdown report path")
    ap.add_argument("--json", dest="json_out", type=Path, help="output JSON path")
    ap.add_argument("--python", default=sys.executable,
                    help="project venv python (with built pdfspine wheel)")
    ap.add_argument("--oracle-python", default=str(REPO_ROOT / ".venv-oracle" / "bin" / "python"))
    ap.add_argument("--timeout", type=float, default=120.0, help="per-PDF wall-clock timeout (s)")
    ap.add_argument("--limit", type=int, default=None, help="cap entries per manifest")
    ap.add_argument("--selftest", action="store_true",
                    help="run offline plumbing self-test (no wheel/network/oracle) and exit")
    args = ap.parse_args(argv)

    if args.selftest:
        return _selftest()

    if not args.manifest:
        ap.error("at least one --manifest is required (or use --selftest)")
    if not args.report or not args.json_out:
        ap.error("--report and --json are required")

    missing = [str(m) for m in args.manifest if not m.exists()]
    if missing:
        print(f"ERROR: manifest(s) not found: {missing}", file=sys.stderr)
        return 1

    payload = run(args.manifest, args.python, args.oracle_python, args.timeout, args.limit)

    args.json_out.parent.mkdir(parents=True, exist_ok=True)
    args.json_out.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(f"\nWrote {args.json_out}")

    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(render_report(payload), encoding="utf-8")
    print(f"Wrote {args.report}")

    # Console one-liner.
    h = payload["head_to_head"]
    if h["comparable"]:
        print(f"pdfspine ≥ fitz (order): {h['pdfspine_ge_fitz']}/{h['comparable']} "
              f"({h['pdfspine_ge_fitz'] / h['comparable']:.1%})")
    else:
        print("no comparable pdfspine-vs-fitz docs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
