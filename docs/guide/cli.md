# Command-line interface

The `pdfspine` command-line tool ships with the package (console-script entry
point `pdfspine`, since 0.1.0) and uses only the standard-library `argparse`.
Page selectors are **1-based**, in the `1-3,5,8-` style (comma-separated pages
or ranges; either end of a range may be omitted). Errors are reported as a
single `pdfspine: <message>` line on stderr with a non-zero exit code.

## Usage

```bash
pdfspine <command> [options] <file.pdf>
pdfspine --version
```

## Subcommands

| Command | Purpose |
|---|---|
| `info` | Print document facts (page count, metadata, encryption, PDF version). |
| `text` | Extract text from a page range (`--format text` / `json` / `html` / `xhtml` / `xml` / `blocks` / `words` / `dict` / `rawdict` / `rawjson`). |
| `render` | Rasterize pages to PNG at a given `--dpi` (or `--zoom`). |
| `merge` | Concatenate several PDFs into one. |
| `split` | Split a PDF into per-page or per-`--ranges` files. |
| `pages` | Keep / reorder pages (`--select`) into a new file. |
| `images` | Extract embedded images from a document. |
| `toc` | Print the table of contents (bookmarks / outline). |

### Examples

```bash
# Document facts.
pdfspine info input.pdf

# Extract text from pages 1-3 to stdout.
pdfspine text input.pdf --pages 1-3

# Render every page to PNG at 150 DPI into ./out/.
pdfspine render input.pdf --dpi 150 -o out/

# Merge several PDFs.
pdfspine merge a.pdf b.pdf c.pdf -o merged.pdf

# Split into one file per page.
pdfspine split input.pdf -o parts/

# Keep only pages 3, 1, 2 (reorder) into a new file.
pdfspine pages input.pdf --select 3,1,2 -o reordered.pdf

# Dump embedded images.
pdfspine images input.pdf -o images/

# Print the table of contents.
pdfspine toc input.pdf
```

Each of these can also be scripted against the Python API — see
[Quickstart](quickstart.md) and [Editing & saving](editing.md).
