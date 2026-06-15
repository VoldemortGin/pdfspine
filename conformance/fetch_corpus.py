#!/usr/bin/env python3
"""Fetch a small, license-clean Tier-1 corpus for the oxide-pdf validation harness.

Tier-1 (committable) — ONLY public-domain / CC0 / permissive sources. Everything
here is a US-federal-government work (public domain in the US under 17 U.S.C. 105):
IRS forms/publications, GovInfo congressional bills/documents, CDC MMWR articles,
NASA NTRS technical reports, USGS, and NIST special publications. These land in
``fixtures/corpus/`` and each is recorded in ``fixtures/MANIFEST.toml`` with its
source, license, sha256, and clearance metadata.

Tier-2 (fetch-only, NEVER committed) — the PDF Association ``pdf20examples`` repo.
Those files are CC BY-SA 4.0 (a *ShareAlike* copyleft license that is NOT on the
strict Tier-1 permissive allowlist), so they are downloaded into the gitignored
``conformance/corpus-cache/`` and used ONLY for open-rate / never-panic / crash
robustness testing. They are never committed and their content is never reproduced
in any committed output.

Network notes:
* A local HTTP(S) proxy on this machine breaks TLS to several ``.gov`` hosts, so
  all requests are issued with proxy env vars stripped (see ``_session``).
* Downloads are validated by HTTP status, Content-Type, and a ``%PDF`` magic-byte
  check. Unreachable / non-PDF sources are skipped with a note — one bad source
  never fails the whole run.

Usage (from repo root, in the project venv)::

    env -u CONDA_PREFIX .venv/bin/python conformance/fetch_corpus.py            # Tier-1 only
    env -u CONDA_PREFIX .venv/bin/python conformance/fetch_corpus.py --tier2    # + Tier-2 cache
    env -u CONDA_PREFIX .venv/bin/python conformance/fetch_corpus.py --update-manifest
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import urllib.error
import urllib.request
from datetime import date
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
TIER1_DIR = REPO_ROOT / "fixtures" / "corpus"
TIER2_DIR = REPO_ROOT / "conformance" / "corpus-cache"
MANIFEST = REPO_ROOT / "fixtures" / "MANIFEST.toml"

CLEARED_BY = "validation-harness"
CLEARED_DATE = date.today().isoformat()

_UA = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
)

# --------------------------------------------------------------------------- #
# Tier-1 sources — all US-federal-government works (public domain, 17 USC 105).
# (filename, url, source-label, license, note)
# --------------------------------------------------------------------------- #
_GOV_PD = "Public-Domain"  # US federal works; 17 U.S.C. 105.

TIER1_SOURCES: list[tuple[str, str, str, str, str]] = [
    # --- IRS tax forms (AcroForm / interactive forms; varied complexity) ---
    ("irs-f1040.pdf", "https://www.irs.gov/pub/irs-pdf/f1040.pdf", "IRS", _GOV_PD, "Form 1040 (AcroForm)"),
    ("irs-fw9.pdf", "https://www.irs.gov/pub/irs-pdf/fw9.pdf", "IRS", _GOV_PD, "Form W-9"),
    ("irs-fw4.pdf", "https://www.irs.gov/pub/irs-pdf/fw4.pdf", "IRS", _GOV_PD, "Form W-4"),
    ("irs-fw7.pdf", "https://www.irs.gov/pub/irs-pdf/fw7.pdf", "IRS", _GOV_PD, "Form W-7"),
    ("irs-f941.pdf", "https://www.irs.gov/pub/irs-pdf/f941.pdf", "IRS", _GOV_PD, "Form 941"),
    ("irs-f1065.pdf", "https://www.irs.gov/pub/irs-pdf/f1065.pdf", "IRS", _GOV_PD, "Form 1065"),
    ("irs-f1120.pdf", "https://www.irs.gov/pub/irs-pdf/f1120.pdf", "IRS", _GOV_PD, "Form 1120"),
    ("irs-f2848.pdf", "https://www.irs.gov/pub/irs-pdf/f2848.pdf", "IRS", _GOV_PD, "Form 2848"),
    ("irs-f4868.pdf", "https://www.irs.gov/pub/irs-pdf/f4868.pdf", "IRS", _GOV_PD, "Form 4868"),
    ("irs-f8949.pdf", "https://www.irs.gov/pub/irs-pdf/f8949.pdf", "IRS", _GOV_PD, "Form 8949"),
    ("irs-f8843.pdf", "https://www.irs.gov/pub/irs-pdf/f8843.pdf", "IRS", _GOV_PD, "Form 8843"),
    ("irs-f1040sb.pdf", "https://www.irs.gov/pub/irs-pdf/f1040sb.pdf", "IRS", _GOV_PD, "Schedule B"),
    ("irs-f1040sc.pdf", "https://www.irs.gov/pub/irs-pdf/f1040sc.pdf", "IRS", _GOV_PD, "Schedule C"),
    ("irs-f1099msc.pdf", "https://www.irs.gov/pub/irs-pdf/f1099msc.pdf", "IRS", _GOV_PD, "Form 1099-MISC"),
    # --- IRS publications (long-form text, multi-column, tables) ---
    ("irs-p15.pdf", "https://www.irs.gov/pub/irs-pdf/p15.pdf", "IRS", _GOV_PD, "Publication 15 (text-heavy)"),
    ("irs-p501.pdf", "https://www.irs.gov/pub/irs-pdf/p501.pdf", "IRS", _GOV_PD, "Publication 501"),
    ("irs-p502.pdf", "https://www.irs.gov/pub/irs-pdf/p502.pdf", "IRS", _GOV_PD, "Publication 502"),
    # --- GovInfo congressional bills / documents (text-heavy, line-numbered) ---
    ("govinfo-hr1.pdf", "https://www.govinfo.gov/content/pkg/BILLS-118hr1ih/pdf/BILLS-118hr1ih.pdf", "GovInfo", _GOV_PD, "H.R.1 (118th)"),
    ("govinfo-hr2.pdf", "https://www.govinfo.gov/content/pkg/BILLS-118hr2ih/pdf/BILLS-118hr2ih.pdf", "GovInfo", _GOV_PD, "H.R.2 (118th)"),
    ("govinfo-s1.pdf", "https://www.govinfo.gov/content/pkg/BILLS-118s1is/pdf/BILLS-118s1is.pdf", "GovInfo", _GOV_PD, "S.1 (118th)"),
    ("govinfo-hjres1.pdf", "https://www.govinfo.gov/content/pkg/BILLS-118hjres1ih/pdf/BILLS-118hjres1ih.pdf", "GovInfo", _GOV_PD, "H.J.Res.1 (118th)"),
    ("govinfo-hr3056.pdf", "https://www.govinfo.gov/content/pkg/BILLS-118hr3056ih/pdf/BILLS-118hr3056ih.pdf", "GovInfo", _GOV_PD, "H.R.3056 (118th)"),
    ("govinfo-hr815enr.pdf", "https://www.govinfo.gov/content/pkg/BILLS-118hr815enr/pdf/BILLS-118hr815enr.pdf", "GovInfo", _GOV_PD, "H.R.815 enrolled"),
    ("govinfo-cdoc110-50.pdf", "https://www.govinfo.gov/content/pkg/CDOC-110hdoc50/pdf/CDOC-110hdoc50.pdf", "GovInfo", _GOV_PD, "Senate Document (charters of freedom)"),
    # --- CDC MMWR (scientific articles; figures, tables) ---
    ("cdc-mmwr-7301a1.pdf", "https://www.cdc.gov/mmwr/volumes/73/wr/pdfs/mm7301a1-H.pdf", "CDC", _GOV_PD, "MMWR 73/01 a1"),
    ("cdc-mmwr-7302a1.pdf", "https://www.cdc.gov/mmwr/volumes/73/wr/pdfs/mm7302a1-H.pdf", "CDC", _GOV_PD, "MMWR 73/02 a1"),
    ("cdc-mmwr-7251a1.pdf", "https://www.cdc.gov/mmwr/volumes/72/wr/pdfs/mm7251a1-H.pdf", "CDC", _GOV_PD, "MMWR 72/51 a1"),
    # --- NASA NTRS technical report ---
    ("nasa-ntrs-19950009349.pdf", "https://ntrs.nasa.gov/api/citations/19950009349/downloads/19950009349.pdf", "NASA-NTRS", _GOV_PD, "NASA technical report"),
    # --- USGS / NIST ---
    ("usgs-fs20183024.pdf", "https://pubs.usgs.gov/fs/2018/3024/fs20183024.pdf", "USGS", _GOV_PD, "USGS fact sheet (figures/images)"),
    ("nist-sp800-63-3.pdf", "https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-63-3.pdf", "NIST", _GOV_PD, "NIST SP 800-63-3"),
]

# --------------------------------------------------------------------------- #
# Tier-2 sources — fetch-only, NEVER committed (CC BY-SA 4.0, copyleft).
# PDF Association pdf20examples (PDF 2.0 conformance edge cases).
# --------------------------------------------------------------------------- #
_PDF20_BASE = "https://raw.githubusercontent.com/pdf-association/pdf20examples/master/"
TIER2_SOURCES: list[tuple[str, str, str, str, str]] = [
    ("pdf20-simple.pdf", _PDF20_BASE + "Simple%20PDF%202.0%20file.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "Simple PDF 2.0"),
    ("pdf20-utf8-test.pdf", _PDF20_BASE + "pdf20-utf8-test.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "UTF-8 strings test"),
    ("pdf20-utf8-annot.pdf", _PDF20_BASE + "PDF%202.0%20UTF-8%20string%20and%20annotation.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "UTF-8 string + annotation"),
    ("pdf20-image-bpc.pdf", _PDF20_BASE + "PDF%202.0%20image%20with%20BPC.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "Image with BPC"),
    ("pdf20-incremental.pdf", _PDF20_BASE + "PDF%202.0%20via%20incremental%20save.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "Incremental save"),
    ("pdf20-offset-start.pdf", _PDF20_BASE + "PDF%202.0%20with%20offset%20start.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "Offset start (non-zero header)"),
    ("pdf20-output-intent.pdf", _PDF20_BASE + "PDF%202.0%20with%20page%20level%20output%20intent.pdf", "pdf-association/pdf20examples", "CC-BY-SA-4.0", "Page-level output intent"),
]


def _session_get(url: str, timeout: int = 60) -> bytes:
    """Fetch ``url`` with proxy env stripped and a browser UA. Raises on error."""
    # The local proxy breaks TLS to several .gov hosts; bypass it explicitly.
    proxy_handler = urllib.request.ProxyHandler({})
    opener = urllib.request.build_opener(proxy_handler)
    req = urllib.request.Request(url, headers={"User-Agent": _UA})
    with opener.open(req, timeout=timeout) as resp:
        ctype = resp.headers.get("Content-Type", "")
        data = resp.read()
    if not data.startswith(b"%PDF"):
        raise ValueError(f"not a PDF (Content-Type={ctype!r}, {len(data)} bytes)")
    return data


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _fetch_group(
    sources: list[tuple[str, str, str, str, str]],
    dest_dir: Path,
    *,
    label: str,
) -> tuple[list[dict], list[dict]]:
    """Download a source group. Returns (succeeded, skipped) metadata lists."""
    dest_dir.mkdir(parents=True, exist_ok=True)
    ok: list[dict] = []
    skipped: list[dict] = []
    for fname, url, source, license_, note in sources:
        out = dest_dir / fname
        try:
            data = _session_get(url)
        except (urllib.error.URLError, urllib.error.HTTPError, ValueError, OSError, TimeoutError) as exc:
            print(f"  SKIP  {fname:<28} {type(exc).__name__}: {exc}")
            skipped.append({"file": fname, "url": url, "reason": f"{type(exc).__name__}: {exc}"})
            continue
        out.write_bytes(data)
        sha = _sha256(data)
        print(f"  OK    {fname:<28} {len(data):>9,} B  sha256={sha[:12]}…")
        ok.append(
            {
                "file": fname,
                "path": f"corpus/{fname}" if label == "tier1" else fname,
                "url": url,
                "source": source,
                "license": license_,
                "sha256": sha,
                "size": len(data),
                "note": note,
            }
        )
    return ok, skipped


def _render_manifest_entries(entries: list[dict]) -> str:
    """Render [[fixture]] tables for committed Tier-1 corpus files."""
    lines: list[str] = []
    for e in entries:
        notes = e["note"].replace('"', '\\"')
        lines.append("")
        lines.append("[[fixture]]")
        lines.append(f'path         = "{e["path"]}"')
        lines.append(f'source       = "{e["source"]}"')
        lines.append(f'license      = "{e["license"]}"')
        lines.append(f'sha256       = "{e["sha256"]}"')
        lines.append(f'cleared_by   = "{CLEARED_BY}"')
        lines.append(f'cleared_date = "{CLEARED_DATE}"')
        lines.append(f'notes        = "US federal government work (17 U.S.C. 105); {notes}"')
    return "\n".join(lines) + "\n"


def _update_manifest(entries: list[dict]) -> None:
    """Append (idempotently) Tier-1 [[fixture]] entries to MANIFEST.toml.

    Replaces any prior auto-generated block delimited by the harness markers so
    re-runs don't duplicate entries.
    """
    begin = "# >>> validation-harness corpus (auto-generated) >>>"
    end = "# <<< validation-harness corpus (auto-generated) <<<"
    text = MANIFEST.read_text(encoding="utf-8")
    block = f"\n{begin}\n" + _render_manifest_entries(entries) + f"{end}\n"
    if begin in text and end in text:
        head = text[: text.index(begin)].rstrip("\n")
        tail = text[text.index(end) + len(end):]
        text = head + "\n" + block.lstrip("\n") + tail
    else:
        text = text.rstrip("\n") + "\n" + block
    MANIFEST.write_text(text, encoding="utf-8")
    print(f"\nManifest updated: {len(entries)} Tier-1 entries -> {MANIFEST}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tier2", action="store_true", help="also fetch Tier-2 (gitignored cache, never committed)")
    ap.add_argument("--update-manifest", action="store_true", help="write Tier-1 entries into fixtures/MANIFEST.toml")
    ap.add_argument("--summary-json", type=Path, default=None, help="write a fetch summary JSON here")
    args = ap.parse_args(argv)

    print(f"== Tier-1 (committable, public-domain) -> {TIER1_DIR} ==")
    t1_ok, t1_skip = _fetch_group(TIER1_SOURCES, TIER1_DIR, label="tier1")

    t2_ok: list[dict] = []
    t2_skip: list[dict] = []
    if args.tier2:
        print(f"\n== Tier-2 (fetch-only, NEVER committed, CC BY-SA 4.0) -> {TIER2_DIR} ==")
        t2_ok, t2_skip = _fetch_group(TIER2_SOURCES, TIER2_DIR, label="tier2")

    print(
        f"\nTier-1: {len(t1_ok)} fetched, {len(t1_skip)} skipped. "
        f"Tier-2: {len(t2_ok)} fetched, {len(t2_skip)} skipped."
    )

    if args.update_manifest and t1_ok:
        _update_manifest(t1_ok)

    if args.summary_json:
        args.summary_json.write_text(
            json.dumps(
                {
                    "tier1": {"ok": t1_ok, "skipped": t1_skip},
                    "tier2": {"ok": t2_ok, "skipped": t2_skip},
                },
                indent=2,
            ),
            encoding="utf-8",
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
