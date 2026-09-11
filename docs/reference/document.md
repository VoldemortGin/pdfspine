# Documents & pages

A `Document` is a parsed PDF; obtain one with [`pdfspine.open`](functions.md#pdfspine.open).
A `Page` is one page of a `Document`, obtained with `doc[i]` or `doc.load_page(i)`.

Use `doc.to_html()` to combine every page's HTML extraction into a complete
HTML5 document, or `doc.save_html(path)` to write it as UTF-8.

## Document

::: pdfspine.Document

## Page

::: pdfspine.Page

## Typed page content (pdfspine extension)

Frozen value objects returned by the native typed `Page` API —
`page.content_blocks()`, `page.link_annotations()` and
`page.filled_rectangles()`. They are a pdfspine-original extension, not part
of the PyMuPDF-compatible surface.

::: pdfspine.TextBlock

::: pdfspine.ImageBlock

::: pdfspine.LinkAnnotation

::: pdfspine.FilledRectangle

## Registering a page font

`Page.insert_font(fontname="helv", fontfile=None, fontbuffer=None)` returns a
positive font xref without adding page contents. An existing exact resource name
wins before source validation. New registrations support Core14 and standalone
glyf TrueType fonts; file input takes precedence over buffer input. TrueType
programs are embedded in full, so output can be larger than subset embedding.

Registered names work with `insert_text` and `insert_textbox`, including after
save/reopen: encoding and widths come from the stored font resource. Unicode
aliases sharing one glyph remain distinct in extraction. A missing character
raises `PdfUnsupportedError` before writing rather than silently substituting it.
This does not add shaping or change the existing textbox wrapping algorithm.

New CFF/CFF2 fonts, font collections and nondefault `set_simple`, `wmode` or
`encoding` values are unsupported and fail before PDF mutation. These parameters
remain in the signature for compatibility; this is not full font-mode parity.
