#!/usr/bin/env python3
"""Smoke-test document HTML export from an installed pdfspine distribution."""

from __future__ import annotations

import tempfile
from pathlib import Path

import pdfspine


def main() -> None:
    doc = pdfspine.open()
    doc.set_metadata({"title": "Wheel <smoke>"})
    doc.new_page().insert_text((72, 72), "HTML wheel smoke")

    html = doc.to_html()
    assert html.startswith("<!doctype html>")
    assert '<meta charset="utf-8">' in html
    assert "<title>Wheel &lt;smoke&gt;</title>" in html
    assert "HTML wheel smoke" in html

    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / "document.html"
        assert doc.save_html(output) is None
        assert output.read_text(encoding="utf-8") == html

    print("OK: installed distribution exposes browser-ready Document HTML export.")


if __name__ == "__main__":
    main()
