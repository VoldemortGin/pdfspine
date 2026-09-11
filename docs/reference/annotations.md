# Annotations & forms

`Annot` is a page annotation (created by `Page.add_*_annot(...)`); `Widget` is an
AcroForm field widget (from `Page.widgets()`).

## Annot

`annot.get_textbox(rect)` extracts only that annotation's visible appearance,
including custom text appearances on ordinary annotations. It does not return
underlying page text or fall back to `/Contents` when no appearance exists.
The query uses rotated page coordinates and includes intersecting character
boxes. A non-`None` `textpage` raises `ValueError`; prebuilt annotation-owned
text pages are not supported. The older `get_textpage` / `get_text` methods
still read the parent page region.

::: pdfspine.Annot

## Widget

::: pdfspine.Widget
