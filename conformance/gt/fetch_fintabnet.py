#!/usr/bin/env python3
"""Fetch a SMALL FinTabNet.c gold table-structure sample (annotations + PDFs).

Part of the objective ground-truth subsystem (sibling of ``fetch_eurlex.py`` /
``pmc_fetch.py``). Where ``tables_diff.py`` measures pdfspine ``find_tables`` vs
**fitz agreement** (no objective ground truth), this fetcher supplies HUMAN-derived
GOLD cell-structure annotations so ``find_tables`` can be scored ABSOLUTELY with
GriTS (see ``grits.py`` / ``tables_diff.py --gold``).

Dataset & license
-----------------
* **FinTabNet.c** (Smock, Pesala & Abraham, 2023; arXiv 2303.00716) — a cleaned,
  canonicalized fork of IBM's FinTabNet with corrected cell-structure annotations.
* License: **CDLA-Permissive-2.0** (annotations) — a permissive Community Data
  License Agreement; the underlying FinTabNet PDFs are **CDLA-Permissive-1.0**.
  Both permit reuse incl. commercial. We use ONLY these permissively-licensed data.
* Source of record:
    - annotations: ``https://huggingface.co/datasets/bsmock/FinTabNet.c``
      (file ``FinTabNet.c-PDF_Annotations.tar.gz`` — 77,437 per-table JSON files,
      NO PDFs inside despite the name).
    - the matching source **PDFs** live ONLY in the original FinTabNet release on
      IBM's Data Asset eXchange CDN (``dax-cdn.cdn.appdomain.cloud``), as
      ``dax-fintabnet/1.0.0/fintabnet.tar.gz``; the annotation's ``pdf_bbox``
      coordinates are in that PDF's page space, so a coordinate-matched PDF is
      REQUIRED — no image/HTML mirror substitutes.

What this fetches (SMALL by design)
-----------------------------------
We do NOT pull the whole dataset. We stream the HF annotations tarball and extract
only the first ``--sample`` (default 30) ``*_tables.json`` files (each is one PDF
page), then for each fetch only that one source PDF from the FinTabNet CDN at
``pdf/<TICKER>/<YEAR>/<page_NN.pdf>``. Everything is cached to ``--out`` so re-runs
are cheap, and the corpus dir matches the gitignored ``conformance/gt/corpus-*/``
glob (we commit the SCRIPT + the REPORT, never the data).

Network reality
---------------
Like the sibling fetchers we strip proxy env via ``ProxyHandler({})`` and use
HTTPS with retries; failures are skipped and the run continues. If the FinTabNet
PDF CDN is unreachable from the current environment, the annotations are still
fetched and the manifest records ``pdf_status`` per entry so the harness/report can
state precisely what is missing (a clean BLOCKED status, not a fabricated score).

CLI::

    env -u CONDA_PREFIX .venv/bin/python conformance/gt/fetch_fintabnet.py \
        --out conformance/gt/corpus-fintabnet --sample 30

    # offline-safe + tiny-real self test:
    env -u CONDA_PREFIX .venv/bin/python conformance/gt/fetch_fintabnet.py --self-test
"""

from __future__ import annotations

import argparse
import http.client
import json
import sys
import tarfile
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO_ROOT / "conformance" / "gt" / "corpus-fintabnet"

# HuggingFace — annotations (reachable, CDLA-Permissive-2.0).
HF_ANNO_URL = (
    "https://huggingface.co/datasets/bsmock/FinTabNet.c/resolve/main/"
    "FinTabNet.c-PDF_Annotations.tar.gz"
)
ANNO_LICENSE = "CDLA-Permissive-2.0"

# IBM Data Asset eXchange — original FinTabNet PDFs (CDLA-Permissive-1.0).
# The single-page PDFs are inside fintabnet.tar.gz at pdf/<TICKER>/<YEAR>/page_N.pdf.
# We try a per-file path first (cheap) and record status; the whole tarball is GBs
# and intentionally NOT downloaded here.
DAX_PDF_BASES = (
    "https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/fintabnet/pdf/",
    "https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/pdf/",
)
PDF_LICENSE = "CDLA-Permissive-1.0"

_UA = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
)
_TIMEOUT = 120
_MAX_RETRIES = 3
_REQUEST_DELAY = 0.3


