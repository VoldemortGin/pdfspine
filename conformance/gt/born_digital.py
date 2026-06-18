#!/usr/bin/env python3
"""Born-digital multi-column PDF generator with PERFECT reading-order ground truth.

Part of the OBJECTIVE ground-truth accuracy subsystem for pdfspine. The existing
``conformance/run_validation.py`` scores our text extraction against PyMuPDF as a
*pseudo*-oracle (fitz is not truth). This module manufactures PDFs whose true
reading order is known by construction, so pdfspine / fitz / pdfminer can all be
scored against the SAME objective truth — the known weak spot being multi-column
reading order.

Why the ground truth is trustworthy:
    We lay paragraphs out with CSS multi-column (``column-count: N``). The browser
    *flows* content: it fills column 1 top-to-bottom, then column 2, etc. So the
    visual reading order == the source DOM order == our ground truth. Chrome may
    serialize the PDF content stream row-major across columns even though a human
    reads column-major — that mismatch is exactly the extraction challenge we want
    to measure, and our ground truth tells the right (column-major) answer.

Ground truth = the header text (if any) first, then the paragraph texts in source
order, joined by blank lines. Nothing the page does not contain is ever added.

Rendering: headless Chrome (BSD-licensed) via ``--headless --print-to-pdf``, with
page-number header/footer disabled and @page margins zeroed so nothing pollutes
the ground truth.

Text source: public-domain Project Gutenberg prose (PG header/footer stripped),
cached under ``conformance/gt/cache/``. If the network is unavailable, falls back
to a bundled ~40-paragraph public-domain snippet embedded below.

CLI::

    .venv/bin/python conformance/gt/born_digital.py --out conformance/gt/corpus-born

Self-test (no args)::

    .venv/bin/python conformance/gt/born_digital.py --self-test
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

GT_DIR = Path(__file__).resolve().parent
CACHE_DIR = GT_DIR / "cache"

CHROME_CANDIDATES = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    "chrome",
    "google-chrome",
    "chromium",
]

_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) pdfspine-gt/1.0"

# Project Gutenberg plain-text books (public domain). (id, label).
PG_BOOKS = [
    (1342, "pride-and-prejudice"),  # Jane Austen, 1813
    (1661, "sherlock-holmes"),      # Arthur Conan Doyle, 1892
]

# Default variants to generate (all of them).
DEFAULT_VARIANTS = [
    "1col",
    "2col",
    "2col-justified",
    "3col",
    "2col-with-header",
    "2col-narrow-gutter",
]

# How many paragraphs to flow into each PDF body (enough to span columns).
N_PARAGRAPHS = 28

HEADER_TEXT = "On First Principles of Quiet Country Living"


# --------------------------------------------------------------------------- #
# Bundled fallback prose — public domain (Jane Austen, *Pride and Prejudice*,
# 1813; and a few lines of Conan Doyle, *Adventures of Sherlock Holmes*, 1892).
# Used only when the network is unavailable. ~40 short prose paragraphs.
# --------------------------------------------------------------------------- #
_FALLBACK_PARAGRAPHS = [
    "It is a truth universally acknowledged, that a single man in possession of a good fortune, must be in want of a wife.",
    "However little known the feelings or views of such a man may be on his first entering a neighbourhood, this truth is so well fixed in the minds of the surrounding families, that he is considered the rightful property of some one or other of their daughters.",
    "My dear Mr. Bennet, said his lady to him one day, have you heard that Netherfield Park is let at last?",
    "Mr. Bennet replied that he had not.",
    "But it is, returned she; for Mrs. Long has just been here, and she told me all about it.",
    "Mr. Bennet made no answer.",
    "Do you not want to know who has taken it? cried his wife impatiently.",
    "You want to tell me, and I have no objection to hearing it.",
    "This was invitation enough.",
    "Why, my dear, you must know, Mrs. Long says that Netherfield is taken by a young man of large fortune from the north of England; that he came down on Monday in a chaise and four to see the place, and was so much delighted with it that he agreed with Mr. Morris immediately.",
    "He is to take possession before Michaelmas, and some of his servants are to be in the house by the end of next week.",
    "What is his name?",
    "Bingley.",
    "Is he married or single?",
    "Oh! single, my dear, to be sure! A single man of large fortune; four or five thousand a year. What a fine thing for our girls!",
    "How so? how can it affect them?",
    "My dear Mr. Bennet, replied his wife, how can you be so tiresome! You must know that I am thinking of his marrying one of them.",
    "Is that his design in settling here?",
    "Design! nonsense, how can you talk so! But it is very likely that he may fall in love with one of them, and therefore you must visit him as soon as he comes.",
    "I see no occasion for that. You and the girls may go, or you may send them by themselves, which perhaps will be still better; for as you are as handsome as any of them, Mr. Bingley might like you the best of the party.",
    "My dear, you flatter me. I certainly have had my share of beauty, but I do not pretend to be anything extraordinary now.",
    "When a woman has five grown-up daughters, she ought to give over thinking of her own beauty.",
    "In such cases, a woman has not often much beauty to think of.",
    "But, my dear, you must indeed go and see Mr. Bingley when he comes into the neighbourhood.",
    "It is more than I engage for, I assure you.",
    "But consider your daughters. Only think what an establishment it would be for one of them.",
    "Sir William and Lady Lucas are determined to go, merely on that account; for in general, you know, they visit no new-comers.",
    "Indeed you must go, for it will be impossible for us to visit him, if you do not.",
    "To Sherlock Holmes she is always the woman. I have seldom heard him mention her under any other name.",
    "In his eyes she eclipses and predominates the whole of her sex.",
    "It was not that he felt any emotion akin to love for Irene Adler.",
    "All emotions, and that one particularly, were abhorrent to his cold, precise but admirably balanced mind.",
    "He was, I take it, the most perfect reasoning and observing machine that the world has seen.",
    "But as a lover he would have placed himself in a false position.",
    "He never spoke of the softer passions, save with a gibe and a sneer.",
    "They were admirable things for the observer, excellent for drawing the veil from men's motives and actions.",
    "I had seen little of Holmes lately. My marriage had drifted us away from each other.",
    "My own complete happiness, and the home-centred interests which rise up around the man who first finds himself master of his own establishment, were sufficient to absorb all my attention.",
    "One night, it was on the twentieth of March, I was returning from a journey to a patient.",
    "My way led me through Baker Street. As I passed the well-remembered door, I was seized with a keen desire to see Holmes again.",
]


# --------------------------------------------------------------------------- #
# Text fetching / paragraph extraction
# --------------------------------------------------------------------------- #
def _pg_url(book_id: int) -> str:
    return f"https://www.gutenberg.org/cache/epub/{book_id}/pg{book_id}.txt"


def _fetch_text(url: str, timeout: int = 30) -> str:
    """Fetch UTF-8 text with proxy env stripped (the local proxy breaks some TLS)."""
    proxy_handler = urllib.request.ProxyHandler({})
    opener = urllib.request.build_opener(proxy_handler)
    req = urllib.request.Request(url, headers={"User-Agent": _UA})
    with opener.open(req, timeout=timeout) as resp:
        raw = resp.read()
    return raw.decode("utf-8", errors="replace")


_START_RE = re.compile(r"\*\*\*\s*START OF.*?\*\*\*", re.IGNORECASE | re.DOTALL)
_END_RE = re.compile(r"\*\*\*\s*END OF.*?\*\*\*", re.IGNORECASE | re.DOTALL)


def _strip_pg_boilerplate(text: str) -> str:
    """Return only the body between the PG START/END markers."""
    start = _START_RE.search(text)
    body = text[start.end():] if start else text
    end = _END_RE.search(body)
    if end:
        body = body[: end.start()]
    return body


def _split_paragraphs(text: str) -> list[str]:
    """Split a Gutenberg body into clean single-line prose paragraphs.

    Paragraphs are blank-line separated. Internal newlines are collapsed to
    spaces so each paragraph is one normalized line (matching how the browser
    will lay it out and how extractors emit it).
    """
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    chunks = re.split(r"\n[ \t]*\n", text)
    paras: list[str] = []
    for chunk in chunks:
        flat = re.sub(r"\s+", " ", chunk).strip()
        if not flat:
            continue
        # Skip headings / chapter markers / very short bits.
        if len(flat) < 80:
            continue
        if re.match(r"^(chapter|CHAPTER|VOLUME|Volume|CONTENTS|ILLUSTRATIONS)\b", flat):
            continue
        # Skip Gutenberg italics-wrapped editorial front matter (prefaces,
        # dedications) and all-caps press/imprint lines — keep real narrative.
        if flat.startswith("_") or flat.startswith("“_") or flat.startswith("["):
            continue
        letters = [c for c in flat if c.isalpha()]
        if letters and sum(c.isupper() for c in letters) / len(letters) > 0.6:
            continue
        # Must read like prose: contain sentence punctuation and lowercase words.
        if not re.search(r"[a-z]{3,}", flat):
            continue
        paras.append(flat)
    return paras


def _load_paragraphs(n: int) -> tuple[list[str], str]:
    """Return (paragraphs, source_note). Tries cache, then network, then fallback."""
    # 1) Cached files first.
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    collected: list[str] = []
    used: list[str] = []
    for book_id, label in PG_BOOKS:
        cache_path = CACHE_DIR / f"{label}-{book_id}.txt"
        body = ""
        if cache_path.exists():
            try:
                body = cache_path.read_text(encoding="utf-8")
                used.append(f"cache:{label}")
            except OSError:
                body = ""
        if not body:
            try:
                raw = _fetch_text(_pg_url(book_id))
                body = _strip_pg_boilerplate(raw)
                cache_path.write_text(body, encoding="utf-8")
                used.append(f"net:{label}")
            except Exception as exc:  # noqa: BLE001
                used.append(f"fail:{label}({type(exc).__name__})")
                body = ""
        if body:
            collected.extend(_split_paragraphs(body))
        if len(collected) >= n:
            break

    if len(collected) >= n:
        return collected[:n], "gutenberg(" + ",".join(used) + ")"

    # 2) Fallback bundled snippet (public domain).
    note = "fallback-bundled"
    if used:
        note += " after " + ",".join(used)
    paras = list(_FALLBACK_PARAGRAPHS)
    # If we got *some* real paragraphs, prefer them, padded with fallback.
    if collected:
        merged = collected + [p for p in paras if p not in collected]
        return merged[:n], "partial-gutenberg+fallback(" + ",".join(used) + ")"
    return paras[:n], note


# --------------------------------------------------------------------------- #
# Ground truth + HTML construction
# --------------------------------------------------------------------------- #
def _ground_truth(paragraphs: list[str], header: str | None) -> str:
    """Exact source-order reading text: header first (if any), then paragraphs."""
    parts: list[str] = []
    if header:
        parts.append(header)
    parts.extend(paragraphs)
    return "\n\n".join(parts)


def _build_html(
    paragraphs: list[str],
    *,
    columns: int,
    justified: bool,
    header: str | None,
    gap_px: int,
) -> str:
    """Build a Letter-size, zero-margin, multi-column HTML document."""
    text_align = "justify" if justified else "left"
    header_html = ""
    if header:
        header_html = (
            f'  <h1 class="hdr">{html.escape(header)}</h1>\n'
        )
    paras_html = "\n".join(
        f'    <p>{html.escape(p)}</p>' for p in paragraphs
    )
    # column-count on the body wrapper makes the browser FLOW paragraphs
    # column-major (fill col 1 top-to-bottom, then col 2, ...). The <h1> sits
    # outside the multicol wrapper so it spans full width (header variant).
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>
  @page {{
    size: Letter;
    margin: 0;
  }}
  html, body {{
    margin: 0;
    padding: 0;
  }}
  body {{
    font-family: Georgia, "Times New Roman", serif;
    font-size: 10pt;
    line-height: 1.45;
    color: #000;
    padding: 0.6in 0.6in 0.6in 0.6in;
    box-sizing: border-box;
  }}
  h1.hdr {{
    font-size: 18pt;
    font-weight: bold;
    margin: 0 0 14px 0;
    padding: 0;
    text-align: left;
    column-span: all;
  }}
  .cols {{
    column-count: {columns};
    column-gap: {gap_px}px;
    column-fill: auto;
    height: 9.0in;
  }}
  .cols p {{
    margin: 0 0 9px 0;
    padding: 0;
    text-align: {text_align};
    -webkit-hyphens: none;
    hyphens: none;
    orphans: 2;
    widows: 2;
  }}
</style>
</head>
<body>
{header_html}  <div class="cols">
{paras_html}
  </div>
</body>
</html>
"""


