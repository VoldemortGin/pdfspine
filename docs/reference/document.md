# Documents & pages

A `Document` is a parsed PDF; obtain one with [`pdfspine.open`](functions.md#pdfspine.open).
A `Page` is one page of a `Document`, obtained with `doc[i]` or `doc.load_page(i)`.

Use `doc.to_html()` to combine every page's HTML extraction into a complete
HTML5 document, or `doc.save_html(path)` to write it as UTF-8.

## Document

::: pdfspine.Document

## Page

::: pdfspine.Page
