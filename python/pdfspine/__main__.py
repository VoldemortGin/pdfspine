"""``python -m pdfspine`` entry point — delegates to :func:`pdfspine.cli.main`."""

from __future__ import annotations

from .cli import main

if __name__ == "__main__":
    raise SystemExit(main())
