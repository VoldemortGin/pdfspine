# Text extraction

A `TextPage` is a reusable, parsed text layer for one page. Build one with
`Page.get_textpage(...)` and pass it back via `textpage=` to `get_text` /
`search_for` to avoid re-parsing.

::: pdfspine.TextPage
