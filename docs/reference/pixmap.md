# Rendering

`Pixmap` is a native raster buffer; `DisplayList` is a recorded, replayable page
render. Both come from `Page.get_pixmap(...)` / `Page.get_displaylist()`.

## Pixmap

::: pdfspine.Pixmap

## DisplayList

::: pdfspine.DisplayList

## Replay devices and events

These are pdfspine extensions, not native FzDevice2 handle compatibility.
`Page.run` and `DisplayList.run` accept callback or typed replay targets; see
[the replay contract](../replay-callback-contract.md) for ownership, atomicity,
resource errors and operation-level area selection.

::: pdfspine.ReplayDevice

::: pdfspine.ReplayEvent
