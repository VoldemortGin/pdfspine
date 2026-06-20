# Exceptions

Every pdfspine error derives from `PdfError`, so `except pdfspine.PdfError:`
catches them all. The concrete subclasses map to PyMuPDF's failure modes.

## PdfError

::: pdfspine.PdfError

## PdfSyntaxError

::: pdfspine.PdfSyntaxError

## PdfPasswordError

::: pdfspine.PdfPasswordError

## PdfUnsupportedError

::: pdfspine.PdfUnsupportedError

## PdfDecodeError

::: pdfspine.PdfDecodeError

## PdfLimitError

::: pdfspine.PdfLimitError

## PdfRedactionError

::: pdfspine.PdfRedactionError
