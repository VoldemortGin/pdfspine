#!/usr/bin/env python3
"""Fetch a small, permissively-licensed PMC Open Access sample (PDF + JATS .nxml).

Part of the OBJECTIVE ground-truth accuracy subsystem: we need *real* PDFs whose
true text is independently knowable. PMC Open Access articles ship both a PDF and
the publisher's JATS XML (``.nxml``); the XML gives us ground-truth body text
(resolved separately by ``jats_text``), and the PDF is what every extractor
(pdfspine, fitz, pdfminer) is scored against.

Only CC0 / CC BY articles are collected (excluding CC BY-NC*, CC BY-ND, and
"Other") so the corpus is permissively licensed.

Network reality (verified June 2026):
* The PMC FTP service moved all legacy bulk files into ``/pub/pmc/deprecated/``;
  the old top-level ``oa_package/`` 404s. Tarballs now live at
  ``https://ftp.ncbi.nlm.nih.gov/pub/pmc/deprecated/<File>`` where ``<File>`` is
  the ``oa_package/xx/yy/PMC#######.tar.gz`` path from the file-list CSV.
* The file-list CSV is ~600 MB; it is STREAMED and only the first chunk of rows
  is read (we never download it whole).
* As a fallback we query the OA Web Service
  ``oa.fcgi?id=PMC#######`` for the exact tgz href and convert ``ftp://`` ->
  ``https://`` to download over HTTPS.
* A local proxy on this machine breaks TLS to some hosts; like the sibling
  ``fetch_corpus.py`` we strip proxy env via ``ProxyHandler({})``.

Stdlib only (urllib/csv/tarfile/io/json/argparse) — the project venv has no
``requests``. Downloads are cached so re-runs are cheap.

CLI::

    env -u CONDA_PREFIX .venv/bin/python conformance/gt/pmc_fetch.py \
        --out conformance/gt/corpus-pmc --n 25

API::

    from conformance.gt.pmc_fetch import fetch_commercial_sample
    fetch_commercial_sample(Path("conformance/gt/corpus-pmc"), n=25)
"""

from __future__ import annotations

import argparse
import csv
import http.client
import io
import json
import re
import sys
import tarfile
import time
import urllib.error
import urllib.request
from pathlib import Path

# --------------------------------------------------------------------------- #
# Constants
# --------------------------------------------------------------------------- #
REPO_ROOT = Path(__file__).resolve().parents[2]

# Commercial-use ("oa_comm") file list. Columns:
#   File, Article Citation, Accession ID, Last Updated (...), PMID, License
FILE_LIST_URL = "https://ftp.ncbi.nlm.nih.gov/pub/pmc/deprecated/oa_comm_use_file_list.csv"
# Full subset list — same column layout — used if the comm list is unreachable.
FILE_LIST_FALLBACK_URL = "https://ftp.ncbi.nlm.nih.gov/pub/pmc/deprecated/oa_file_list.csv"

# Tarballs were moved under deprecated/; <File> is the oa_package/xx/yy/PMC*.tar.gz path.
PACKAGE_BASE = "https://ftp.ncbi.nlm.nih.gov/pub/pmc/deprecated/"
# OA Web Service (per-article download links).
OA_FCGI = "https://www.ncbi.nlm.nih.gov/pmc/utils/oa/oa.fcgi?id="

_UA = "pdfspine-conformance/1.0 (+ground-truth corpus; pubmedcentral OA subset)"

# Permissive licenses we accept (exact match against the CSV License column).
DEFAULT_LICENSES: tuple[str, ...] = ("CC0", "CC BY")

# How many CSV rows to scan for candidates while reaching ``n`` good articles.
_CANDIDATE_ROWS = 60
_REQUEST_DELAY = 0.5  # politeness delay between article downloads (seconds)
_MAX_RETRIES = 3


# --------------------------------------------------------------------------- #
# HTTP helpers (proxy-stripped, retrying)
# --------------------------------------------------------------------------- #
def _opener() -> urllib.request.OpenerDirector:
    """Build an opener with proxies disabled (local proxy breaks TLS to NCBI)."""
    return urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _request(url: str) -> urllib.request.Request:
    return urllib.request.Request(url, headers={"User-Agent": _UA})


