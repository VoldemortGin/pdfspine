#!/usr/bin/env python3
"""Federal Register running-header placement — the reading-order acceptance set.

The govinfo FR subset (``conformance/gt/corpus-govinfo/``) ships no ground-truth
text, so it cannot go through ``run_gt.py``. What it *does* have is a running
header on nearly every page ("Federal Register / Vol. 91, No. 1 / …"), painted
last in the content stream and spanning both body columns — exactly the shape
that a paint-ordered block sequence gets wrong. Three content-free counts fall
out of ``get_text("blocks")`` and are directly comparable between extractors:

``header_pages``
    pages where a header block was found at all (a fragmented header may fail
    the regex, so a *lower* count is itself a defect);
``misplaced``
    pages where a body block (one starting at or below the header's bottom
    edge) is emitted *before* the header — the reading-order failure;
``fragmented``
    pages whose header band holds more than one block (the line-level gutter
    split cutting the header at the column gutter, see D4).

Run it once per extractor and diff the JSON. It never writes oracle text, only
these counts (see the clean-room posture in ``conformance/REPORT.md``)::

    .venv/bin/python conformance/gt/fr_header_order.py \\
        --manifest conformance/gt/corpus-govinfo/manifest.json \\
        --json fr-header-pdfspine.json

    .venv-oracle/bin/python conformance/gt/fr_header_order.py \\
        --engine fitz \\
        --manifest conformance/gt/corpus-govinfo/manifest.json \\
        --json fr-header-fitz.json
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]

# The FR running header, as it survives both extractors' word spacing.
HEADER_RE = re.compile(r"Federal\s+Register|Vol\.\s*\d+,\s*No\.\s*\d+\s*/")
# The header band: PyMuPDF and pdfspine both report device y-down page coords,
# and the FR header baseline sits ~40 pt from the top on a 792 pt page.
HEADER_BAND_Y = 60.0


def _page_blocks(page: Any) -> list[tuple[float, float, float, float, str]]:
    """`(x0, y0, x1, y1, text)` per text block, in the extractor's own order."""
    out = []
    for block in page.get_text("blocks"):
        if len(block) >= 7 and block[6] != 0:
            continue  # image block
        x0, y0, x1, y1, text = block[0], block[1], block[2], block[3], block[4]
        out.append((float(x0), float(y0), float(x1), float(y1), str(text)))
    return out


def scan_document(open_document: Any, pdf: Path) -> dict[str, int]:
    counts = {"pages": 0, "header_pages": 0, "misplaced": 0, "fragmented": 0}
    with open_document(str(pdf)) as doc:
        for page in doc:
            counts["pages"] += 1
            blocks = _page_blocks(page)
            header_index = next(
                (
                    i
                    for i, b in enumerate(blocks)
                    if b[1] < HEADER_BAND_Y and HEADER_RE.search(b[4])
                ),
                None,
            )
            if header_index is None:
                continue
            counts["header_pages"] += 1
            header = blocks[header_index]
            if any(b[1] >= header[3] for b in blocks[:header_index]):
                counts["misplaced"] += 1
            band = [b for b in blocks if b[1] < HEADER_BAND_Y]
            if len(band) > 1:
                counts["fragmented"] += 1
    return counts


def _open_document(engine: str) -> Any:
    if engine == "pdfspine":
        import pdfspine

        return pdfspine.open
    import fitz  # noqa: PLC0415 — oracle venv only

    return fitz.open


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--manifest", type=Path, required=True)
    ap.add_argument("--json", dest="json_out", type=Path, required=True)
    ap.add_argument("--engine", choices=("pdfspine", "fitz"), default="pdfspine")
    ap.add_argument(
        "--collection",
        default="FR",
        help="manifest `collection` to keep (default FR; empty string keeps all)",
    )
    args = ap.parse_args(argv)

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    entries = manifest if isinstance(manifest, list) else manifest.get("entries", [])
    open_document = _open_document(args.engine)

    per_document: dict[str, dict[str, int]] = {}
    total = {"pages": 0, "header_pages": 0, "misplaced": 0, "fragmented": 0}
    for entry in entries:
        if args.collection and entry.get("collection") != args.collection:
            continue
        pdf = Path(str(entry.get("pdf") or entry.get("path")))
        if not pdf.exists():
            pdf = args.manifest.parent / pdf.name
        if not pdf.exists():
            print(f"missing: {pdf}", file=sys.stderr)
            continue
        counts = scan_document(open_document, pdf)
        per_document[str(entry.get("name") or pdf.stem)] = counts
        for key, value in counts.items():
            total[key] += value

    payload = {
        "engine": args.engine,
        "collection": args.collection,
        "n_documents": len(per_document),
        "total": total,
        "by_document": per_document,
    }
    args.json_out.parent.mkdir(parents=True, exist_ok=True)
    args.json_out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(total, indent=2))
    print(f"Wrote {args.json_out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
