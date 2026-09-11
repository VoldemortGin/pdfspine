# Typeset ↔ LibreOffice advisory report (TS-7)

*Generated 2026-09-10T19:34:26-0700 · soffice: `/Applications/LibreOffice.app/Contents/MacOS/soffice` · dpi 100.0 · advisory band **0.80–0.90** (local-only — LibreOffice is NOT a CI dependency).*

The same content is authored as minimal OOXML (converted by LibreOffice)
and as a pdf-typeset fixture; both PDFs are rasterized by pdfspine's own
renderer and compared with pure-Python SSIM. Scores inside or above the
band are expected; `below-band` means a layout regression to investigate.

| pair | fixture | LO oracle | SSIM | status |
|------|---------|-----------|------|--------|
| docx | `fixtures/typeset/typeset-lo-doc.pdf` | `sample-doc.pdf` | 0.9822 | above-band |
| pptx | `fixtures/typeset/typeset-lo-slide.pdf` | `sample-slide.pdf` | 0.9780 | above-band |
| docx-shading | `fixtures/typeset/typeset-lo-shading.pdf` | `sample-shading.pdf` | 0.9868 | above-band |

Regenerate: `python conformance/gt/typeset_lo_oracle.py --report conformance/gt/TYPESET-LO-REPORT.md`

Paragraph shading increment: LibreOffice 26.8.0.3, same input content and fonts.
The new colored DOCX scored 0.9514 against the old unshaded engine PDF; adding
paragraph backgrounds raises it to 0.9868. Existing DOCX/PPTX pairs retain the
newly measured 0.9822/0.9780 baseline, and all four existing engine fixture PDFs
remain byte-identical. The previous 2026-09-05 report's 0.9815/0.9777 values are
historical measurements, not the current before/after baseline.

This increment covers solid paragraph fill only. Isolated paragraph spacing is
unpainted; adjacent equal fills with equal horizontal indents join across the
intervening spacing, stopping at page boundaries. It does not add paragraph
borders, pattern shading, superscript/subscript, character spacing, or lineGap
placement changes. LibreOffice remains a local-only advisory oracle.
