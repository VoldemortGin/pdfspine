"""Generated deferred-symbol set — do not edit by hand.

Regenerate with ``python3 scripts/_compat_catalog.py`` (derived from the
same catalog as COMPAT.toml). One entry per deferred baseline symbol,
spelled ``Class.member`` (or a bare module-level name).
"""

from __future__ import annotations

DEFERRED: frozenset[str] = frozenset(
    {
        "Annot.get_textbox",
        "DisplayList.get_textpage",
        "DisplayList.run",
        "Document.FormFonts",
        "Document.add_layer",
        "Document.get_layers",
        "Document.get_oc",
        "Document.get_ocmd",
        "Document.insert_file",
        "Document.set_layer_ui_config",
        "Document.set_ocmd",
        "Document.switch_layer",
        "Page.extend_textpage",
        "Page.insert_font",
        "Page.refresh",
        "Page.remove_rotation",
        "Page.run",
        "Page.write_text",
        "Pixmap.warp",
        "Tools.set_annot_stem",
        "Tools.set_subset_fontnames",
    }
)