def _get_bytes(url: str, timeout: int = 120, retries: int = _MAX_RETRIES) -> bytes:
    """GET ``url`` fully, retrying on transient network errors. Raises on failure."""
    opener = _opener()
    last: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            with opener.open(_request(url), timeout=timeout) as resp:
                return resp.read()
        except (urllib.error.URLError, OSError, TimeoutError, http.client.IncompleteRead) as exc:
            last = exc
            if isinstance(exc, urllib.error.HTTPError) and exc.code in (404, 403):
                raise  # not transient; let caller fall back
            if attempt < retries:
                time.sleep(min(2.0 * attempt, 5.0))
    assert last is not None
    raise last


# --------------------------------------------------------------------------- #
# CSV streaming (read only the first chunk of rows)
# --------------------------------------------------------------------------- #
def _stream_candidates(
    url: str,
    licenses: tuple[str, ...],
    max_rows: int,
    timeout: int = 60,
) -> list[dict]:
    """Stream the file-list CSV and collect up to ``max_rows`` permissive rows.

    Returns dicts: {"file": <oa_package path>, "pmcid": PMCID, "license": L}.
    Reads the response incrementally and stops as soon as enough rows are found
    or a generous line budget is exhausted — never downloads the whole ~600 MB CSV.
    """
    opener = _opener()
    accepted = set(licenses)
    out: list[dict] = []
    resp = opener.open(_request(url), timeout=timeout)
    try:
        reader = csv.reader(_iter_text_lines(resp))
        header = next(reader, None)
        if not header:
            return out
        col = {name.strip(): i for i, name in enumerate(header)}
        # Robust column lookup (header text varies slightly across list variants).
        i_file = _find_col(col, ("File",))
        i_pmcid = _find_col(col, ("Accession ID", "PMCID"))
        i_lic = _find_col(col, ("License",))
        if i_file is None or i_pmcid is None or i_lic is None:
            return out
        scanned = 0
        for row in reader:
            scanned += 1
            if scanned > 2_000_000:  # hard safety budget; we never reach this
                break
            if len(row) <= max(i_file, i_pmcid, i_lic):
                continue
            lic = row[i_lic].strip()
            if lic not in accepted:
                continue
            out.append({"file": row[i_file].strip(), "pmcid": row[i_pmcid].strip(), "license": lic})
            if len(out) >= max_rows:
                break
    finally:
        resp.close()
    return out


def _find_col(col: dict[str, int], names: tuple[str, ...]) -> int | None:
    for n in names:
        if n in col:
            return col[n]
    return None


def _iter_text_lines(resp) -> "iter[str]":  # type: ignore[type-arg]
    """Yield decoded text lines from a binary HTTP response, streaming in chunks."""
    buf = b""
    while True:
        chunk = resp.read(65536)
        if not chunk:
            break
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            yield line.decode("utf-8", "replace")
    if buf:
        yield buf.decode("utf-8", "replace")


# --------------------------------------------------------------------------- #
# Per-article tarball resolution + extraction
# --------------------------------------------------------------------------- #
def _oa_fcgi_tgz_href(pmcid: str, timeout: int = 60) -> str | None:
    """Query the OA Web Service for the article's tgz href (https-converted)."""
    try:
        xml = _get_bytes(OA_FCGI + pmcid, timeout=timeout, retries=2).decode("utf-8", "replace")
    except Exception:  # noqa: BLE001
        return None
    m = re.search(r'format="tgz"[^>]*href="([^"]+)"', xml)
    if not m:
        return None
    href = m.group(1)
    return href.replace("ftp://ftp.ncbi.nlm.nih.gov/", "https://ftp.ncbi.nlm.nih.gov/")