# --------------------------------------------------------------------------- #
# Chrome rendering
# --------------------------------------------------------------------------- #
def _find_chrome() -> str | None:
    """Locate a usable Chrome/Chromium binary (verified with --version)."""
    import shutil

    for cand in CHROME_CANDIDATES:
        path = cand
        if os.path.sep not in cand:
            found = shutil.which(cand)
            if not found:
                continue
            path = found
        if not os.path.exists(path):
            continue
        try:
            proc = subprocess.run(
                [path, "--version"], capture_output=True, text=True, timeout=20
            )
            if proc.returncode == 0 and proc.stdout.strip():
                return path
        except Exception:  # noqa: BLE001
            continue
    return None


def _chrome_cmd(chrome: str, profile: str, html_path: Path, pdf_path: Path) -> list[str]:
    """Build the headless-Chrome print-to-pdf command line."""
    return [
        chrome,
        "--headless",
        "--disable-gpu",
        "--no-sandbox",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-crash-reporter",
        "--disable-breakpad",
        "--no-pings",
        "--disable-component-update",
        "--disable-background-networking",
        f"--user-data-dir={profile}",
        "--no-pdf-header-footer",
        f"--print-to-pdf={pdf_path}",
        html_path.as_uri(),
    ]


def _kill_tree(proc: subprocess.Popen) -> None:
    """Terminate a Chrome process group hard (it spawns helpers that linger)."""
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError, OSError):
        try:
            proc.kill()
        except OSError:
            pass
    try:
        proc.wait(timeout=10)
    except Exception:  # noqa: BLE001
        pass


