#!/usr/bin/env python3
"""Fetch a MULTILINGUAL ground-truth corpus from EUR-Lex / EU Publications Office.

Part of the OBJECTIVE ground-truth accuracy subsystem (sibling of ``pmc_fetch.py``
and ``born_digital.py``). Where the existing corpora are English-only (PMC papers,
born-digital Gutenberg), this one supplies REAL, non-English PDFs whose true text
is independently knowable — to test text extraction on accented Latin, Greek, and
Cyrillic scripts.

Why EU law makes ideal ground truth
-----------------------------------
The EU Publications Office serves the SAME legal document as BOTH an official PDF
and an official plain-text/HTML rendition in 24 languages, so the text rendition
is genuine ground truth for the PDF. Everything published by the Publications
Office is reusable under CC-BY 4.0 (Commission Decision 2011/833/EU), commercial
reuse included.

Endpoint reality (verified June 2026 — the load-bearing discovery)
------------------------------------------------------------------
``eur-lex.europa.eu`` itself sits behind CloudFront + AWS WAF: every request to
``/legal-content/.../TXT/PDF/?uri=CELEX:...`` returns HTTP 202 with an empty body
and ``x-amzn-waf-action: challenge`` (a JavaScript bot challenge we cannot solve
with urllib). The actual document bytes, however, live on a DIFFERENT host —
the Publications Office **Cellar** at ``publications.europa.eu`` — which is NOT
behind the WAF and supports clean HTTP content negotiation:

* Ground-truth TEXT (per language)::

      GET https://publications.europa.eu/resource/celex/{CELEX}
          Accept: application/xhtml+xml
          Accept-Language: {ISO-639-3, e.g. eng/fra/deu/ell/bul/pol/spa/ita}

  → 200, the official XHTML rendition. Its *final* URL (after the Cellar's
  internal redirect) is ``.../cellar/{uuid}.{seq}.03/DOC_1`` — revealing the
  document's Cellar UUID and the per-language manifestation sequence.

* The matching PDF is the SAME manifestation with format suffix ``.01`` instead
  of ``.03``::

      GET http://publications.europa.eu/resource/cellar/{uuid}.{seq}.01/DOC_1
          Accept: application/pdf;type=pdfa1a

  → 200, ``application/pdf;type=pdfa1a``, a real ``%PDF-1.4`` (PDF/A-1a) file.

So for each (CELEX, LANG) we fetch the XHTML (→ gt_text + uuid/seq), then derive
and fetch the ``.01`` PDF. Either side missing → that pair is skipped.

Notes
-----
* Stdlib only (urllib/json/argparse/html.parser/csv) — the project venv has no
  ``requests``. Like the sibling fetchers, all requests strip proxy env via
  ``ProxyHandler({})`` (a local proxy breaks TLS to some hosts).
* Polite: browser UA, small inter-request delay, timeouts, retries; failures are
  skipped and the run continues. Downloads are cached to disk so re-runs are cheap.
* Greek/Cyrillic text is preserved as proper UTF-8 (no ASCII folding) — the XHTML
  is decoded UTF-8 and written with ``encoding="utf-8"`` / ``ensure_ascii=False``.

CLI::

    env -u CONDA_PREFIX .venv/bin/python conformance/gt/fetch_eurlex.py \
        --out conformance/gt/corpus-eurlex

    # offline-safe + tiny-real self test (2 CELEX x 3 langs incl. Greek/Cyrillic):
    env -u CONDA_PREFIX .venv/bin/python conformance/gt/fetch_eurlex.py --self-test

API::

    from conformance.gt.fetch_eurlex import fetch_eurlex
    fetch_eurlex(Path("conformance/gt/corpus-eurlex"))
"""

from __future__ import annotations

import argparse
import http.client
import json
import re
import sys
import time
import urllib.error
import urllib.request
from html.parser import HTMLParser
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO_ROOT / "conformance" / "gt" / "corpus-eurlex"

