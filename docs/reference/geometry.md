# Geometry

The geometry value types mirror PyMuPDF 1.24.x arithmetic **exactly**. They
behave as sequences (`r[0]`, `tuple(r)`, unpacking), so code that reads
`page.rect` works unchanged. The coordinate space is top-left origin, y-down;
distances are in points (1/72 inch) unless a unit is given.

## Point

::: pdfspine.Point

## Rect

::: pdfspine.Rect

## IRect

::: pdfspine.IRect

## Matrix

::: pdfspine.Matrix

## Quad

::: pdfspine.Quad
