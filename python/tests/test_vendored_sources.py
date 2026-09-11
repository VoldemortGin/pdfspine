"""Vendor inventory and extension freshness must fail on unreviewed changes."""

import importlib.util
from pathlib import Path
import shutil
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[2]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    assert spec and spec.loader
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


def test_vendor_inventory_detects_modified_and_extra_source(tmp_path):
    checker = module("check_vendored_sources")
    shutil.copytree(ROOT / "vendor", tmp_path / "vendor")
    assert checker.verify(tmp_path) == 190
    source = tmp_path / "vendor/tiny-skia/src/pipeline/blitter.rs"
    original = source.read_bytes()
    source.write_bytes(original + b"\n// unreviewed change\n")
    with pytest.raises(ValueError, match="checksum mismatch"):
        checker.verify(tmp_path)
    source.write_bytes(original)
    extra = tmp_path / "vendor/tiny-skia/src/unreviewed.rs"
    extra.write_text("unreviewed")
    with pytest.raises(ValueError, match="inventory differs"):
        checker.verify(tmp_path)


def test_vendor_edit_invalidates_extension_fingerprint(tmp_path, monkeypatch):
    gate = module("quality_gate")
    monkeypatch.setattr(gate, "ROOT", tmp_path)
    subprocess.run(["git", "init", "--quiet", str(tmp_path)], check=True)
    source = tmp_path / "vendor/tiny-skia/src/pipeline/blitter.rs"
    source.parent.mkdir(parents=True)
    source.write_text("old")
    inputs = gate.extension_inputs()
    assert source in inputs
    before = gate.extension_fingerprint(inputs)
    source.write_text("new")
    assert gate.extension_fingerprint(gate.extension_inputs()) != before