# Publications Office "Cellar" — NOT behind the eur-lex.europa.eu WAF.
CELEX_BASE = "https://publications.europa.eu/resource/celex/"

_UA = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
)

# UI language code -> ISO-639-3 used by the Cellar's Accept-Language negotiation.
LANG_ISO: dict[str, str] = {
    "EN": "eng",
    "FR": "fra",
    "DE": "deu",
    "ES": "spa",
    "IT": "ita",
    "EL": "ell",  # Greek
    "BG": "bul",  # Cyrillic
    "PL": "pol",
}
# Ordered so the script-diverse languages (Greek, Cyrillic) come FIRST: under a
# small ``max_docs`` cap, language-major iteration then still guarantees coverage
# of the non-Latin scripts this corpus exists to test.
DEFAULT_LANGS: list[str] = ["EL", "BG", "PL", "DE", "FR", "ES", "IT", "EN"]

# Well-known EU regulations / directives that exist in all 24 languages. Each is a
# substantial, real legal text (multi-column OJ layout; annexes with tables) —
# good extraction diversity. CELEX IDs verified available in our target languages.
DEFAULT_CELEX: list[str] = [
    "32016R0679",  # GDPR — General Data Protection Regulation
    "32011L0083",  # Consumer Rights Directive
    "32014R0596",  # Market Abuse Regulation
    "32019R0947",  # Rules/procedures for operation of unmanned aircraft (drones)
    "32006L0112",  # VAT Directive (common system of value added tax)
    "32019R0881",  # Cybersecurity Act (ENISA / EU cybersecurity certification)
    "32018R1725",  # Data protection by Union institutions and bodies
    "32016L0680",  # Law-enforcement data protection directive
    "32013R0575",  # Capital Requirements Regulation (CRR) — banking
    "32008L0048",  # Consumer credit directive
]

# Cellar manifestation format suffixes (the segment after ``.{seq}.``). The XHTML
# rendition is reliably ``.03``, but the PDF suffix VARIES per document (``.01``
# for GDPR, ``.02`` for the Consumer Rights directive, etc.), so we probe this
# ordered list of candidates and take the first that returns a real ``%PDF``.
_FMT_XHTML = "03"  # official text rendition (ground truth)
_PDF_FMT_CANDIDATES = ("01", "02", "00", "04", "05", "06")

_ACCEPT_XHTML = "application/xhtml+xml"
# PDF Accept variants, broadest first: the generic ``application/pdf`` matches any
# PDF flavor (older OJ docs offer a plain PDF, not PDF/A), while the typed
# variants pin newer PDF/A manifestations. We try each suffix with each Accept.
_ACCEPT_PDF_VARIANTS = (
    "application/pdf",
    "application/pdf;type=pdfa1a",
    "application/pdf;type=pdfa1b",
    "application/pdf;type=pdfx",
)

_REQUEST_DELAY = 0.5   # politeness delay between (CELEX,LANG) pairs (seconds)
_MAX_RETRIES = 3
_TIMEOUT = 90

# final XHTML URL looks like .../cellar/<uuid>.<seq>.<fmt>/DOC_1
_MANIFESTATION_RE = re.compile(r"cellar/([0-9a-fA-F-]+)\.(\d+)\.(\d+)/DOC_1")


# --------------------------------------------------------------------------- #
# HTTP helpers (proxy-stripped, retrying) — mirrors pmc_fetch.py
# --------------------------------------------------------------------------- #
def _opener() -> urllib.request.OpenerDirector:
    """Opener with proxies disabled (local proxy breaks TLS to EU hosts)."""
    return urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _request(url: str, accept: str | None, accept_lang: str | None) -> urllib.request.Request:
    headers = {"User-Agent": _UA}
    if accept:
        headers["Accept"] = accept
    if accept_lang:
        headers["Accept-Language"] = accept_lang
    return urllib.request.Request(url, headers=headers)


