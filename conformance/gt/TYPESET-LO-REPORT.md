# Typeset ↔ LibreOffice advisory report (TS-7)

*Generated 2026-09-05T21:09:44-0700 · soffice: `/opt/homebrew/bin/soffice` · dpi 100.0 · advisory band **0.80–0.90** (local-only — LibreOffice is NOT a CI dependency).*

The same content is authored as minimal OOXML (converted by LibreOffice)
and as a pdf-typeset fixture; both PDFs are rasterized by pdfspine's own
renderer and compared with pure-Python SSIM. Scores inside or above the
band are expected; `below-band` means a layout regression to investigate.

| pair | fixture | LO oracle | SSIM | status |
|------|---------|-----------|------|--------|
| docx | `fixtures/typeset/typeset-lo-doc.pdf` | `sample-doc.pdf` | 0.9815 | above-band |
| pptx | `fixtures/typeset/typeset-lo-slide.pdf` | `sample-slide.pdf` | 0.9777 | above-band |

Regenerate: `python conformance/gt/typeset_lo_oracle.py --report conformance/gt/TYPESET-LO-REPORT.md`