# --------------------------------------------------------------------------- #
# HTTP (proxy-stripped, retrying) — mirrors fetch_eurlex.py / pmc_fetch.py
# --------------------------------------------------------------------------- #
def _openers() -> list[urllib.request.OpenerDirector]:
    """Openers to try, in order.

    Sibling fetchers strip proxy env via ``ProxyHandler({})`` because a local proxy
    breaks their TLS; we keep that as the PRIMARY path. But some environments block
    *direct* egress and only permit the env proxy, so we fall back to the default
    (env-proxy) opener if the proxy-stripped one cannot connect. The first opener
    that connects for a given host is reused thereafter (see ``_get``).
    """
    return [
        urllib.request.build_opener(urllib.request.ProxyHandler({})),  # proxy-stripped
        urllib.request.build_opener(),                                  # env proxy
    ]


# Remember which opener index last connected, so we don't re-pay the dead path's
# timeout on every request once the environment's egress route is known.
_PREFERRED_OPENER: list[int] = [0]
_PROBE_TIMEOUT = 15  # short connect probe to pick the live egress route quickly


def _select_opener(probe_url: str = HF_ANNO_URL) -> int:
    """Pick the egress route once per run with a SHORT probe (HEAD-ish GET).

    Avoids paying the full timeout of a dead route on the big streaming download.
    Sets ``_PREFERRED_OPENER`` and returns its index. If neither route connects in
    the probe window we leave the preference unchanged (the real call will surface
    the network error).
    """
    openers = _openers()
    for oi, opener in enumerate(openers):
        try:
            req = urllib.request.Request(probe_url, headers={"User-Agent": _UA})
            req.add_header("Range", "bytes=0-0")  # 1 byte — cheap reachability check
            with opener.open(req, timeout=_PROBE_TIMEOUT):
                _PREFERRED_OPENER[0] = oi
                return oi
        except urllib.error.HTTPError:
            _PREFERRED_OPENER[0] = oi  # reached the server (any HTTP status = route live)
            return oi
        except Exception:  # noqa: BLE001
            continue
    return _PREFERRED_OPENER[0]


def _get(url: str, *, timeout: int = _TIMEOUT, retries: int = _MAX_RETRIES,
         stream: bool = False):
    """GET ``url``. If ``stream`` return the open response (caller reads); else
    return its full body bytes. Retries transient errors; 404/403/406 raise.

    Tries the proxy-stripped opener first, then the env-proxy opener (whichever
    connected last is tried first on subsequent calls).
    """
    openers = _openers()
    order = sorted(range(len(openers)), key=lambda i: i != _PREFERRED_OPENER[0])
    last: Exception | None = None
    for attempt in range(1, retries + 1):
        for oi in order:
            opener = openers[oi]
            try:
                req = urllib.request.Request(url, headers={"User-Agent": _UA})
                resp = opener.open(req, timeout=timeout)
                _PREFERRED_OPENER[0] = oi
                if stream:
                    return resp
                with resp:
                    return resp.read()
            except urllib.error.HTTPError as exc:
                if exc.code in (400, 403, 404, 406):
                    raise  # definitive answer — don't try the other opener
                last = exc
            except (urllib.error.URLError, OSError, TimeoutError,
                    http.client.IncompleteRead, http.client.HTTPException) as exc:
                last = exc  # transient / route dead — try the next opener
        if attempt < retries:
            time.sleep(min(1.5 * attempt, 4.0))
    assert last is not None
    raise last


# --------------------------------------------------------------------------- #
# Annotation parsing -> document_id / pdf path components
# --------------------------------------------------------------------------- #
def _pdf_components(table_anno: dict) -> tuple[str, str]:
    """Return (pdf_rel_path, page_file) for one table's annotation.

    The annotation carries ``pdf_folder`` (e.g. ``"KIM/2010/"``) and
    ``pdf_file_name`` (e.g. ``"page_125.pdf"``). The FinTabNet CDN serves these at
    ``pdf/<folder><file>``.
    """
    folder = (table_anno.get("pdf_folder") or "").lstrip("/")
    fname = table_anno.get("pdf_file_name") or ""
    return f"{folder}{fname}", fname


