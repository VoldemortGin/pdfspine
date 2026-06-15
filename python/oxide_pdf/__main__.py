"""``python -m oxide_pdf`` entry point — delegates to :func:`oxide_pdf.cli.main`."""

from __future__ import annotations

from .cli import main

if __name__ == "__main__":
    raise SystemExit(main())
