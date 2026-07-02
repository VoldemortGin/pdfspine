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
    - the matching source **PDFs** shipped in the original FinTabNet release on
      IBM's Data Asset eXchange CDN (``dax-cdn.cdn.appdomain.cloud``). That CDN is
      now **DECOMMISSIONED** — the whole ``cdn.appdomain.cloud`` DNS zone SERVFAILs
      from public resolvers (8.8.8.8 / 1.1.1.1; verified 2026-07-02) — so PDFs are
      fetched from a verbatim HuggingFace mirror of the 1.0.0 release
      (``Leon1207/FinTabNet``, ``archive.zip``: the same
      ``fintabnet/pdf/<TICKER>/<YEAR>/page_N.pdf`` tree plus the original
      ``FinTabNet_1.0.0_*.jsonl``). The annotation's ``pdf_bbox`` coordinates are
      in that PDF's page space, so a coordinate-matched PDF is REQUIRED — no
      image/HTML derivative substitutes; the zip mirror IS coordinate-matched
      (confirmed by bbox-IoU gold matching in ``tables_diff.py --gold``).

What this fetches (SMALL by design)
-----------------------------------
We do NOT pull the whole dataset. We stream the HF annotations tarball and extract
only the first ``--sample`` (default 30) ``*_tables.json`` files (each is one PDF
page), then for each extract only that one source PDF from the mirror zip via
HTTP-Range ZIP member reads (never the whole ~16.8 GB archive). Everything is
cached to ``--out`` so re-runs are cheap, and the corpus dir matches the gitignored
``conformance/gt/corpus-*/`` glob (we commit the SCRIPT + the REPORT, never the
data).

Network reality
---------------
Like the sibling fetchers we strip proxy env via ``ProxyHandler({})`` and use
HTTPS with retries (plus an env-proxy fallback route); failures are skipped and
the run continues. If HuggingFace is unreachable, the manifest records
``pdf_status`` per entry so the harness/report can state precisely what is missing
(a clean BLOCKED status, not a fabricated score). The dead FinTabNet CDN is never
contacted.

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
import struct
import sys
import tarfile
import time
import urllib.error
import urllib.request
import zlib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO_ROOT / "conformance" / "gt" / "corpus-fintabnet"

# HuggingFace — annotations (reachable, CDLA-Permissive-2.0).
HF_ANNO_URL = (
    "https://huggingface.co/datasets/bsmock/FinTabNet.c/resolve/main/"
    "FinTabNet.c-PDF_Annotations.tar.gz"
)
ANNO_LICENSE = "CDLA-Permissive-2.0"

# IBM Data Asset eXchange — the ORIGINAL FinTabNet PDF host (CDLA-Permissive-1.0).
# DECOMMISSIONED: the ``cdn.appdomain.cloud`` DNS zone SERVFAILs from public
# resolvers (8.8.8.8 / 1.1.1.1; verified 2026-07-02) — the host is gone for
# everyone, not blocked by any local network. Kept for provenance only; never
# contacted.
DAX_PDF_SOURCE = "https://dax-cdn.cdn.appdomain.cloud/dax-fintabnet/1.0.0/fintabnet/pdf/"

# Working PDF source: a verbatim HuggingFace mirror of the FinTabNet 1.0.0 release
# (same ``fintabnet/pdf/<TICKER>/<YEAR>/page_N.pdf`` tree + the original
# ``FinTabNet_1.0.0_*.jsonl``), served as ONE ~16.8 GB zip. We never download the
# archive: single-page PDFs are extracted with HTTP-Range ZIP member reads — the
# technique of ``fetch_robustness.py``'s ``_read_central_dir``/``_fetch_member``,
# re-implemented here because this >4 GiB archive additionally needs the per-entry
# zip64 extended-information extra field (0x0001) for member offsets, and because
# this module's dual-route (proxy-stripped + env-proxy) opener must be reused.
# License unchanged: the mirrored PDFs remain CDLA-Permissive-1.0.
MIRROR_ZIP_URL = (
    "https://huggingface.co/datasets/Leon1207/FinTabNet/resolve/main/archive.zip"
)
MIRROR_PDF_PREFIX = "fintabnet/pdf/"
MIRROR_INDEX_CACHE = (
    REPO_ROOT / "conformance" / "gt" / "cache" / "fintabnet-mirror-index.json"
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
         stream: bool = False, headers: dict[str, str] | None = None):
    """GET ``url``. If ``stream`` return the open response (caller reads); else
    return its full body bytes. Retries transient errors; 404/403/406 raise.

    Tries the proxy-stripped opener first, then the env-proxy opener (whichever
    connected last is tried first on subsequent calls). ``headers`` are merged on
    top of the default User-Agent (used for the mirror zip's Range requests).
    """
    openers = _openers()
    order = sorted(range(len(openers)), key=lambda i: i != _PREFERRED_OPENER[0])
    last: Exception | None = None
    for attempt in range(1, retries + 1):
        for oi in order:
            opener = openers[oi]
            try:
                req = urllib.request.Request(
                    url, headers={"User-Agent": _UA, **(headers or {})}
                )
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
# Source PDF — HTTP-Range extraction from the HF mirror zip.
# Approach shared with ``fetch_robustness.py`` (EOCD -> central directory ->
# per-member Range reads), extended with zip64 per-entry offsets (>4 GiB archive).
# --------------------------------------------------------------------------- #
def _range_get(url: str, start: int, end: int) -> bytes:
    """Fetch bytes ``[start, end]`` (inclusive) via ``_get``'s dual-route opener."""
    data = _get(url, headers={"Range": f"bytes={start}-{end}"})
    want = end - start + 1
    if len(data) != want:
        raise OSError(f"range {start}-{end}: got {len(data)} bytes, want {want}")
    return data