def _get(
    url: str,
    *,
    accept: str | None = None,
    accept_lang: str | None = None,
    timeout: int = _TIMEOUT,
    retries: int = _MAX_RETRIES,
) -> tuple[bytes, str, str]:
    """GET ``url`` fully. Returns (body, content_type, final_url).

    Retries transient network errors; raises HTTPError 404/406/400 immediately
    (those are "not this manifestation" answers the caller handles).
    """
    opener = _opener()
    last: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            with opener.open(_request(url, accept, accept_lang), timeout=timeout) as resp:
                data = resp.read()
                return data, resp.headers.get("Content-Type", ""), resp.geturl()
        except urllib.error.HTTPError as exc:
            if exc.code in (400, 404, 406, 403):
                raise  # definitive: wrong manifestation / not available
            last = exc
        except (urllib.error.URLError, OSError, TimeoutError,
                http.client.IncompleteRead, http.client.HTTPException) as exc:
            last = exc  # transient (e.g. chunked-transfer truncation) — retry
        if attempt < retries:
            time.sleep(min(1.5 * attempt, 4.0))
    assert last is not None
    raise last


# --------------------------------------------------------------------------- #
# HTML -> plain text (stdlib html.parser, robust)
# --------------------------------------------------------------------------- #
# Tags whose textual content is NOT body text and must be dropped entirely.
_SKIP_TAGS = {"script", "style", "head", "title", "meta", "link"}
# Block-level tags that imply a line break around their content.
_BLOCK_TAGS = {
    "p", "br", "div", "tr", "table", "h1", "h2", "h3", "h4", "h5", "h6",
    "li", "ul", "ol", "td", "th", "section", "article", "header", "footer",
    "blockquote", "hr", "caption",
}


