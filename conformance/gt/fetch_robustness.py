#!/usr/bin/env python3
"""Fetch a large, diverse real-world PDF robustness corpus for pdfspine.

This is a FETCH-ONLY tool. It downloads a broad, heterogeneous set of
real-world PDFs (varied producers, eras, encodings, malformities) so the
harness can later (a) stress never-panic robustness and (b) run an
pdfspine-vs-fitz differential extraction. NO ground truth is produced here --
the scoring happens elsewhere in the main loop.

Sources
-------
Primary -- GovDocs1 (Digital Corpora): ~1M files harvested from US ``.gov``
domains, explicitly free of copyright/privacy encumbrance. Organized into
"threads": zip bundles ``000.zip``..``999.zip``, each ~1000 mixed-type files
(PDF, DOC, XLS, HTML, ...). Each thread zip is ~300-500 MB. Two equivalent
hosts are tried (whichever responds first wins):

  * https://downloads.digitalcorpora.org/corpora/files/govdocs1/zipfiles/NNN.zip
  * https://digitalcorpora.s3.amazonaws.com/corpora/files/govdocs1/zipfiles/NNN.zip

Fallback -- SafeDocs CC-MAIN-2021-31-PDF-UNTRUNCATED: 1000 real-world web
PDFs per zip (mixed copyright -- fetch-only, never committed). The S3 layout
nests a range directory, e.g.::

  .../CC-MAIN-2021-31-PDF-UNTRUNCATED/zipfiles/0000-0999/0000.zip

Both sources are downloaded only; only ``*.pdf`` members validated against the
``%PDF`` magic byte are extracted. The output directory
``conformance/gt/corpus-robustness/`` is already gitignored
(``conformance/gt/corpus-*/``) -- only this fetcher script is committed.

Range-based selective extraction
--------------------------------
A full thread zip is ~300-500 MB but only a fraction of that is the PDFs we
keep. Rather than download the whole archive, this fetcher uses HTTP Range
requests against these range-capable hosts to extract only what it needs:

  1. Range-fetch the zip tail to locate the End-Of-Central-Directory record
     and read the central directory (typically <100 KB).
  2. Parse the central directory for ``*.pdf`` members and their byte offsets.
  3. For each candidate, Range-fetch just that member's local header + data,
     inflate it in memory, and validate the ``%PDF`` magic byte.

This turns a ~486 MB download into a few tens of MB for N=250 PDFs, which
matters on slow/throttled links. A parsed central directory is cached to disk
so re-runs skip the tail round-trip.

Network notes
-------------
* A local HTTP(S) proxy on this machine breaks TLS to several hosts, so all
  requests strip proxy settings (``ProxyHandler({})``), mirroring
  ``conformance/fetch_corpus.py``.
* Per-member fetches use a bounded socket timeout; a stalled or non-range host
  is skipped and the run continues. One bad member never fails the whole run.

Usage (from repo root)::

    env -u CONDA_PREFIX python conformance/gt/fetch_robustness.py \
        --out conformance/gt/corpus-robustness --n 250
    env -u CONDA_PREFIX python conformance/gt/fetch_robustness.py \
        --out conformance/gt/corpus-robustness --n 250 --source safedocs
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
import urllib.error
import urllib.request
import zlib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO_ROOT / "conformance" / "gt" / "corpus-robustness"
# Cache downloaded thread zips here so re-runs don't re-download ~500 MB.
CACHE_DIR = REPO_ROOT / "conformance" / "gt" / "cache" / "robustness-zips"

_UA = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
)

# --------------------------------------------------------------------------- #
# Source definitions.
#
# Each source provides a list of candidate thread URLs. The first host that
# responds to a Range probe is used. ``num`` is the zero-padded thread index.
# --------------------------------------------------------------------------- #


def _govdocs1_urls(num: int) -> list[str]:
    """Candidate URLs for GovDocs1 thread ``num`` (e.g. 0 -> ``000.zip``).

    The S3 origin is listed first: the ``downloads.digitalcorpora.org`` mirror
    was observed to throttle bulk transfers to ~50 KiB/s, while the S3 bucket
    serves at multi-MiB/s. Both are equivalent content.
    """
    name = f"{num:03d}.zip"
    return [
        f"https://digitalcorpora.s3.amazonaws.com/corpora/files/govdocs1/zipfiles/{name}",
        f"https://downloads.digitalcorpora.org/corpora/files/govdocs1/zipfiles/{name}",
    ]


def _safedocs_urls(num: int) -> list[str]:
    """Candidate URLs for SafeDocs thread ``num`` (nested range directory).

    SafeDocs zips live under a ``NNNN-MMMM/`` range subdirectory, e.g. thread
    0 is ``zipfiles/0000-0999/0000.zip``.
    """
    name = f"{num:04d}.zip"
    lo = (num // 1000) * 1000
    rng = f"{lo:04d}-{lo + 999:04d}"
    base = "corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/zipfiles"
    return [
        f"https://digitalcorpora.s3.amazonaws.com/{base}/{rng}/{name}",
        f"https://downloads.digitalcorpora.org/{base}/{rng}/{name}",
    ]


_SOURCES: dict[str, dict] = {
    "govdocs1": {
        "urls": _govdocs1_urls,
        "license": "public-domain",
        # Threads to walk, in order, until we have enough PDFs.
        "threads": [0, 1],
    },
    "safedocs": {
        "urls": _safedocs_urls,
        "license": "web-mixed-fetch-only",
        "threads": [0, 1],
    },
}


def _opener() -> urllib.request.OpenerDirector:
    """An opener with proxy env stripped (the local proxy breaks TLS)."""
    return urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _probe(url: str, timeout: int = 30) -> int | None:
    """Range-probe ``url``. Return the total size in bytes, or ``None`` on failure."""
    req = urllib.request.Request(
        url, headers={"User-Agent": _UA, "Range": "bytes=0-1"}
    )
    try:
        with _opener().open(req, timeout=timeout) as resp:
            cr = resp.headers.get("Content-Range", "")
            head = resp.read(2)
            if not head.startswith(b"PK"):
                return None
            # Content-Range: "bytes 0-1/486762106" -> total after the slash.
            if "/" in cr:
                try:
                    return int(cr.rsplit("/", 1)[1])
                except ValueError:
                    pass
            cl = resp.headers.get("Content-Length")
            return int(cl) if cl and cl.isdigit() else 0
    except (urllib.error.URLError, urllib.error.HTTPError, OSError, TimeoutError):
        return None


def _pick_url(candidates: list[str]) -> tuple[str, int] | None:
    """Probe candidate URLs; return ``(url, total_size)`` for the first that works."""
    for url in candidates:
        total = _probe(url)
        if total is not None:
            return url, total
    return None


def _range(url: str, start: int, end: int, *, timeout: int = 60) -> bytes:
    """Range-fetch ``[start, end]`` (inclusive) from ``url``. Raises on failure."""
    req = urllib.request.Request(
        url, headers={"User-Agent": _UA, "Range": f"bytes={start}-{end}"}
    )
    with _opener().open(req, timeout=timeout) as resp:
        return resp.read()


# ZIP record signatures and struct layouts.
_EOCD_SIG = b"PK\x05\x06"
_EOCD64_LOC_SIG = b"PK\x06\x07"
_CDH_SIG = b"PK\x01\x02"
_LFH_SIG = b"PK\x03\x04"


def _read_central_dir(url: str, total: int) -> list[dict]:
    """Range-fetch and parse the ZIP central directory of ``url``.

    Returns a list of member dicts ``{name, method, comp_size, lfh_off}`` for
    deflate/stored members (the only methods we inflate). Returns ``[]`` if the
    archive uses an unsupported layout (zip64 with too-large offsets, etc.).
    """
    # The EOCD lives in the last (22 + comment) bytes; the comment is <= 65535.
    tail_len = min(total, 65557)
    tail = _range(url, total - tail_len, total - 1)
    i = tail.rfind(_EOCD_SIG)
    if i < 0:
        return []
    eocd = tail[i : i + 22]
    (_, _, _, _, cd_count, cd_size, cd_off, _) = struct.unpack("<IHHHHIIH", eocd)
    # Zip64: if any field is maxed out, find the zip64 EOCD locator just before.
    if cd_off == 0xFFFFFFFF or cd_count == 0xFFFF or cd_size == 0xFFFFFFFF:
        j = tail.rfind(_EOCD64_LOC_SIG, 0, i)
        if j < 0:
            return []
        (_, _, eocd64_off, _) = struct.unpack("<IIQI", tail[j : j + 20])
        z64 = _range(url, eocd64_off, eocd64_off + 55)
        if not z64.startswith(b"PK\x06\x06"):
            return []
        (cd_size, cd_off) = struct.unpack("<QQ", z64[40:56])
    cd = _range(url, cd_off, cd_off + cd_size - 1)
    members: list[dict] = []
    p = 0
    end = len(cd)
    while p + 46 <= end and cd[p : p + 4] == _CDH_SIG:
        (
            _, _, _, _, method, _, _, _, comp_size, _,
            name_len, extra_len, cmt_len, _, _, _, lfh_off,
        ) = struct.unpack("<IHHHHHHIIIHHHHHII", cd[p : p + 46])
        name = cd[p + 46 : p + 46 + name_len].decode("utf-8", "replace")
        members.append(
            {"name": name, "method": method, "comp_size": comp_size, "lfh_off": lfh_off}
        )
        p += 46 + name_len + extra_len + cmt_len
    return members


def _fetch_member(url: str, m: dict, *, timeout: int = 60) -> bytes | None:
    """Range-fetch and inflate a single ZIP member. ``None`` on any failure."""
    method = m["method"]
    if method not in (0, 8):  # only stored / deflate
        return None
    # The local file header has its own variable name/extra lengths; read it
    # first (fixed 30 bytes), then skip name+extra to reach the data.
    try:
        lfh = _range(url, m["lfh_off"], m["lfh_off"] + 29, timeout=timeout)
    except (urllib.error.URLError, urllib.error.HTTPError, OSError, TimeoutError):
        return None
    if not lfh.startswith(_LFH_SIG):
        return None
    name_len, extra_len = struct.unpack("<HH", lfh[26:30])
    data_off = m["lfh_off"] + 30 + name_len + extra_len
    try:
        comp = _range(url, data_off, data_off + m["comp_size"] - 1, timeout=timeout)
    except (urllib.error.URLError, urllib.error.HTTPError, OSError, TimeoutError):
        return None
    if method == 0:
        return comp
    try:
        return zlib.decompress(comp, -zlib.MAX_WBITS)  # raw deflate
    except zlib.error:
        return None


def fetch_robustness(
    out_dir: Path,
    n: int = 250,
    source: str = "govdocs1",
    *,
    max_member_bytes: int = 8 << 20,
) -> list[dict]:
    """Extract up to ``n`` valid PDFs from ``source`` thread zips via HTTP Range.

    Only the central directory and the chosen ``*.pdf`` members are downloaded,
    not the whole archive. Each PDF is validated to start with ``b"%PDF"`` and
    be non-empty; corrupt/encrypted/non-PDF members are skipped and the run
    continues.

    Parameters
    ----------
    out_dir:
        Destination for ``<id>.pdf`` files and ``manifest.json``.
    n:
        Maximum number of PDFs to keep.
    source:
        ``"govdocs1"`` (default, public-domain) or ``"safedocs"`` (fetch-only).
    max_member_bytes:
        Skip members whose compressed size exceeds this (default 8 MiB). Keeps
        per-member fetches bounded on slow links; the corpus stays diverse
        without a handful of giant outliers dominating the download time.

    Returns
    -------
    The list of manifest entries (also written to ``out_dir/manifest.json``).
    """
    if source not in _SOURCES:
        raise ValueError(f"unknown source {source!r}; choose from {sorted(_SOURCES)}")
    cfg = _SOURCES[source]
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    entries: list[dict] = []
    idx = 0
    for thread in cfg["threads"]:
        if len(entries) >= n:
            break
        candidates = cfg["urls"](thread)
        print(f"\n== {source} thread {thread} ==")
        picked = _pick_url(candidates)
        if picked is None:
            print(f"  no working host for thread {thread} (tried {len(candidates)})")
            continue
        url, total = picked
        print(f"  using {url}  ({total:,} B)")

        # Cache the parsed central directory so re-runs skip the tail round-trip.
        cd_cache = CACHE_DIR / f"{source}-{thread:04d}-cd.json"
        if cd_cache.exists():
            members = json.loads(cd_cache.read_text(encoding="utf-8"))
        else:
            try:
                members = _read_central_dir(url, total)
            except (urllib.error.URLError, urllib.error.HTTPError, OSError, TimeoutError, struct.error) as exc:
                print(f"  FAIL central dir: {type(exc).__name__}: {exc}")
                continue
            if members:
                cd_cache.write_text(json.dumps(members), encoding="utf-8")
        pdfs = [
            m
            for m in members
            if m["name"].lower().endswith(".pdf")
            and 0 < m["comp_size"] <= max_member_bytes
        ]
        print(f"  {len(members)} members, {len(pdfs)} *.pdf <= {max_member_bytes:,} B")

        got = 0
        for m in pdfs:
            if len(entries) >= n:
                break
            data = _fetch_member(url, m)
            if not data or not data.startswith(b"%PDF"):
                continue
            name = f"{source}-{idx:05d}"
            out_path = out_dir / f"{name}.pdf"
            try:
                out_path.write_bytes(data)
            except OSError as exc:
                print(f"    skip {m['name']}: write {type(exc).__name__}")
                continue
            entries.append(
                {
                    "name": name,
                    "pdf": str(out_path.resolve()),
                    "source": source,
                    "license": cfg["license"],
                    "size": len(data),
                    "orig": m["name"],
                }
            )
            idx += 1
            got += 1
            if got % 10 == 0:
                print(f"\r  +{got} PDFs (total {len(entries)}/{n})", end="", flush=True)
        print(f"\r  +{got} PDFs (total {len(entries)}/{n})")

    manifest = out_dir / "manifest.json"
    manifest.write_text(json.dumps(entries, indent=2), encoding="utf-8")
    print(f"\nWrote {len(entries)} PDFs + manifest -> {manifest}")
    if entries:
        sizes = sorted(e["size"] for e in entries)
        print(
            f"Size range: {sizes[0]:,} B .. {sizes[-1]:,} B "
            f"(median {sizes[len(sizes) // 2]:,} B)"
        )
    return entries


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT, help="output directory")
    ap.add_argument("--n", type=int, default=250, help="max PDFs to keep (default 250)")
    ap.add_argument(
        "--source",
        choices=sorted(_SOURCES),
        default="govdocs1",
        help="corpus source (default govdocs1)",
    )
    ap.add_argument(
        "--max-member-mb",
        type=float,
        default=8.0,
        help="skip PDF members larger (compressed) than this many MiB (default 8)",
    )
    args = ap.parse_args(argv)
    fetch_robustness(
        args.out,
        n=args.n,
        source=args.source,
        max_member_bytes=int(args.max_member_mb * (1 << 20)),
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