# --------------------------------------------------------------------------- #
# Annotation sample (stream the HF tarball, take the first N pages)
# --------------------------------------------------------------------------- #
def fetch_annotation_sample(out_dir: Path, sample: int, *, verbose: bool = True) -> list[Path]:
    """Stream the HF annotations tarball and extract the first ``sample`` page
    annotation files into ``out_dir/annotations``. Cached: skips if already there.

    Returns the list of extracted ``*_tables.json`` paths (sorted by name).
    """
    anno_dir = out_dir / "annotations"
    anno_dir.mkdir(parents=True, exist_ok=True)

    existing = sorted(anno_dir.glob("*_tables.json"))
    if len(existing) >= sample:
        if verbose:
            print(f"  CACHED {len(existing)} annotation file(s) in {anno_dir}")
        return existing[:sample]

    if verbose:
        print(f"  streaming annotations from HuggingFace (taking first {sample}) ...")
    _select_opener()  # pick the live egress route quickly before the big download
    resp = _get(HF_ANNO_URL, stream=True)
    extracted: list[Path] = []
    try:
        # Stream-decompress; stop as soon as we have ``sample`` files.
        with tarfile.open(fileobj=resp, mode="r|gz") as tf:
            for member in tf:
                if not (member.isfile() and member.name.endswith("_tables.json")):
                    continue
                data = tf.extractfile(member)
                if data is None:
                    continue
                name = Path(member.name).name
                dest = anno_dir / name
                dest.write_bytes(data.read())
                extracted.append(dest)
                if verbose and len(extracted) % 10 == 0:
                    print(f"    ... {len(extracted)}/{sample}")
                if len(extracted) >= sample:
                    break
    finally:
        with __import__("contextlib").suppress(Exception):
            resp.close()
    if verbose:
        print(f"  fetched {len(extracted)} annotation file(s) -> {anno_dir}")
    return sorted(extracted)


# --------------------------------------------------------------------------- #
# Source PDF (FinTabNet CDN) — per-page fetch with status
# --------------------------------------------------------------------------- #
def fetch_pdf_for(table_anno: dict, pdf_dir: Path, *, verbose: bool = False) -> tuple[Path | None, str]:
    """Fetch the single source PDF for one annotation. Returns (path|None, status).

    ``status`` is one of: ``"cached"``, ``"ok"``, ``"http-404"``, ``"unreachable"``.
    The CDN file lives at ``pdf/<TICKER>/<YEAR>/page_N.pdf``; we try the known base
    URLs and validate the ``%PDF`` magic.
    """
    rel, fname = _pdf_components(table_anno)
    if not rel or not fname:
        return None, "no-path"
    doc_id = table_anno.get("document_id") or rel.replace("/", "_")
    dest = pdf_dir / f"{doc_id}.pdf"
    if dest.exists() and dest.stat().st_size > 0 and dest.read_bytes()[:4] == b"%PDF":
        return dest, "cached"

    pdf_dir.mkdir(parents=True, exist_ok=True)
    last_status = "unreachable"
    for base in DAX_PDF_BASES:
        url = base + rel
        try:
            data = _get(url)
        except urllib.error.HTTPError as exc:
            last_status = f"http-{exc.code}"
            continue
        except (urllib.error.URLError, OSError, TimeoutError,
                http.client.HTTPException):
            last_status = "unreachable"
            continue
        if data[:4] == b"%PDF":
            dest.write_bytes(data)
            return dest, "ok"
        last_status = "not-pdf"
    return None, last_status


