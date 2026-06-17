#!/usr/bin/env python3
"""Fetch domain-diverse, public-domain US government PDFs from GovInfo (US GPO).

This is a FETCH-ONLY tool. It downloads real public-domain PDFs published by the
US Government Publishing Office so the oxide-pdf harness can run an
oxide-vs-fitz *differential* extraction across document DOMAINS that our other
corpora (forms / scientific / legal-EU / multilingual) under-represent:

  * ``USCOURTS``   -- US federal court opinions (legal prose, varied typesetting)
  * ``GAOREPORTS`` -- Government Accountability Office audit reports (headings,
                      tables, footnotes, two-column summaries)
  * ``FR``         -- the Federal Register (dense multi-column regulatory text;
                      the hardest layout for reading-order / column detection)

All GovInfo content is a US-federal-government work and therefore public domain
in the US (17 U.S.C. 105). NO ground truth is produced here -- the manifest is
``gt_text``-free; scoring is the oxide-vs-fitz/pdfminer differential run by the
existing ``conformance/run_validation.py`` harness (which globs ``*.pdf`` from
the corpus dir; the manifest is metadata only).

Two discovery back-ends
-----------------------
1. GovInfo JSON API (``https://api.govinfo.gov``, free via api.data.gov, key
   ``DEMO_KEY``). Collection listings paginate by an *opaque mark*
   (``offsetMark=*``, NOT a numeric ``offset``); each package's PDF link is
   resolved via ``/packages/{id}/summary``. ``DEMO_KEY`` is shared and harshly
   rate-limited (HTTP 429 with multi-hour ``Retry-After``), so this is only the
   *preferred* back-end when ``--api-key`` is a real registered key.

2. Keyless sitemaps + bulk content URLs (the DEFAULT, used whenever the API is
   rate-limited or no real key is given). GovInfo publishes per-collection XML
   sitemaps that need no key and are not rate-limited::

       /sitemap/{CODE}_sitemap_index.xml      -> year/court leaf sitemaps
       <leaf sitemap>                         -> /app/details/{packageId} URLs

   and serves the PDF bytes keyless from the bulk content tree::

       FR, GAOREPORTS:  /content/pkg/{packageId}/pdf/{packageId}.pdf
       USCOURTS:        /content/pkg/{packageId}/pdf/{packageId}-0.pdf  (granule 0)

   This path uses no quota at all, so it is the robust default.

Politeness
----------
Both back-ends use a short delay between requests, retry 429/5xx with
exponential backoff (honouring a *bounded* ``Retry-After`` so we never sleep
for hours), cache discovered package lists per collection, cache downloaded
PDFs on disk, and skip any failing package without aborting the run.

Network notes
-------------
A local HTTP(S) proxy on this machine breaks TLS to several hosts, so every
request strips proxy settings (``ProxyHandler({})``), mirroring
``conformance/fetch_corpus.py`` and ``fetch_robustness.py``.

Usage (from repo root)::

    env -u CONDA_PREFIX .venv/bin/python conformance/gt/fetch_govinfo.py \
        --out conformance/gt/corpus-govinfo
    # force the rate-limited JSON API with a real key:
    env -u CONDA_PREFIX .venv/bin/python conformance/gt/fetch_govinfo.py \
        --out conformance/gt/corpus-govinfo --backend api --api-key YOURKEY
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO_ROOT / "conformance" / "gt" / "corpus-govinfo"
# Cache discovered package lists so re-runs skip discovery network cost.
CACHE_DIR = REPO_ROOT / "conformance" / "gt" / "cache" / "govinfo"

API_BASE = "https://api.govinfo.gov"
WWW_BASE = "https://www.govinfo.gov"
DEFAULT_COLLECTIONS = ("USCOURTS", "GAOREPORTS", "FR")

# lastModified window for the JSON-API listing back-end (wide -> more packages).
WINDOW_START = "2020-01-01T00:00:00Z"
WINDOW_END = "2026-06-01T00:00:00Z"

# Cap per-PDF size. FR daily issues can be ~20 MB which both bloats the corpus
# and dominates the differential run time; prefer many small/medium documents
# over a few giants. Enforced while streaming so we never buffer a huge body.
DEFAULT_MAX_BYTES = 12 << 20  # 12 MiB

# A bounded ceiling on any Retry-After backoff: a real rate-limit reset can be
# hours away, but this tool must finish, so cap the wait and skip instead.
MAX_BACKOFF_WAIT = 30.0

_UA = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
)


def _opener() -> urllib.request.OpenerDirector:
    """An opener with proxy env stripped (the local proxy breaks TLS to .gov)."""
    return urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _with_key(url: str, api_key: str) -> str:
    """Append ``api_key`` to ``url`` (handles both ``?`` and ``&`` cases)."""
    sep = "&" if "?" in url else "?"
    return f"{url}{sep}api_key={urllib.parse.quote(api_key)}"


class _RateLimited(Exception):
    """Raised when the API is still rate-limited after bounded retries."""


def _request(
    url: str,
    *,
    timeout: int = 60,
    max_bytes: int | None = None,
    retries: int = 3,
) -> tuple[str, bytes]:
    """GET ``url`` with proxy stripped, retrying 429/5xx with bounded backoff.

    Returns ``(content_type, body)``. If ``max_bytes`` is set, the body is read
    incrementally and a ``ValueError`` is raised once the cap is exceeded (so we
    never buffer an oversized PDF). Raises ``_RateLimited`` if still throttled
    after ``retries`` attempts; other HTTP/URL errors propagate.
    """
    backoff = 2.0
    last_exc: Exception | None = None
    for attempt in range(retries + 1):
        req = urllib.request.Request(url, headers={"User-Agent": _UA})
        try:
            with _opener().open(req, timeout=timeout) as resp:
                ctype = resp.headers.get("Content-Type", "")
                if max_bytes is None:
                    return ctype, resp.read()
                chunks: list[bytes] = []
                got = 0
                while True:
                    chunk = resp.read(1 << 16)
                    if not chunk:
                        break
                    got += len(chunk)
                    if got > max_bytes:
                        raise ValueError(f"exceeds {max_bytes} byte cap")
                    chunks.append(chunk)
                return ctype, b"".join(chunks)
        except urllib.error.HTTPError as exc:
            last_exc = exc
            if exc.code == 429 or 500 <= exc.code < 600:
                if attempt >= retries:
                    if exc.code == 429:
                        raise _RateLimited(str(exc)) from exc
                    raise
                wait = backoff
                ra = exc.headers.get("Retry-After") if exc.headers else None
                if ra and ra.isdigit():
                    wait = min(max(wait, float(ra)), MAX_BACKOFF_WAIT)
                if wait >= MAX_BACKOFF_WAIT and exc.code == 429:
                    # A multi-hour reset: don't sit here, surface as rate-limited.
                    raise _RateLimited(f"{exc} (Retry-After={ra})") from exc
                print(f"    HTTP {exc.code}; backing off {wait:.1f}s "
                      f"(attempt {attempt + 1}/{retries})")
                time.sleep(wait)
                backoff *= 2
                continue
            raise
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            last_exc = exc
            if attempt >= retries:
                raise
            time.sleep(backoff)
            backoff *= 2
    if last_exc:
        raise last_exc
    raise RuntimeError("unreachable")


def _get_bytes(url: str, *, delay: float, timeout: int = 60) -> bytes:
    """GET raw bytes with a politeness delay."""
    time.sleep(delay)
    _ctype, body = _request(url, timeout=timeout)
    return body


def _get_json(url: str, *, delay: float, timeout: int = 60) -> dict:
    """GET a JSON endpoint with a politeness delay; returns the decoded dict."""
    return json.loads(_get_bytes(url, delay=delay, timeout=timeout))


# --------------------------------------------------------------------------- #
# Discovery: package-id lists per collection.
# --------------------------------------------------------------------------- #

_LOC_RE = re.compile(rb"<loc>\s*(.*?)\s*</loc>", re.DOTALL)
_DETAILS_RE = re.compile(r"/app/details/([A-Za-z0-9._-]+)")


def _discover_sitemap(
    collection: str, *, want: int, delay: float
) -> list[str]:
    """Discover up to ``want`` package IDs for ``collection`` via keyless sitemaps.

    Walks the per-collection sitemap index, then leaf sitemaps, collecting the
    ``/app/details/{packageId}`` package IDs. Newer leaf sitemaps (by the year in
    their filename) are visited first so we sample current documents. Returns a
    de-duplicated list of package IDs (best-effort; partial on any failure).
    """
    index_url = f"{WWW_BASE}/sitemap/{collection}_sitemap_index.xml"
    try:
        body = _get_bytes(index_url, delay=delay)
    except (urllib.error.HTTPError, urllib.error.URLError, OSError,
            TimeoutError, _RateLimited) as exc:
        print(f"  [{collection}] sitemap index failed: "
              f"{type(exc).__name__}: {exc}")
        return []
    leaves = [m.decode("utf-8", "replace") for m in _LOC_RE.findall(body)]

    # Prefer the most recent years first (filenames embed a 4-digit year).
    def _year(u: str) -> int:
        m = re.search(r"_(\d{4})_sitemap", u)
        return int(m.group(1)) if m else 0

    leaves.sort(key=_year, reverse=True)

    ids: list[str] = []
    seen: set[str] = set()
    for leaf in leaves:
        if len(ids) >= want:
            break
        try:
            lbody = _get_bytes(leaf, delay=delay)
        except (urllib.error.HTTPError, urllib.error.URLError, OSError,
                TimeoutError, _RateLimited) as exc:
            print(f"    leaf {leaf.split('/')[-1]} failed: "
                  f"{type(exc).__name__}: {exc}")
            continue
        for loc in _LOC_RE.findall(lbody):
            m = _DETAILS_RE.search(loc.decode("utf-8", "replace"))
            if not m:
                continue
            pid = m.group(1)
            if pid in seen:
                continue
            seen.add(pid)
            ids.append(pid)
            if len(ids) >= want:
                break
    return ids


def _discover_api(
    collection: str, *, want: int, api_key: str, page_size: int, delay: float
) -> list[str]:
    """Discover up to ``want`` package IDs via the rate-limited JSON API.

    Returns a list of package IDs whose summary advertises a ``pdfLink``. On
    rate-limiting it returns whatever it resolved so far.
    """
    start = urllib.parse.quote(WINDOW_START, safe="")
    end = urllib.parse.quote(WINDOW_END, safe="")
    next_url: str | None = (
        f"{API_BASE}/collections/{collection}/{start}/{end}"
        f"?offsetMark=%2A&pageSize={page_size}"
    )
    ids: list[str] = []
    seen: set[str] = set()
    pages = 0
    while next_url and len(ids) < want and pages < 60:
        pages += 1
        try:
            data = _get_json(_with_key(next_url, api_key), delay=delay)
        except _RateLimited:
            print(f"  [{collection}] API rate-limited; keeping {len(ids)}")
            break
        except (urllib.error.HTTPError, urllib.error.URLError, OSError,
                json.JSONDecodeError, TimeoutError) as exc:
            print(f"  [{collection}] API listing failed: "
                  f"{type(exc).__name__}: {exc}")
            break
        for pkg in data.get("packages", []):
            if len(ids) >= want:
                break
            pid = pkg.get("packageId")
            if not pid or pid in seen:
                continue
            seen.add(pid)
            pdf_link = (pkg.get("download") or {}).get("pdfLink")
            if not pdf_link:
                try:
                    summary = _get_json(
                        _with_key(f"{API_BASE}/packages/{pid}/summary", api_key),
                        delay=delay,
                    )
                except _RateLimited:
                    print(f"  [{collection}] API rate-limited resolving summaries; "
                          f"keeping {len(ids)}")
                    next_url = None
                    break
                except (urllib.error.HTTPError, urllib.error.URLError, OSError,
                        json.JSONDecodeError, TimeoutError):
                    continue
                pdf_link = (summary.get("download") or {}).get("pdfLink")
            if pdf_link:
                ids.append(pid)
        if next_url is None:
            break
        next_url = data.get("nextPage")
    return ids


def _discover(
    collection: str, *, backend: str, want: int, api_key: str,
    page_size: int, delay: float,
) -> list[str]:
    """Discover package IDs for ``collection`` using the chosen ``backend``.

    Results are cached per (collection, backend) so re-runs skip discovery. If
    the API back-end yields nothing (e.g. rate-limited) it falls back to the
    keyless sitemap back-end automatically.
    """
    cache = CACHE_DIR / f"{collection}-{backend}-ids.json"
    if cache.exists():
        try:
            cached = json.loads(cache.read_text("utf-8"))
            if isinstance(cached, list) and len(cached) >= want:
                print(f"  [{collection}] using {want} cached package ids")
                return cached[:want]
        except (json.JSONDecodeError, OSError):
            pass

    if backend == "api":
        ids = _discover_api(
            collection, want=want, api_key=api_key,
            page_size=page_size, delay=delay,
        )
        if not ids:
            print(f"  [{collection}] API yielded 0; falling back to sitemaps")
            ids = _discover_sitemap(collection, want=want, delay=delay)
    else:
        ids = _discover_sitemap(collection, want=want, delay=delay)

    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    try:
        cache.write_text(json.dumps(ids), encoding="utf-8")
    except OSError:
        pass
    print(f"  [{collection}] discovered {len(ids)} package ids")
    return ids


# --------------------------------------------------------------------------- #
# Download: keyless bulk content URLs (per-collection PDF path templates).
# --------------------------------------------------------------------------- #


def _pdf_url_candidates(collection: str, pid: str) -> list[str]:
    """Keyless content-URL candidates for ``pid`` (collection-specific).

    GAOREPORTS / FR expose a single package-level PDF; USCOURTS exposes the
    opinion as granule ``-0`` (occasionally other granule indices), so a few
    candidates are tried in order.
    """
    base = f"{WWW_BASE}/content/pkg/{pid}/pdf"
    if collection == "USCOURTS":
        return [f"{base}/{pid}-0.pdf", f"{base}/{pid}-1.pdf", f"{base}/{pid}.pdf"]
    return [f"{base}/{pid}.pdf", f"{base}/{pid}-0.pdf"]


def fetch_govinfo(
    out_dir: Path | str,
    collections: tuple[str, ...] = DEFAULT_COLLECTIONS,
    per: int = 10,
    api_key: str = "DEMO_KEY",
    *,
    backend: str = "sitemap",
    max_bytes: int = DEFAULT_MAX_BYTES,
    page_size: int = 25,
    delay: float = 0.5,
) -> list[dict]:
    """Fetch ~``per`` public-domain PDFs from each GovInfo ``collection``.

    Package IDs are discovered (keyless sitemaps by default, or the rate-limited
    JSON API with ``backend="api"``), then each PDF is streamed keyless from the
    bulk content tree (capped at ``max_bytes``) and validated against the
    ``%PDF`` magic byte. Files land in ``out_dir/<name>.pdf`` and a
    ``gt_text``-free ``manifest.json`` of ``{name, pdf(abs), source, collection,
    license}`` is written. Downloads are cached on disk (a present, valid file
    is reused). Failures are logged and skipped; the run always finishes and
    returns whatever it collected.

    Returns the list of manifest entries (also written to ``manifest.json``).
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    entries: list[dict] = []
    per_collection: dict[str, int] = {}
    for collection in collections:
        print(f"\n== {collection} (target {per}, backend={backend}) ==")
        # Discover extra ids to absorb HTML-only / oversized / dead packages.
        ids = _discover(
            collection, backend=backend, want=per * 4,
            api_key=api_key, page_size=page_size, delay=delay,
        )
        got = 0
        for pid in ids:
            if got >= per:
                break
            safe_id = pid
            for ch in "/\\:":
                safe_id = safe_id.replace(ch, "_")
            name = safe_id if safe_id.startswith(collection) else f"{collection}-{safe_id}"
            out_path = out_dir / f"{name}.pdf"

            # Cache hit: reuse a previously downloaded valid PDF.
            if out_path.exists() and out_path.stat().st_size > 4:
                if out_path.read_bytes()[:5].startswith(b"%PDF"):
                    entries.append({
                        "name": name,
                        "pdf": str(out_path.resolve()),
                        "source": "govinfo",
                        "collection": collection,
                        "license": "public-domain",
                        "size": out_path.stat().st_size,
                        "packageId": pid,
                    })
                    got += 1
                    print(f"  cached {name} ({out_path.stat().st_size:,} B)")
                    continue

            data: bytes | None = None
            for url in _pdf_url_candidates(collection, pid):
                try:
                    time.sleep(delay)
                    ctype, body = _request(url, max_bytes=max_bytes)
                except _RateLimited:
                    print(f"  rate-limited downloading {pid}; stopping "
                          f"{collection} with {got} collected")
                    body = None
                    break
                except ValueError as exc:  # size cap
                    print(f"    skip {pid}: {exc}")
                    body = None
                    break
                except (urllib.error.HTTPError, urllib.error.URLError, OSError,
                        TimeoutError):
                    continue
                if body.startswith(b"%PDF"):
                    data = body
                    break
            if data is None:
                continue

            try:
                out_path.write_bytes(data)
            except OSError as exc:
                print(f"    skip {pid}: write {type(exc).__name__}: {exc}")
                continue

            entries.append({
                "name": name,
                "pdf": str(out_path.resolve()),
                "source": "govinfo",
                "collection": collection,
                "license": "public-domain",
                "size": len(data),
                "packageId": pid,
            })
            got += 1
            print(f"  OK {name} ({len(data):,} B)")

        per_collection[collection] = got
        print(f"  [{collection}] kept {got}/{per}")

    manifest = out_dir / "manifest.json"
    manifest.write_text(json.dumps(entries, indent=2), encoding="utf-8")

    print(f"\nWrote {len(entries)} PDFs + manifest -> {manifest}")
    for c in collections:
        print(f"  {c:12} {per_collection.get(c, 0)}")
    if entries:
        sizes = sorted(e["size"] for e in entries)
        print(f"Size range: {sizes[0]:,} B .. {sizes[-1]:,} B "
              f"(median {sizes[len(sizes) // 2]:,} B)")
    return entries


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT,
                    help="output directory (default conformance/gt/corpus-govinfo)")
    ap.add_argument("--collections", nargs="+", default=list(DEFAULT_COLLECTIONS),
                    help="GovInfo collection codes (default USCOURTS GAOREPORTS FR)")
    ap.add_argument("--per", type=int, default=10,
                    help="target PDFs per collection (default 10)")
    ap.add_argument("--api-key", default="DEMO_KEY",
                    help="api.data.gov key for --backend api (default DEMO_KEY)")
    ap.add_argument("--backend", choices=("sitemap", "api"), default="sitemap",
                    help="discovery back-end (default sitemap: keyless, no quota)")
    ap.add_argument("--max-mb", type=float, default=DEFAULT_MAX_BYTES / (1 << 20),
                    help="skip PDFs larger than this many MiB (default 12)")
    ap.add_argument("--page-size", type=int, default=25,
                    help="API listing page size for --backend api (default 25)")
    ap.add_argument("--delay", type=float, default=0.5,
                    help="seconds between requests (politeness; default 0.5)")
    args = ap.parse_args(argv)

    fetch_govinfo(
        args.out,
        collections=tuple(args.collections),
        per=args.per,
        api_key=args.api_key,
        backend=args.backend,
        max_bytes=int(args.max_mb * (1 << 20)),
        page_size=args.page_size,
        delay=args.delay,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
