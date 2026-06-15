#!/usr/bin/env python3
"""Isolated oxide_pdf worker — run inside the project venv (with our built wheel).

Invoked as a SUBPROCESS by ``run_validation.py`` so that a Rust panic/abort or a
hang on one input cannot take down the whole harness: the parent applies a wall
clock timeout and treats a non-zero exit / SIGABRT / timeout as a robustness
failure for that single document.

Reads one PDF path and prints a JSON object::

    {"opened": true, "repaired": false, "page_count": 3,
     "pages": ["page 0 text", ...],          # get_text("text") per page
     "save_ok": true, "save_path": "/tmp/...pdf",   # if --save given
     "error": null}

On a handled exception ``opened`` is false and ``error`` carries the message.
An *unhandled* abort never reaches this JSON — the parent detects it by exit code.
"""

from __future__ import annotations

import argparse
import json
import sys
import traceback


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--save", default=None, help="if set, doc.save() to this path")
    args = ap.parse_args(argv)

    out: dict = {
        "opened": False,
        "repaired": None,
        "page_count": None,
        "pages": [],
        "save_ok": None,
        "save_path": None,
        "error": None,
        "error_type": None,
    }

    try:
        import oxide_pdf
    except Exception as exc:  # noqa: BLE001
        out["error"] = f"import oxide_pdf failed: {exc}"
        out["error_type"] = type(exc).__name__
        sys.stdout.write(json.dumps(out))
        return 0

    doc = None
    try:
        doc = oxide_pdf.open(args.path)
        out["opened"] = True
        try:
            out["repaired"] = bool(doc.is_repaired)
        except Exception:  # noqa: BLE001
            out["repaired"] = None

        # Encrypted-but-openable docs: try empty-password auth so text works.
        try:
            if getattr(doc, "needs_pass", False):
                try:
                    doc.authenticate("")
                except Exception:  # noqa: BLE001
                    pass
        except Exception:  # noqa: BLE001
            pass

        n = doc.page_count
        out["page_count"] = n
        pages: list[str] = []
        for i in range(n):
            try:
                page = doc.load_page(i)
                pages.append(page.get_text("text"))
            except Exception as exc:  # noqa: BLE001
                pages.append("")
                if out["error"] is None:
                    out["error"] = f"page {i} get_text: {type(exc).__name__}: {exc}"
        out["pages"] = pages

        if args.save is not None:
            try:
                doc.save(args.save)
                out["save_ok"] = True
                out["save_path"] = args.save
            except Exception as exc:  # noqa: BLE001
                out["save_ok"] = False
                out["error"] = (out["error"] or "") + f" | save: {type(exc).__name__}: {exc}"
    except Exception as exc:  # noqa: BLE001
        out["error"] = f"{type(exc).__name__}: {exc}"
        out["error_type"] = type(exc).__name__
        out["traceback"] = traceback.format_exc()
    finally:
        if doc is not None:
            try:
                doc.close()
            except Exception:  # noqa: BLE001
                pass

    sys.stdout.write(json.dumps(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