def _zip_total_size(url: str) -> int:
    """Total archive size, from a 1-byte Range probe's ``Content-Range`` header."""
    resp = _get(url, headers={"Range": "bytes=0-0"}, stream=True)
    try:
        cr = resp.headers.get("Content-Range", "")
    finally:
        with __import__("contextlib").suppress(Exception):
            resp.close()
    if "/" not in cr:
        raise OSError(f"no Content-Range in probe response (got {cr!r})")
    return int(cr.rsplit("/", 1)[1])


def _parse_mirror_index(url: str) -> dict[str, list[int]]:
    """Parse the mirror zip's central directory via HTTP Range.

    Returns ``{member_name: [method, comp_size, uncomp_size, lfh_off]}`` for the
    ``fintabnet/pdf/**.pdf`` members only. Chain: EOCD -> zip64 EOCD locator ->
    zip64 EOCD -> central directory (~9.5 MB, fetched in 4 MiB chunks), honoring
    the zip64 extended-information extra field (id 0x0001) for any size/offset
    stored as a ``0xFFFFFFFF`` placeholder.
    """
    total = _zip_total_size(url)
    tail_len = min(total, 65557 + 20)  # EOCD (+comment) + zip64 locator
    tail = _range_get(url, total - tail_len, total - 1)
    i = tail.rfind(b"PK\x05\x06")
    if i < 0:
        raise OSError("mirror zip: EOCD signature not found")
    (_, _, _, _, n_rec, cd_size, cd_off, _) = struct.unpack_from("<IHHHHIIH", tail, i)
    if cd_off == 0xFFFFFFFF or cd_size == 0xFFFFFFFF or n_rec == 0xFFFF:
        j = tail.rfind(b"PK\x06\x07", 0, i)
        if j < 0:
            raise OSError("mirror zip: zip64 EOCD locator not found")
        (_, _, eocd64_off, _) = struct.unpack_from("<IIQI", tail, j)
        z64 = _range_get(url, eocd64_off, eocd64_off + 55)
        if not z64.startswith(b"PK\x06\x06"):
            raise OSError("mirror zip: bad zip64 EOCD record")
        cd_size, cd_off = struct.unpack_from("<QQ", z64, 40)

    chunks: list[bytes] = []
    pos = cd_off
    step = 4 << 20
    while pos < cd_off + cd_size:
        end = min(pos + step, cd_off + cd_size) - 1
        chunks.append(_range_get(url, pos, end))
        pos = end + 1
    cd = b"".join(chunks)

    members: dict[str, list[int]] = {}
    p = 0
    while p + 46 <= len(cd) and cd[p:p + 4] == b"PK\x01\x02":
        (_, _, _, _, method, _, _, _, csize, usize,
         nlen, elen, clen, _, _, _, lfh_off) = struct.unpack_from("<IHHHHHHIIIHHHHHII", cd, p)
        name = cd[p + 46:p + 46 + nlen].decode("utf-8", "replace")
        if 0xFFFFFFFF in (csize, usize, lfh_off):
            extra = cd[p + 46 + nlen:p + 46 + nlen + elen]
            q = 0
            while q + 4 <= len(extra):
                hid, hsz = struct.unpack_from("<HH", extra, q)
                if hid == 0x0001:  # zip64 extended information
                    r = q + 4
                    if usize == 0xFFFFFFFF:
                        usize = struct.unpack_from("<Q", extra, r)[0]
                        r += 8
                    if csize == 0xFFFFFFFF:
                        csize = struct.unpack_from("<Q", extra, r)[0]
                        r += 8
                    if lfh_off == 0xFFFFFFFF:
                        lfh_off = struct.unpack_from("<Q", extra, r)[0]
                        r += 8
                    break
                q += 4 + hsz
        if name.startswith(MIRROR_PDF_PREFIX) and name.endswith(".pdf"):
            members[name] = [method, csize, usize, lfh_off]
        p += 46 + nlen + elen + clen
    if not members:
        raise OSError("mirror zip: no fintabnet/pdf/*.pdf members parsed")
    return members


