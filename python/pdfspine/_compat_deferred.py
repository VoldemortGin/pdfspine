"""Generated deferred-symbol set — do not edit by hand.

Regenerate with ``python3 scripts/_compat_catalog.py`` (derived from the
same catalog as COMPAT.toml). One entry per deferred baseline symbol,
spelled ``Class.member`` (or a bare module-level name).
"""

from __future__ import annotations

DEFERRED: frozenset[str] = frozenset(
    {
        "DisplayList.run",
        "Page.run",
        "Pixmap.warp",
        "Tools.set_subset_fontnames",
    }
)
