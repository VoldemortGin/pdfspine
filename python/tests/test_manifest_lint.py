"""Portable conformance manifests may retain URLs, but never local absolute paths."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest


@pytest.mark.parametrize(
    ("value", "invalid"),
    [
        ("https://publications.europa.eu/resource/celex/", False),
        ("http://example.com/corpus.pdf", False),
        ("s3://corpus/document.pdf", False),
        ("fixtures/born/document.pdf", False),
        ("C:/Users/test/document.pdf", True),
        (r"C:\Users\test\document.pdf", True),
        ("/Users/test/document.pdf", True),
        ("/home/test/document.pdf", True),
        ("/root/document.pdf", True),
    ],
)
def test_conformance_manifest_urls_and_machine_paths(
    tmp_path, monkeypatch, value, invalid
):
    script = Path(__file__).resolve().parents[2] / "scripts" / "manifest-lint.py"
    spec = importlib.util.spec_from_file_location("manifest_lint", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    manifest = tmp_path / "manifest.json"
    manifest.write_text(json.dumps({"source": value}), encoding="utf-8")
    monkeypatch.setattr(module, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(module, "_git_tracked", lambda pattern: [manifest])
    errors = module.lint_conformance_manifests()
    assert bool(errors) is invalid