def _render_pdf(chrome: str, html_path: Path, pdf_path: Path,
                timeout: float = 60.0) -> tuple[bool, str]:
    """Render an HTML file to PDF via headless Chrome. Returns (ok, diagnostic).

    On this machine (and others with a background updater/crashpad), Chrome writes
    the PDF within a few seconds but then HANGS instead of exiting. So rather than
    wait for a clean exit, we poll for the output file, wait until its size is
    stable and it starts with ``%PDF``, then kill the Chrome process group.
    """
    if pdf_path.exists():
        pdf_path.unlink()
    # Throwaway profile dir per render keeps runs hermetic and avoids clobbering a
    # live Chrome session.
    profile = tempfile.mkdtemp(prefix="pdfspine-gt-chrome-")
    cmd = _chrome_cmd(chrome, profile, html_path, pdf_path)
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,  # own process group -> killpg cleans helpers
    )
    deadline = time.monotonic() + timeout
    last_size = -1
    stable_since = None
    ok = False
    diag = ""
    try:
        while time.monotonic() < deadline:
            if pdf_path.exists():
                try:
                    size = pdf_path.stat().st_size
                except OSError:
                    size = 0
                if size > 0 and size == last_size:
                    if stable_since is None:
                        stable_since = time.monotonic()
                    elif time.monotonic() - stable_since >= 0.6:
                        # Size held steady -> file fully flushed.
                        ok = pdf_path.read_bytes()[:4] == b"%PDF"
                        diag = "ok" if ok else "output exists but is not a PDF"
                        break
                else:
                    stable_since = None
                last_size = size
            # If Chrome exited on its own (clean machines), check immediately.
            if proc.poll() is not None and pdf_path.exists():
                ok = pdf_path.stat().st_size > 0 and pdf_path.read_bytes()[:4] == b"%PDF"
                diag = "ok (chrome exited cleanly)" if ok else \
                    f"chrome exited rc={proc.returncode}, no valid PDF"
                break
            if proc.poll() is not None and not pdf_path.exists():
                stderr = (proc.stderr.read() or b"").decode("utf-8", "replace")
                diag = (f"chrome exited rc={proc.returncode} without producing a PDF.\n"
                        f"  cmd: {' '.join(cmd)}\n  stderr: {stderr.strip()[:600]!r}")
                break
            time.sleep(0.2)
        else:
            diag = (f"timeout after {timeout}s waiting for PDF.\n"
                    f"  cmd: {' '.join(cmd)}")
    finally:
        _kill_tree(proc)
        try:
            import shutil
            shutil.rmtree(profile, ignore_errors=True)
        except Exception:  # noqa: BLE001
            pass
    return ok, diag


