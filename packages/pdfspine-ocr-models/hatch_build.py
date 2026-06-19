"""Hatchling build hook for the ``pdfspine-ocr-models`` data distribution.

The 3 PP-OCRv4 ONNX models are the single source of truth in the pdfspine repo
at ``crates/pdf-ocr/models/`` (git-tracked, also used by the Rust dev fallback).
To avoid duplicating ~16 MB in git, this hook ``force_include``s those files into
both the sdist and the wheel at build time instead of vendoring a second copy.

Resolution of the models source dir (first that exists wins), so it works both
for an in-repo build AND for building a wheel from an unpacked sdist:

  1. ``<package>/pdfspine_ocr_models/`` — already-vendored ONNX (e.g. an sdist
     that carried them into the package dir);
  2. ``<package>/../../crates/pdf-ocr/models/`` — the in-repo source of truth.
"""

from __future__ import annotations

import os

from hatchling.builders.hooks.plugin.interface import BuildHookInterface

# The 3 ONNX weights live as data; the ~26 KB dict + PROVENANCE stay embedded /
# carried elsewhere. Only these are force-included from the repo source of truth.
_ONNX_FILES = ("ppocrv4_det.onnx", "ppocrv4_rec.onnx", "ppocrv2_cls.onnx")


class CustomBuildHook(BuildHookInterface):
    def _models_src_dir(self) -> str:
        candidates = (
            os.path.join(self.root, "pdfspine_ocr_models"),
            os.path.join(self.root, os.pardir, os.pardir, "crates", "pdf-ocr", "models"),
        )
        for cand in candidates:
            if all(os.path.isfile(os.path.join(cand, f)) for f in _ONNX_FILES):
                return os.path.abspath(cand)
        searched = "\n  ".join(os.path.abspath(c) for c in candidates)
        raise RuntimeError(
            "pdfspine-ocr-models: could not locate the PP-OCRv4 ONNX models "
            f"({', '.join(_ONNX_FILES)}). Searched:\n  {searched}\n"
            "Build this distribution from a full pdfspine checkout (the models are "
            "git-tracked at crates/pdf-ocr/models), or from its sdist."
        )

    def initialize(self, version: str, build_data: dict) -> None:
        src = self._models_src_dir()
        force_include = build_data.setdefault("force_include", {})
        for fname in _ONNX_FILES:
            # Wheel: land inside the import package. Sdist: land inside the
            # package dir too, so a later wheel build from the sdist finds them
            # via candidate (1) above. Same destination works for both targets.
            force_include[os.path.join(src, fname)] = os.path.join(
                "pdfspine_ocr_models", fname
            )
