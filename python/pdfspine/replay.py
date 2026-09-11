"""Owned callback replay; deliberately independent of native MuPDF devices."""

from __future__ import annotations

from threading import Lock
from typing import Callable

from . import _core

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
        self._lock = Lock()
        self._running = False
        self._closed = False

    def close(self) -> None:
        with self._lock:
            if self._running:
                raise RuntimeError("replay device is running")
            self._closed = True


def _run(record, device, matrix, area) -> None:
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
        events = record._replay_prepare(m, a)
        for event in events:
            device._callback(event)
    finally:
        with device._lock:
            device._running = False