def _download_tarball(file_path: str, pmcid: str) -> bytes | None:
    """Download the article tarball, preferring the deprecated/<File> path.

    Falls back to oa.fcgi (-> exact tgz href, ftp->https) on 404.
    """
    primary = PACKAGE_BASE + file_path.lstrip("/")
    try:
        return _get_bytes(primary)
    except urllib.error.HTTPError as exc:
        if exc.code not in (404, 403):
            return None
    except Exception:  # noqa: BLE001
        return None
    href = _oa_fcgi_tgz_href(pmcid)
    if not href:
        return None
    # If the fcgi href still points at the old top-level oa_package path, rewrite
    # it into the deprecated/ tree where the files actually live now.
    if "/pub/pmc/oa_package/" in href:
        href = href.replace("/pub/pmc/oa_package/", "/pub/pmc/deprecated/oa_package/")
    try:
        return _get_bytes(href)
    except Exception:  # noqa: BLE001
        return None


def _pick_main_pdf(names: list[str]) -> str | None:
    """Choose the main article PDF (not a supplementary figure file).

    Heuristics: prefer a member whose basename equals ``<PMCID>.pdf``; otherwise
    the basename with the fewest dotted segments (supplementary files look like
    ``pbio.0000013.sg001.pdf``); tie-break on shortest basename.
    """
    pdfs = [n for n in names if n.lower().endswith(".pdf")]
    if not pdfs:
        return None
    pmc = re.compile(r"PMC\d+\.pdf$", re.IGNORECASE)
    for n in pdfs:
        if pmc.search(n.replace("\\", "/").split("/")[-1]):
            return n

    def score(n: str) -> tuple[int, int]:
        base = n.replace("\\", "/").split("/")[-1]
        return (base.count("."), len(base))

    return sorted(pdfs, key=score)[0]


def _extract_pdf_and_nxml(data: bytes) -> tuple[bytes, bytes] | None:
    """Extract (pdf_bytes, nxml_bytes) from a gzipped article tarball, or None."""
    try:
        tf = tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")
    except (tarfile.TarError, OSError):
        return None
    try:
        names = tf.getnames()
        nxmls = [n for n in names if n.lower().endswith(".nxml")]
        pdf_name = _pick_main_pdf(names)
        if not pdf_name or not nxmls:
            return None
        pdf_member = tf.extractfile(pdf_name)
        nxml_member = tf.extractfile(sorted(nxmls, key=len)[0])
        if pdf_member is None or nxml_member is None:
            return None
        pdf_bytes = pdf_member.read()
        nxml_bytes = nxml_member.read()
    finally:
        tf.close()
    if not pdf_bytes.startswith(b"%PDF"):
        return None
    return pdf_bytes, nxml_bytes