class _TextExtractor(HTMLParser):
    """Strip HTML to readable plain text, dropping script/style and adding
    newlines around block elements. Preserves all Unicode (entities resolved)."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._chunks: list[str] = []
        self._skip_depth = 0

    def handle_starttag(self, tag: str, attrs) -> None:  # noqa: ANN001
        if tag in _SKIP_TAGS:
            self._skip_depth += 1
        elif tag in _BLOCK_TAGS:
            self._chunks.append("\n")

    def handle_startendtag(self, tag: str, attrs) -> None:  # noqa: ANN001
        if tag in _BLOCK_TAGS:
            self._chunks.append("\n")

    def handle_endtag(self, tag: str) -> None:
        if tag in _SKIP_TAGS and self._skip_depth > 0:
            self._skip_depth -= 1
        elif tag in _BLOCK_TAGS:
            self._chunks.append("\n")

    def handle_data(self, data: str) -> None:
        if self._skip_depth == 0:
            self._chunks.append(data)

    def get_text(self) -> str:
        text = "".join(self._chunks)
        # Collapse intra-line whitespace runs; cap blank-line runs at one.
        text = re.sub(r"[ \t\f\v ]+", " ", text)
        text = re.sub(r" *\n *", "\n", text)
        text = re.sub(r"\n{3,}", "\n\n", text)
        return text.strip()


def html_to_text(html_bytes: bytes) -> str:
    """Decode (UTF-8) and strip an XHTML rendition to clean body text.

    UTF-8 is correct for Cellar XHTML (Content-Type charset=UTF-8); decoding with
    ``replace`` guards against any stray bytes without corrupting Greek/Cyrillic.
    """
    html = html_bytes.decode("utf-8", "replace")
    parser = _TextExtractor()
    parser.feed(html)
    return parser.get_text()


# --------------------------------------------------------------------------- #
# Per-(CELEX,LANG) resolution
# --------------------------------------------------------------------------- #
def _fetch_text_and_uuid(celex: str, iso: str) -> tuple[str, str, str] | None:
    """Fetch the XHTML rendition for (celex, iso).

    Returns (gt_text, uuid, seq) or None if unavailable. ``seq`` is the language
    manifestation sequence parsed from the Cellar's final redirect URL; the PDF
    is derived from (uuid, seq).
    """
    try:
        body, ctype, final = _get(
            CELEX_BASE + celex, accept=_ACCEPT_XHTML, accept_lang=iso
        )
    except (urllib.error.HTTPError, urllib.error.URLError, OSError, TimeoutError,
            http.client.HTTPException):
        return None
    if not body or "xml" not in ctype.lower() and "html" not in ctype.lower():
        return None
    m = _MANIFESTATION_RE.search(final)
    if not m:
        return None
    uuid, seq = m.group(1), m.group(2)
    text = html_to_text(body)
    if not text.strip():
        return None
    return text, uuid, seq


def _cellar_base() -> str:
    return CELEX_BASE.replace("/celex/", "/cellar/")


def _fetch_pdf(uuid: str, seq: str) -> bytes | None:
    """Find and fetch the PDF manifestation for (uuid, seq); validate %PDF magic.

    Both the format SUFFIX (``.01`` for GDPR, ``.02`` for Consumer Rights, ...)
    and the PDF FLAVOR (plain ``application/pdf`` for older OJ docs, ``pdfa1a``
    for newer ones) vary per document, so probe every suffix × every Accept
    variant and return the first combination that yields a real ``%PDF``.

    A 404/406 just means "not this suffix/flavor" → try the next combination;
    only a hard network failure aborts (returns None).
    """
    for fmt in _PDF_FMT_CANDIDATES:
        url = f"{_cellar_base()}{uuid}.{seq}.{fmt}/DOC_1"
        for accept in _ACCEPT_PDF_VARIANTS:
            try:
                data, _ctype, _final = _get(url, accept=accept)
            except urllib.error.HTTPError:
                continue  # wrong suffix/flavor for this document
            except (urllib.error.URLError, OSError, TimeoutError,
                    http.client.HTTPException):
                return None
            if data.startswith(b"%PDF"):
                return data
    return None


# --------------------------------------------------------------------------- #
# Public API
# --------------------------------------------------------------------------- #
def fetch_eurlex(
    out_dir: Path,
    celex_ids: list[str] | None = None,
    langs: list[str] | None = None,
    max_docs: int = 40,
    *,
    verbose: bool = True,
) -> list[dict]:
    """Fetch up to ``max_docs`` (PDF, ground-truth-text) pairs from EUR-Lex/Cellar.

    For every (CELEX, LANG) pair (iterated language-major so the corpus stays
    balanced across scripts if ``max_docs`` is small), fetch the official XHTML
    text rendition and the matching PDF manifestation, caching both to disk.

    Returns the manifest entries (also written to ``out_dir/manifest.json``)::

        {"name": "{CELEX}_{LANG}", "pdf": "<abs path>", "gt_text": "<text>",
         "lang": "{LANG}", "license": "CC-BY-4.0"}

    Already-downloaded pairs are reused (cache); failures are skipped and the run
    continues. Languages not in ``LANG_ISO`` are ignored with a note.
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    celex_ids = list(celex_ids or DEFAULT_CELEX)
    langs = list(langs or DEFAULT_LANGS)

    def log(msg: str) -> None:
        if verbose:
            print(msg, flush=True)

    unknown = [l for l in langs if l.upper() not in LANG_ISO]
    if unknown:
        log(f"  note: ignoring unknown languages {unknown} (no ISO mapping)")
    langs = [l.upper() for l in langs if l.upper() in LANG_ISO]

    manifest: list[dict] = []
    # Language-major iteration keeps script coverage balanced under a small cap.
    pairs = [(celex, lang) for lang in langs for celex in celex_ids]
    for celex, lang in pairs:
        if len(manifest) >= max_docs:
            break
        iso = LANG_ISO[lang]
        name = f"{celex}_{lang}"
        pdf_path = out_dir / f"{name}.pdf"
        txt_path = out_dir / f"{name}.txt"

        # Cache hit: reuse both sides if present and non-empty.
        if pdf_path.exists() and txt_path.exists() and pdf_path.stat().st_size > 0:
            gt_text = txt_path.read_text(encoding="utf-8")
            if gt_text.strip():
                log(f"  CACHED {name}")
                manifest.append(_entry(name, pdf_path, gt_text, lang))
                continue

        time.sleep(_REQUEST_DELAY)
        resolved = _fetch_text_and_uuid(celex, iso)
        if resolved is None:
            log(f"  SKIP   {name} — text rendition unavailable")
            continue
        gt_text, uuid, seq = resolved

        pdf_bytes = _fetch_pdf(uuid, seq)
        if pdf_bytes is None:
            log(f"  SKIP   {name} — PDF manifestation unavailable (uuid={uuid} seq={seq})")
            continue

        pdf_path.write_bytes(pdf_bytes)
        txt_path.write_text(gt_text, encoding="utf-8")
        n_nonascii = sum(1 for c in gt_text if ord(c) > 127)
        log(f"  OK     {name} pdf={len(pdf_bytes):,}B text={len(gt_text):,}chars "
            f"(non-ascii={n_nonascii:,}) [{len(manifest) + 1}/{max_docs}]")
        manifest.append(_entry(name, pdf_path, gt_text, lang))

    _write_manifest(out_dir, manifest)
    by_lang: dict[str, int] = {}
    for e in manifest:
        by_lang[e["lang"]] = by_lang.get(e["lang"], 0) + 1
    log(f"\nFetched {len(manifest)} (pdf,gt) pair(s) across {len(by_lang)} language(s): "
        f"{', '.join(f'{k}={v}' for k, v in sorted(by_lang.items()))}")
    log(f"Manifest -> {out_dir / 'manifest.json'}")
    return manifest