# --------------------------------------------------------------------------- #
# Variant specs
# --------------------------------------------------------------------------- #
def _variant_spec(name: str) -> dict:
    """Map a variant name to layout parameters. Raises ValueError if unknown."""
    specs: dict[str, dict] = {
        "1col": {"columns": 1, "justified": False, "header": False, "gap": 24},
        "2col": {"columns": 2, "justified": False, "header": False, "gap": 24},
        "2col-justified": {"columns": 2, "justified": True, "header": False, "gap": 24},
        "3col": {"columns": 3, "justified": False, "header": False, "gap": 24},
        "2col-with-header": {"columns": 2, "justified": False, "header": True, "gap": 24},
        "2col-narrow-gutter": {"columns": 2, "justified": False, "header": False, "gap": 10},
    }
    if name not in specs:
        raise ValueError(
            f"unknown variant {name!r}; known: {', '.join(specs)}"
        )
    return specs[name]


# --------------------------------------------------------------------------- #
# Public API
# --------------------------------------------------------------------------- #
def generate(out_dir: Path, variants: list[str] | None = None) -> list[dict]:
    """Generate multi-column PDFs with perfect reading-order ground truth.

    Returns a list of manifest entries and also writes ``<out_dir>/manifest.json``.
    Each entry: {name, pdf (abs path), gt_text, columns, justified, header}.

    If Chrome cannot render, the HTML is still written next to where the PDF
    would be (``<name>.html``) so the failure is debuggable, the entry's ``pdf``
    points at the (missing) target, and a clear diagnostic is printed to stderr.
    """
    out_dir = Path(out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    variants = list(variants) if variants else list(DEFAULT_VARIANTS)

    paragraphs, source_note = _load_paragraphs(N_PARAGRAPHS)
    print(f"[born_digital] text source: {source_note} ({len(paragraphs)} paragraphs)",
          file=sys.stderr)

    chrome = _find_chrome()
    if chrome is None:
        print(
            "[born_digital] DIAGNOSTIC: no working Chrome/Chromium found. Tried: "
            + ", ".join(CHROME_CANDIDATES)
            + " — verify the binary responds to --version. HTML will still be written.",
            file=sys.stderr,
        )
    else:
        print(f"[born_digital] using chrome: {chrome}", file=sys.stderr)

    entries: list[dict] = []
    for name in variants:
        spec = _variant_spec(name)
        header = HEADER_TEXT if spec["header"] else None
        body_html = _build_html(
            paragraphs,
            columns=spec["columns"],
            justified=spec["justified"],
            header=header,
            gap_px=spec["gap"],
        )
        html_path = out_dir / f"{name}.html"
        html_path.write_text(body_html, encoding="utf-8")
        pdf_path = out_dir / f"{name}.pdf"

        gt_text = _ground_truth(paragraphs, header)

        if chrome is not None:
            ok, diag = _render_pdf(chrome, html_path, pdf_path)
            if not ok:
                print(f"[born_digital] DIAGNOSTIC for {name}: {diag}", file=sys.stderr)
            else:
                print(f"[born_digital] rendered {name} -> {pdf_path} ({diag})",
                      file=sys.stderr)

        entries.append({
            "name": name,
            "pdf": str(pdf_path.resolve()),
            "gt_text": gt_text,
            "columns": spec["columns"],
            "justified": spec["justified"],
            "header": spec["header"],
        })

    manifest_path = out_dir / "manifest.json"
    manifest_path.write_text(json.dumps(entries, indent=2, ensure_ascii=False),
                             encoding="utf-8")
    print(f"[born_digital] wrote manifest: {manifest_path.resolve()}", file=sys.stderr)
    return entries


# --------------------------------------------------------------------------- #
# Self-test
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    variants = ["2col", "2col-with-header"]
    with tempfile.TemporaryDirectory(prefix="pdfspine-gt-selftest-") as tmp:
        out = Path(tmp) / "corpus"
        entries = generate(out, variants=variants)

        assert len(entries) == len(variants), f"expected {len(variants)} entries"
        by_name = {e["name"]: e for e in entries}

        for name in variants:
            e = by_name[name]
            assert e["gt_text"].strip(), f"{name}: gt_text is empty"
            pdf = Path(e["pdf"])
            assert pdf.exists(), f"{name}: PDF not created at {pdf}"
            size = pdf.stat().st_size
            assert size > 1024, f"{name}: PDF too small ({size} bytes)"
            head = pdf.read_bytes()[:4]
            assert head == b"%PDF", f"{name}: not a PDF (head={head!r})"

        # Header variant: gt_text must START with the header text.
        hdr = by_name["2col-with-header"]
        assert hdr["header"] is True
        assert hdr["gt_text"].startswith(HEADER_TEXT), "header not first in gt_text"

        # Plain 2col must NOT contain the header text.
        assert HEADER_TEXT not in by_name["2col"]["gt_text"], \
            "plain 2col leaked header text into gt"

        # Manifest exists and is valid.
        manifest = out / "manifest.json"
        assert manifest.exists(), "manifest.json not written"
        loaded = json.loads(manifest.read_text(encoding="utf-8"))
        assert len(loaded) == len(variants)
        assert all(m["gt_text"].strip() for m in loaded), "manifest gt_text empty"

    print("born_digital.py self-test OK")
    print("variants:", ", ".join(variants))
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=None,
                    help="output directory for generated PDFs + manifest.json")
    ap.add_argument("--variants", nargs="*", default=None,
                    help=f"subset of variants (default: all = {', '.join(DEFAULT_VARIANTS)})")
    ap.add_argument("--self-test", action="store_true",
                    help="render 2col + 2col-with-header into a temp dir and assert")
    args = ap.parse_args(argv)

    if args.self_test or args.out is None:
        return _self_test()

    entries = generate(args.out, variants=args.variants)
    print(f"Generated {len(entries)} variant(s) into {Path(args.out).resolve()}:")
    for e in entries:
        status = "OK" if Path(e["pdf"]).exists() else "NO-PDF"
        print(f"  [{status}] {e['name']:<22} cols={e['columns']} "
              f"justified={e['justified']} header={e['header']} -> {e['pdf']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
