# Tables

`Page.find_tables(...)` returns a `TableFinder` (iterable; `.tables` is the list
of detected `Table`s).

## TableFinder

::: pdfspine.TableFinder

## Table

::: pdfspine.Table

## ImageTable

`Page.find_image_tables(...)` (a pdfspine extra for scanned / image-only pages)
returns a list of `ImageTable`s made of `ImageTableCell`s.

::: pdfspine.ImageTable

## ImageTableCell

::: pdfspine.ImageTableCell
