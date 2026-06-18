# Command-line interface

!!! warning "Coming soon — not yet implemented"
    The `pdfspine` command-line tool is **planned but not yet shipped**. There
    is no console-script entry point in the current build, so the commands below
    describe the *intended* CLI and are not yet runnable. This page will be
    updated to match the implementation when it lands. For now, use the Python
    API ([Quickstart](quickstart.md)).

## Planned usage

```bash
pdfspine <command> [options] <file.pdf>
```

## Planned subcommands

| Command | Purpose |
|---|---|
| `info` | Print document facts (page count, metadata, encryption, PDF version). |
| `text` | Extract text from a page range (plain / json / html variants). |
| `render` | Rasterize pages to PNG at a given DPI. |
| `merge` | Concatenate several PDFs into one. |
| `split` | Split a PDF into per-page or per-range files. |
| `pages` | Select / reorder / delete pages into a new file. |
| `images` | Extract embedded images from a document. |
| `toc` | Print or set the table of contents. |

### Illustrative examples (planned)

```bash
# Document facts.
pdfspine info input.pdf

# Extract text from pages 1-3 to stdout.
pdfspine text --pages 1-3 input.pdf

# Render every page to PNG at 150 DPI into ./out/.
pdfspine render --dpi 150 --out out/ input.pdf

# Merge several PDFs.
pdfspine merge a.pdf b.pdf c.pdf --out merged.pdf

# Split into one file per page.
pdfspine split input.pdf --out parts/

# Keep only pages 3, 1, 2 (reorder) into a new file.
pdfspine pages --select 3,1,2 input.pdf --out reordered.pdf

# Dump embedded images.
pdfspine images input.pdf --out images/

# Print the table of contents.
pdfspine toc input.pdf
```

Until the CLI ships, each of these can be scripted against the Python API — see
[Quickstart](quickstart.md) and [Editing & saving](editing.md).