# In-process memo of the mirror index (also cached on disk across runs).
_MIRROR_INDEX: dict[str, list[int]] | None = None


def _mirror_index(*, verbose: bool = False) -> dict[str, list[int]]:
    """The mirror zip's pdf-member index; memoized in-process, cached on disk.

    Disk cache: ``conformance/gt/cache/fintabnet-mirror-index.json`` (gitignored
    with the rest of ``conformance/gt/cache/``). A corrupt cache is re-parsed.
    """
    global _MIRROR_INDEX
    if _MIRROR_INDEX is not None:
        return _MIRROR_INDEX
    if MIRROR_INDEX_CACHE.exists():
        try:
            cached = json.loads(MIRROR_INDEX_CACHE.read_text(encoding="utf-8"))
        except Exception:  # noqa: BLE001
            cached = None
        if cached:
            _MIRROR_INDEX = cached
            return _MIRROR_INDEX
    if verbose:
        print("  parsing mirror-zip central directory (one-time, ~10 MB) ...", flush=True)
    _MIRROR_INDEX = _parse_mirror_index(MIRROR_ZIP_URL)
    MIRROR_INDEX_CACHE.parent.mkdir(parents=True, exist_ok=True)
    MIRROR_INDEX_CACHE.write_text(json.dumps(_MIRROR_INDEX), encoding="utf-8")
    if verbose:
        print(f"  mirror index: {len(_MIRROR_INDEX)} PDF members -> {MIRROR_INDEX_CACHE}")
    return _MIRROR_INDEX


def _fetch_mirror_member(name: str, method: int, csize: int, usize: int,
                         lfh_off: int) -> bytes | None:
    """Extract one zip member (usually a single Range read: header + data).

    Returns the decompressed bytes, or ``None`` if the member is not a valid
    stored/deflated PDF matching the indexed sizes.
    """
    if method not in (0, 8):  # stored / deflate only
        return None
    # 30-byte local header + name + extra (256 B slack) + compressed data.
    blob = _range_get(MIRROR_ZIP_URL, lfh_off,
                      lfh_off + 30 + len(name.encode("utf-8")) + 256 + csize)
    if not blob.startswith(b"PK\x03\x04"):
        return None
    nlen, elen = struct.unpack_from("<HH", blob, 26)
    data = blob[30 + nlen + elen:30 + nlen + elen + csize]
    if len(data) != csize:  # local extra field larger than the slack: exact re-read
        data_off = lfh_off + 30 + nlen + elen
        data = _range_get(MIRROR_ZIP_URL, data_off, data_off + csize - 1)
    if method == 8:
        try:
            data = zlib.decompress(data, -zlib.MAX_WBITS)  # raw deflate
        except zlib.error:
            return None
    if len(data) != usize or not data.startswith(b"%PDF"):
        return None
    return data


def fetch_pdf_for(table_anno: dict, pdf_dir: Path, *, verbose: bool = False) -> tuple[Path | None, str]:
    """Fetch the single source PDF for one annotation. Returns (path|None, status).

    ``status`` is one of: ``"cached"``, ``"ok"``, ``"mirror-missing"``,
    ``"not-pdf"``, ``"unreachable"``, ``"no-path"``. The PDF is extracted from the
    HF mirror zip (``MIRROR_ZIP_URL``) at ``fintabnet/pdf/<TICKER>/<YEAR>/page_N.pdf``
    and validated (``%PDF`` magic + exact uncompressed size). The decommissioned
    DAX CDN is never contacted.
    """
    rel, fname = _pdf_components(table_anno)
    if not rel or not fname:
        return None, "no-path"
    doc_id = table_anno.get("document_id") or rel.replace("/", "_")
    dest = pdf_dir / f"{doc_id}.pdf"
    if dest.exists() and dest.stat().st_size > 0 and dest.read_bytes()[:4] == b"%PDF":
        return dest, "cached"

    try:
        index = _mirror_index(verbose=verbose)
    except Exception:  # noqa: BLE001
        return None, "unreachable"
    entry = index.get(MIRROR_PDF_PREFIX + rel)
    if entry is None:
        return None, "mirror-missing"
    method, csize, usize, lfh_off = entry
    try:
        data = _fetch_mirror_member(MIRROR_PDF_PREFIX + rel, method, csize, usize, lfh_off)
    except (urllib.error.URLError, urllib.error.HTTPError, OSError, TimeoutError,
            http.client.HTTPException):
        return None, "unreachable"
    if data is None:
        return None, "not-pdf"
    pdf_dir.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(data)
    return dest, "ok"


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
        "pdf_source": MIRROR_ZIP_URL,
        "pdf_source_original": DAX_PDF_SOURCE + " (DECOMMISSIONED: DNS zone gone)",
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
        log("NOTE: 0 source PDFs fetched — annotations are present but the HF "
            "mirror zip (Leon1207/FinTabNet archive.zip) was unreachable or is "
            "missing the members. Scoring is BLOCKED on the PDFs; see the report.")
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