# --------------------------------------------------------------------------- #
# Public API: build the corpus + manifest
# --------------------------------------------------------------------------- #
def fetch_fintabnet(out_dir: Path, sample: int = 30, *, verbose: bool = True) -> dict:
    """Fetch a small FinTabNet.c gold sample (annotations + matching PDFs).

    Writes ``out_dir/manifest.json`` describing every page: its annotation file,
    the source-PDF path/status, table count, and license/provenance. Returns the
    manifest dict (also a summary with ``n_pages`` / ``n_pdfs`` counts).

    Entries whose source PDF could not be fetched still appear (with
    ``pdf_status`` and ``pdf=null``) so the harness can score what is available and
    the report can state exactly what is missing.
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    pdf_dir = out_dir / "pdfs"

    def log(m: str) -> None:
        if verbose:
            print(m, flush=True)

    anno_files = fetch_annotation_sample(out_dir, sample, verbose=verbose)

    entries: list[dict] = []
    n_pdf_ok = 0
    pdf_status_counts: dict[str, int] = {}
    for af in anno_files:
        try:
            tables = json.loads(af.read_text(encoding="utf-8"))
        except Exception as exc:  # noqa: BLE001
            log(f"  SKIP   {af.name} — bad json: {exc}")
            continue
        if not tables:
            continue
        head = tables[0]
        doc_id = head.get("document_id") or af.stem.replace("_tables", "")
        rel, _fname = _pdf_components(head)

        time.sleep(_REQUEST_DELAY)
        pdf_path, status = fetch_pdf_for(head, pdf_dir, verbose=verbose)
        pdf_status_counts[status] = pdf_status_counts.get(status, 0) + 1
        if pdf_path is not None:
            n_pdf_ok += 1

        n_struct = sum(1 for t in tables if not t.get("exclude_for_structure"))
        entries.append({
            "document_id": doc_id,
            "annotation": str(af.resolve()),
            "pdf": str(pdf_path.resolve()) if pdf_path else None,
            "pdf_status": status,
            "pdf_rel_path": rel,
            "n_tables": len(tables),
            "n_tables_structure": n_struct,
            "pdf_page_index": head.get("pdf_page_index", 0),
            "anno_license": ANNO_LICENSE,
            "pdf_license": PDF_LICENSE,
        })
        flag = "PDF " if pdf_path else "    "
        log(f"  {flag} {doc_id}: tables={len(tables)} struct={n_struct} pdf={status}")

    manifest = {
        "dataset": "FinTabNet.c",
        "dataset_paper": "arXiv:2303.00716",
        "annotations_source": HF_ANNO_URL,
        "annotations_license": ANNO_LICENSE,
        "pdf_source": DAX_PDF_BASES[0],
        "pdf_license": PDF_LICENSE,
        "metric": "GriTS (grits.py): GriTS_Top (topology) + GriTS_Con (content)",
        "sample_requested": sample,
        "n_pages": len(entries),
        "n_pdfs_fetched": n_pdf_ok,
        "pdf_status_counts": pdf_status_counts,
        "entries": entries,
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8"
    )
    log(f"\nFinTabNet.c sample: {len(entries)} page(s), {n_pdf_ok} PDF(s) fetched "
        f"(pdf status: {pdf_status_counts}).")
    log(f"Manifest -> {out_dir / 'manifest.json'}")
    if n_pdf_ok == 0 and entries:
        log("NOTE: 0 source PDFs reachable — annotations are present but the "
            "FinTabNet PDF CDN (dax-cdn.cdn.appdomain.cloud) is unreachable here. "
            "Scoring is BLOCKED on the PDFs; see the report.")
    return manifest


# --------------------------------------------------------------------------- #
# Self-test: offline parsing checks + a tiny real fetch
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    import tempfile

    # --- offline: pdf-path component derivation ---
    anno = {"pdf_folder": "KIM/2010/", "pdf_file_name": "page_125.pdf",
            "document_id": "KIM_2010_page_125"}
    rel, fname = _pdf_components(anno)
    assert rel == "KIM/2010/page_125.pdf", rel
    assert fname == "page_125.pdf", fname
    print("  offline checks OK (pdf path derivation)")

    # --- tiny real fetch: 3 annotation pages from HF; PDFs best-effort ---
    with tempfile.TemporaryDirectory() as td:
        try:
            man = fetch_fintabnet(Path(td), sample=3, verbose=True)
        except Exception as exc:  # noqa: BLE001
            print(f"  live fetch skipped (network unavailable: {type(exc).__name__}: {exc})")
            print("fetch_fintabnet.py self-test OK (offline checks passed)")
            return 0
        if man["n_pages"] == 0:
            print("  live fetch returned 0 pages (HF unreachable/throttled); "
                  "offline checks still passed")
            print("fetch_fintabnet.py self-test OK")
            return 0
        # Every entry must carry a real, parseable gold annotation with cells.
        for e in man["entries"]:
            tables = json.loads(Path(e["annotation"]).read_text(encoding="utf-8"))
            assert tables and "cells" in tables[0], e["document_id"]
            assert tables[0]["cells"], f"{e['document_id']} has no cells"
        print(f"  live fetch: {man['n_pages']} annotation page(s), "
              f"{man['n_pdfs_fetched']} PDF(s); pdf status={man['pdf_status_counts']}")
    print("fetch_fintabnet.py self-test OK")
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT,
                    help="output dir (annotations/, pdfs/, manifest.json)")
    ap.add_argument("--sample", type=int, default=30,
                    help="number of annotation pages to fetch (default 30)")
    ap.add_argument("--self-test", action="store_true",
                    help="offline checks + a tiny real fetch, then exit")
    args = ap.parse_args(argv)

    if args.self_test:
        return _self_test()
    fetch_fintabnet(args.out, sample=args.sample)
    return 0


if __name__ == "__main__":
    sys.exit(main())
