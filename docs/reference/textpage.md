# Text extraction

A `TextPage` is a reusable, parsed text layer for one page. Build one with
`Page.get_textpage(...)` and pass it back via `textpage=` to `get_text` /
`search_for` to avoid re-parsing.

`page.extend_textpage(target, flags=0, matrix=None)` appends to the same target
object and keeps its existing rectangle. Repeated calls intentionally retain
duplicates. New text/image geometry is transformed and clipped to the target;
old blocks retain their order. Each appended segment keeps its own flags and
image resources, including when it comes from another document.

Matrices must be `Matrix` instances with finite coefficients; invalid inputs or
an unreadable image needed to promote a Page-backed target leave it unchanged.
Unlike recorded targets, ordinary Page-backed targets retain their legacy image
visibility even where older creation flags were not honored. This method does
not repair that separate compatibility limitation.

::: pdfspine.TextPage