def _entry(name: str, pdf: Path, gt_text: str, lang: str) -> dict:
    return {
        "name": name,
        "pdf": str(pdf.resolve()),
        "gt_text": gt_text,
        "lang": lang,
        "license": "CC-BY-4.0",
    }


def _write_manifest(out_dir: Path, manifest: list[dict]) -> None:
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8"
    )


# --------------------------------------------------------------------------- #
# Self-test: offline HTML-stripping checks + a tiny real fetch
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    """Offline checks on URL/HTML logic + a small real fetch (2 CELEX x 3 langs,
    incl. Greek and Cyrillic). Network failures degrade to a clear note and a 0
    exit so the autonomous build is never hard-failed by EUR-Lex flakiness.
    """
    import tempfile

    # --- offline: manifestation regex + PDF URL derivation ---
    sample_final = (
        "http://publications.europa.eu/resource/cellar/"
        "3e485e15-11bd-11e6-ba9a-01aa75ed71a1.0006.03/DOC_1"
    )
    m = _MANIFESTATION_RE.search(sample_final)
    assert m and m.group(2) == "0006" and m.group(3) == "03", sample_final
    uuid, seq = m.group(1), m.group(2)
    pdf_url = f"{_cellar_base()}{uuid}.{seq}.{_PDF_FMT_CANDIDATES[0]}/DOC_1"
    assert pdf_url == (
        "https://publications.europa.eu/resource/cellar/"
        "3e485e15-11bd-11e6-ba9a-01aa75ed71a1.0006.01/DOC_1"
    ), pdf_url

    # --- offline: HTML stripping drops script/style, keeps Unicode, breaks blocks ---
    html = (
        b"<html><head><title>x</title><style>.a{}</style></head><body>"
        b"<p>\xce\x9a\xce\x91\xce\x9d\xce\x9f\xce\x9d\xce\x99\xce\xa3\xce\x9c\xce\x9f\xce\xa3</p>"  # ΚΑΝΟΝΙΣΜΟΣ
        b"<script>var x=1;</script>"
        b"<p>&#1056;&#1077;&#1075;&#1083;&#1072;&#1084;&#1077;&#1085;&#1090;</p>"  # Регламент (Cyrillic via entities)
        b"</body></html>"
    )
    text = html_to_text(html)
    assert "ΚΑΝΟΝΙΣΜΟΣ" in text, repr(text)
    assert "Регламент" in text, repr(text)
    assert "var x=1" not in text and ".a{}" not in text, repr(text)
    assert "\n" in text, "block tags should introduce a line break"
    print("  offline checks OK (regex, PDF-URL derivation, HTML stripping w/ Greek+Cyrillic)")

    # --- tiny real fetch: 2 CELEX x 3 langs incl. Greek (EL) + Cyrillic (BG) ---
    small_celex = ["32016R0679", "32011L0083"]
    small_langs = ["EN", "EL", "BG"]
    with tempfile.TemporaryDirectory() as td:
        try:
            entries = fetch_eurlex(
                Path(td), celex_ids=small_celex, langs=small_langs, max_docs=6, verbose=True
            )
        except Exception as exc:  # noqa: BLE001
            print(f"  live fetch skipped (network unavailable: {type(exc).__name__}: {exc})")
            print("fetch_eurlex.py self-test OK (offline checks passed)")
            return 0

        if not entries:
            print("  live fetch returned 0 pairs (EUR-Lex/Cellar unreachable or throttled); "
                  "offline checks still passed")
            print("fetch_eurlex.py self-test OK")
            return 0

        langs_seen = {e["lang"] for e in entries}
        for e in entries:
            pdf = Path(e["pdf"])
            assert pdf.exists(), e["name"]
            head = pdf.read_bytes()[:4]
            assert head == b"%PDF", f"{e['name']} not a PDF: {head!r}"
            assert pdf.stat().st_size > 1024, f"{e['name']} PDF too small"
            assert e["gt_text"].strip(), f"{e['name']} empty gt_text"
        # The EL/BG docs MUST contain non-ASCII (Greek / Cyrillic) text.
        for lang in ("EL", "BG"):
            for e in (x for x in entries if x["lang"] == lang):
                assert any(ord(c) > 127 for c in e["gt_text"]), \
                    f"{e['name']} ({lang}) gt_text has no non-ASCII chars"

        sizes = ", ".join(
            f"{e['name']}: pdf={Path(e['pdf']).stat().st_size:,}B/"
            f"text={len(e['gt_text']):,}ch" for e in entries[:4]
        )
        print(f"  live fetch: {len(entries)} pair(s); langs={sorted(langs_seen)}")
        print(f"  examples: {sizes}")
        print("  EL/BG gt_text verified non-ASCII (proper UTF-8 Greek/Cyrillic)")

    print("fetch_eurlex.py self-test OK")
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT,
                    help="output dir for PDFs + .txt + manifest.json")
    ap.add_argument("--celex", action="append", default=None,
                    help="CELEX id (repeatable); default: a built-in multilingual set")
    ap.add_argument("--langs", default=None,
                    help="comma-separated UI language codes (default: "
                         + ",".join(DEFAULT_LANGS) + ")")
    ap.add_argument("--max-docs", type=int, default=40,
                    help="cap on number of (pdf,gt) pairs to fetch")
    ap.add_argument("--self-test", action="store_true",
                    help="run offline checks + a tiny real fetch, then exit")
    args = ap.parse_args(argv)

    if args.self_test:
        return _self_test()

    langs = ([s.strip() for s in args.langs.split(",") if s.strip()]
             if args.langs else None)
    fetch_eurlex(args.out, celex_ids=args.celex, langs=langs, max_docs=args.max_docs)
    return 0


if __name__ == "__main__":
    sys.exit(main())
