"""Owned callback replay; deliberately independent of native MuPDF devices."""

from __future__ import annotations

from operator import index
from threading import Lock
from typing import TYPE_CHECKING, Any, Callable, cast

from . import _core

if TYPE_CHECKING:
    from .document import TextPage

ReplayEvent = _core.ReplayEvent


class ReplayDevice:
    """Call ``callback(event)`` in recording order; no native device handles.

    Exceptions stop replay unchanged. Successful runs end with an ``end`` event.
    Closing is idempotent except while running; concurrent/recursive runs fail.
    """

    def __init__(self, callback: Callable[[ReplayEvent], object]):
        if not callable(callback):
            raise TypeError("callback must be callable")
        self._callback = callback
        self._target: TextPage | None = None
        self._pixmap: _core.Pixmap | None = None
        self._flags = 0
        self._lock = Lock()
        self._running = False
        self._closed = False

    @classmethod
    def for_textpage(cls, target: TextPage, flags: int = 0) -> ReplayDevice:
        """Append owned segments to a TextPage, atomically preserving its identity.

        Area selects whole recorded operations conservatively; the target rect
        independently clips transformed glyph origins. Flags apply to new content.
        """
        from .document import TextPage

        if not isinstance(target, TextPage):
            raise TypeError("target must be a TextPage")
        flags = index(flags)
        if flags < 0 or flags > 0xFFFFFFFF:
            raise OverflowError("flags must fit an unsigned 32-bit integer")
        device = cls(lambda _event: None)
        device._target = target
        device._flags = flags
        return device

    @classmethod
    def for_pixmap(cls, target: _core.Pixmap) -> ReplayDevice:
        """Paint onto existing straight RGB(A) samples with atomic copy-on-write.

        Area selects operations, not a new pixel clip. Target origin positions the
        raster in final device coordinates; DPI and existing exports are preserved.
        """
        if not isinstance(target, _core.Pixmap):
            raise TypeError("target must be a Pixmap")
        if target.colorspace != "DeviceRGB":
            raise ValueError("replay target must be RGB or RGBA")
        device = cls(lambda _event: None)
        device._pixmap = target
        return device

    def close(self) -> None:
        with self._lock:
            if self._running:
                raise RuntimeError("replay device is running")
            self._closed = True


def _run(record: Any, device: object, matrix: Any, area: Any) -> None:
    if not isinstance(device, ReplayDevice):
        raise TypeError("expected pdfspine.ReplayDevice, not a native device")
    with device._lock:
        if device._running:
            raise RuntimeError("replay device is running")
        if device._closed:
            raise RuntimeError("replay device is closed")
        device._running = True
    try:
        m = None if matrix is None else tuple(float(v) for v in matrix)
        a = None if area is None else tuple(float(v) for v in area)
        if m is not None and len(m) != 6:
            raise ValueError("matrix must contain six coefficients")
        if a is not None and len(a) != 4:
            raise ValueError("area must contain four coordinates")
        if device._pixmap is not None:
            cast(Any, device._pixmap)._replay_pixmap(record, m, a)
        elif device._target is not None:
            # The public stub deliberately omits the internal frozen core slot.
            target = cast(Any, device._target)
            original = target._tp
            replacement = record._replay_textpage(original, device._flags, m, a)
            if target._tp is not original:
                raise RuntimeError("target TextPage changed during replay")
            if replacement is not None:
                target._tp = replacement
        else:
            events = record._replay_prepare(m, a)
            for event in events:
                device._callback(event)
    finally:
        with device._lock:
            device._running = False