# --------------------------------------------------------------------------- #
# Public API
# --------------------------------------------------------------------------- #
def fetch_commercial_sample(
    out_dir: Path,
    n: int = 25,
    licenses: tuple[str, ...] = DEFAULT_LICENSES,
    *,
    candidate_rows: int = _CANDIDATE_ROWS,
    verbose: bool = True,
) -> list[dict]:
    """Fetch up to ``n`` permissive PMC OA articles (PDF + .nxml) into ``out_dir``.

    Returns the manifest entries (also written to ``out_dir/manifest.json``):
    ``{"name": PMCID, "pdf": <abs path>, "nxml": <abs path>, "license": L}``.
    Already-downloaded articles are reused (cache); failures are skipped.
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    def log(msg: str) -> None:
        if verbose:
            print(msg, flush=True)

    # 1. Stream candidate rows from the file list (comm list, then full list).
    candidates: list[dict] = []
    for list_url in (FILE_LIST_URL, FILE_LIST_FALLBACK_URL):
        try:
            log(f"Streaming file list: {list_url}")
            # Scan enough rows to comfortably reach n good articles.
            candidates = _stream_candidates(list_url, tuple(licenses), max(candidate_rows, n * 2))
            if candidates:
                break
        except Exception as exc:  # noqa: BLE001
            log(f"  file list error ({type(exc).__name__}: {exc}); trying fallback")
    if not candidates:
        log("No candidates found (network or list unavailable).")
        _write_manifest(out_dir, _load_existing(out_dir))
        return _load_existing(out_dir)

    log(f"Got {len(candidates)} permissive candidates; need {n} good articles.")

    # 2. Download / extract each, caching to disk.
    manifest: list[dict] = []
    seen_pmcids: set[str] = set()
    for cand in candidates:
        if len(manifest) >= n:
            break
        pmcid = cand["pmcid"]
        if not pmcid or pmcid in seen_pmcids:
            continue
        seen_pmcids.add(pmcid)
        pdf_path = out_dir / f"{pmcid}.pdf"
        nxml_path = out_dir / f"{pmcid}.nxml"

        if pdf_path.exists() and nxml_path.exists() and pdf_path.stat().st_size > 0:
            log(f"  CACHED {pmcid} ({cand['license']})")
            manifest.append(_entry(pmcid, pdf_path, nxml_path, cand["license"]))
            continue

        time.sleep(_REQUEST_DELAY)
        data = _download_tarball(cand["file"], pmcid)
        if data is None:
            log(f"  SKIP   {pmcid} — tarball unavailable")
            continue
        extracted = _extract_pdf_and_nxml(data)
        if extracted is None:
            log(f"  SKIP   {pmcid} — tarball missing pdf or nxml")
            continue
        pdf_bytes, nxml_bytes = extracted
        pdf_path.write_bytes(pdf_bytes)
        nxml_path.write_bytes(nxml_bytes)
        log(f"  OK     {pmcid} ({cand['license']}) pdf={len(pdf_bytes):,}B nxml={len(nxml_bytes):,}B "
            f"[{len(manifest) + 1}/{n}]")
        manifest.append(_entry(pmcid, pdf_path, nxml_path, cand["license"]))

    _write_manifest(out_dir, manifest)
    log(f"\nFetched {len(manifest)} article(s) -> {out_dir / 'manifest.json'}")
    return manifest


def _entry(pmcid: str, pdf: Path, nxml: Path, license_: str) -> dict:
    return {
        "name": pmcid,
        "pdf": str(pdf.resolve()),
        "nxml": str(nxml.resolve()),
        "license": license_,
    }


def _load_existing(out_dir: Path) -> list[dict]:
    """Rebuild a manifest from whatever PDF+nxml pairs already exist on disk."""
    entries: list[dict] = []
    for pdf in sorted(out_dir.glob("PMC*.pdf")):
        nxml = pdf.with_suffix(".nxml")
        if nxml.exists():
            entries.append(_entry(pdf.stem, pdf, nxml, "unknown"))
    return entries


def _write_manifest(out_dir: Path, manifest: list[dict]) -> None:
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")


# --------------------------------------------------------------------------- #
# Offline self-test (no full downloads)
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    """Light self-test: parse a few CSV rows + verify URL construction.

    Never downloads articles. If the network is fully unavailable, prints a clear
    diagnostic and returns 0 (must not hard-fail the autonomous build).
    """
    # URL construction (offline, deterministic) ---------------------------------
    assert OA_FCGI + "PMC176545" == "https://www.ncbi.nlm.nih.gov/pmc/utils/oa/oa.fcgi?id=PMC176545", \
        "oa.fcgi URL construction wrong"
    sample_file = "oa_package/81/5e/PMC193604.tar.gz"
    assert PACKAGE_BASE + sample_file == \
        "https://ftp.ncbi.nlm.nih.gov/pub/pmc/deprecated/oa_package/81/5e/PMC193604.tar.gz", \
        "package URL construction wrong"
    # ftp->https conversion used by the fcgi fallback
    ftp_href = "ftp://ftp.ncbi.nlm.nih.gov/pub/pmc/oa_package/81/5e/PMC193604.tar.gz"
    https_href = ftp_href.replace("ftp://ftp.ncbi.nlm.nih.gov/", "https://ftp.ncbi.nlm.nih.gov/")
    assert https_href.startswith("https://ftp.ncbi.nlm.nih.gov/"), "ftp->https conversion wrong"

    # main-PDF selection (offline)
    names = ["PMC193604/pbio.0000013.sg001.pdf", "PMC193604/pbio.0000013.pdf",
             "PMC193604/pbio.0000013.nxml"]
    assert _pick_main_pdf(names) == "PMC193604/pbio.0000013.pdf", "main-PDF heuristic wrong"
    assert _pick_main_pdf(["a/PMC42.pdf", "a/x.sup.pdf"]) == "a/PMC42.pdf", "PMCID-named PDF not preferred"

    # CSV column parsing on an in-memory sample (no network) --------------------
    sample_csv = (
        "File,Article Citation,Accession ID,Last Updated (YYYY-MM-DD HH:MM:SS),PMID,License\n"
        "oa_package/e6/58/PMC176545.tar.gz,PLoS Biol. 2003;1(1):e5,PMC176545,2023-01-25,12929205,CC BY\n"
        "oa_package/aa/bb/PMC999001.tar.gz,J X. 2020;1:1,PMC999001,2020-01-01,1,CC BY-NC\n"
        "oa_package/cc/dd/PMC999002.tar.gz,J Y. 2021;2:2,PMC999002,2021-01-01,2,CC0\n"
        "oa_package/ee/ff/PMC999003.tar.gz,J Z. 2022;3:3,PMC999003,2022-01-01,3,Other\n"
    )

    class _FakeResp:
        def __init__(self, data: bytes) -> None:
            self._buf = io.BytesIO(data)

        def read(self, size: int = -1) -> bytes:
            return self._buf.read(size)

    reader = csv.reader(_iter_text_lines(_FakeResp(sample_csv.encode("utf-8"))))
    header = next(reader)
    col = {name.strip(): i for i, name in enumerate(header)}
    i_file = _find_col(col, ("File",))
    i_pmcid = _find_col(col, ("Accession ID", "PMCID"))
    i_lic = _find_col(col, ("License",))
    assert i_file == 0 and i_pmcid == 2 and i_lic == 5, f"column indices wrong: {col}"
    accepted = {"CC0", "CC BY"}
    parsed = [r for r in reader if len(r) > i_lic and r[i_lic].strip() in accepted]
    assert len(parsed) == 2, f"expected 2 permissive rows, got {len(parsed)}"
    assert parsed[0][i_pmcid] == "PMC176545" and parsed[1][i_pmcid] == "PMC999002"
    assert {r[i_lic] for r in parsed} == {"CC BY", "CC0"}, "license filtering wrong"

    # Live network probe — informative only, never fatal -----------------------
    live = 0
    try:
        rows = _stream_candidates(FILE_LIST_URL, DEFAULT_LICENSES, max_rows=20, timeout=30)
        live = len(rows)
        if rows:
            print(f"  live probe: streamed {live} permissive rows from the OA file list "
                  f"(e.g. {rows[0]['pmcid']} / {rows[0]['license']})")
        else:
            print("  live probe: connected but found no permissive rows in first chunk")
    except Exception as exc:  # noqa: BLE001
        print(f"  live probe skipped (network unavailable: {type(exc).__name__}: {exc}); "
              "offline checks still passed")

    total_parsed = len(parsed) + live
    print(f"pmc_fetch.py self-test OK (parsed {total_parsed} candidate rows)")
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, default=REPO_ROOT / "conformance" / "gt" / "corpus-pmc",
                    help="output directory for PDFs + .nxml + manifest.json")
    ap.add_argument("--n", type=int, default=25, help="number of good articles to fetch")
    ap.add_argument("--licenses", default="CC0,CC BY",
                    help="comma-separated allowed licenses (exact CSV License values)")
    ap.add_argument("--self-test", action="store_true", help="run offline self-test and exit")
    args = ap.parse_args(argv)

    if args.self_test:
        return _self_test()

    licenses = tuple(s.strip() for s in args.licenses.split(",") if s.strip())
    fetch_commercial_sample(args.out, n=args.n, licenses=licenses)
    return 0


if __name__ == "__main__":
    sys.exit(main())
