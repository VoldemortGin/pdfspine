"""``oxide-pdf`` — a command-line front-end for :mod:`oxide_pdf`.

A small, dependency-free (stdlib ``argparse`` only) CLI that mirrors the spirit
of PyMuPDF's ``python -m fitz`` tool. Subcommands:

* ``info``    — page count, metadata, encryption status, format, file size.
* ``text``    — extract text in any ``get_text`` format (``text``/``json``/…).
* ``render``  — rasterize pages to PNG (``page-0001.png``, …).
* ``merge``   — concatenate several PDFs into one.
* ``split``   — explode a PDF into one file per page (or per ``--ranges``).
* ``pages``   — keep / reorder a subset of pages via ``select``.
* ``images``  — extract embedded image XObjects.
* ``toc``     — print the bookmarks / outline.

Page selectors use **1-based** page numbers (matching PyMuPDF's CLI), in the
``1-3,5,8-`` style: comma-separated single pages or ``start-end`` ranges, where
either end may be omitted (``-3`` = pages 1..3, ``8-`` = page 8 to the last).
They are converted to 0-based indices internally.

Errors (file-not-found, malformed PDF, out-of-range pages, unsupported features)
are reported as a single ``oxide-pdf: <message>`` line on stderr with a non-zero
exit code — never a raw traceback.
"""

from __future__ import annotations

import argparse
import sys
from typing import Sequence

import oxide_pdf
from oxide_pdf import PdfError


class CLIError(Exception):
    """A user-facing error: printed as a clean message, no traceback."""


# --------------------------------------------------------------------------
# page-range parsing
# --------------------------------------------------------------------------


def parse_page_range(spec: str | None, page_count: int) -> list[int]:
    """Parses a 1-based ``1-3,5,8-`` page selector into sorted 0-based indices.

    ``None`` / empty selects every page. Each comma-separated token is either a
    single page ``N`` or a range ``A-B`` (either end omittable: ``-B`` ⇒ ``1-B``,
    ``A-`` ⇒ ``A``..last). Pages outside ``1..page_count`` raise :class:`CLIError`.
    """
    if spec is None or not spec.strip():
        return list(range(page_count))

    pages: list[int] = []
    for raw in spec.split(","):
        token = raw.strip()
        if not token:
            continue
        if "-" in token:
            lo_s, _, hi_s = token.partition("-")
            lo_s, hi_s = lo_s.strip(), hi_s.strip()
            try:
                lo = int(lo_s) if lo_s else 1
                hi = int(hi_s) if hi_s else page_count
            except ValueError:
                raise CLIError(f"invalid page range: {token!r}")
            if lo > hi:
                raise CLIError(f"invalid page range (start > end): {token!r}")
            for p in range(lo, hi + 1):
                pages.append(p)
        else:
            try:
                pages.append(int(token))
            except ValueError:
                raise CLIError(f"invalid page number: {token!r}")

    for p in pages:
        if p < 1 or p > page_count:
            raise CLIError(
                f"page {p} out of range (document has {page_count} page(s))"
            )

    # de-duplicate while preserving sorted order, then convert to 0-based.
    return [p - 1 for p in sorted(set(pages))]


def _open(filename: str) -> "oxide_pdf.Document":
    """Opens ``filename`` as a :class:`Document`, mapping failures to CLIError."""
    try:
        return oxide_pdf.open(filename)
    except FileNotFoundError:
        raise CLIError(f"file not found: {filename}")
    except OSError as exc:
        raise CLIError(f"cannot read {filename}: {exc}")
    except PdfError as exc:
        raise CLIError(f"cannot open {filename}: {exc}")


# --------------------------------------------------------------------------
# subcommands
# --------------------------------------------------------------------------


def _cmd_info(args: argparse.Namespace) -> int:
    import os

    doc = _open(args.file)
    md = doc.metadata
    try:
        size = os.path.getsize(args.file)
    except OSError:
        size = -1

    print(f"file:        {args.file}")
    print(f"file size:   {size} bytes")
    print(f"format:      {md.get('format', '') or 'unknown'}")
    print(f"is pdf:      {doc.is_pdf}")
    print(f"encrypted:   {doc.is_encrypted}")
    print(f"needs pass:  {doc.needs_pass}")
    print(f"page count:  {doc.page_count}")
    for key in ("title", "author", "subject", "keywords", "creator", "producer"):
        val = md.get(key, "")
        if val:
            print(f"{key + ':':12} {val}")
    return 0


def _cmd_text(args: argparse.Namespace) -> int:
    doc = _open(args.file)
    indices = parse_page_range(args.pages, doc.page_count)

    chunks: list[str] = []
    for i in indices:
        page = doc.load_page(i)
        try:
            value = page.get_text(args.format)
        except PdfError as exc:
            raise CLIError(f"text extraction failed on page {i + 1}: {exc}")
        chunks.append(_stringify(value))

    text = "\n".join(chunks) if len(chunks) > 1 else (chunks[0] if chunks else "")

    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(text)
    else:
        sys.stdout.write(text)
        if text and not text.endswith("\n"):
            sys.stdout.write("\n")
    return 0


def _stringify(value) -> str:
    """Renders a ``get_text`` result (str / list / dict) to text for the CLI."""
    if isinstance(value, str):
        return value
    import json

    return json.dumps(value, ensure_ascii=False, default=str)


def _cmd_render(args: argparse.Namespace) -> int:
    import os

    doc = _open(args.file)
    indices = parse_page_range(args.pages, doc.page_count)

    outdir = args.output or "."
    os.makedirs(outdir, exist_ok=True)

    matrix = None
    dpi = None
    if args.zoom is not None:
        matrix = oxide_pdf.Matrix(args.zoom, args.zoom)
    elif args.dpi is not None:
        dpi = args.dpi

    written = 0
    for i in indices:
        page = doc.load_page(i)
        try:
            pix = page.get_pixmap(matrix=matrix, dpi=dpi)
        except PdfError as exc:
            raise CLIError(f"render failed on page {i + 1}: {exc}")
        out = os.path.join(outdir, f"page-{i + 1:04d}.png")
        pix.save(out)
        written += 1
    print(f"rendered {written} page(s) to {outdir}")
    return 0


def _cmd_merge(args: argparse.Namespace) -> int:
    out = oxide_pdf.open()
    for src_path in args.files:
        src = _open(src_path)
        try:
            out.insert_pdf(src)
        except PdfError as exc:
            raise CLIError(f"cannot merge {src_path}: {exc}")
    try:
        out.save(args.output)
    except PdfError as exc:
        raise CLIError(f"cannot write {args.output}: {exc}")
    print(f"merged {len(args.files)} file(s) → {args.output} ({out.page_count} page(s))")
    return 0


def _cmd_split(args: argparse.Namespace) -> int:
    import os

    doc = _open(args.file)
    outdir = args.output or "."
    os.makedirs(outdir, exist_ok=True)

    if args.ranges:
        groups = [
            parse_page_range(token, doc.page_count)
            for token in args.ranges.split(",")
        ]
    else:
        groups = [[i] for i in range(doc.page_count)]

    written = 0
    for n, indices in enumerate(groups, start=1):
        if not indices:
            continue
        part = oxide_pdf.open()
        try:
            for i in indices:
                part.insert_pdf(doc, from_page=i, to_page=i)
            out = os.path.join(outdir, f"part-{n:04d}.pdf")
            part.save(out)
        except PdfError as exc:
            raise CLIError(f"split failed: {exc}")
        written += 1
    print(f"split into {written} file(s) in {outdir}")
    return 0


def _cmd_pages(args: argparse.Namespace) -> int:
    doc = _open(args.file)
    # --select is an ordered, possibly-repeating list (reorder/duplicate); keep
    # the user's order, so parse it ourselves rather than via parse_page_range.
    selected: list[int] = []
    for raw in args.select.split(","):
        token = raw.strip()
        if not token:
            continue
        try:
            p = int(token)
        except ValueError:
            raise CLIError(f"invalid page number in --select: {token!r}")
        if p < 1 or p > doc.page_count:
            raise CLIError(
                f"page {p} out of range (document has {doc.page_count} page(s))"
            )
        selected.append(p - 1)
    if not selected:
        raise CLIError("--select must list at least one page")

    try:
        doc.select(selected)
        doc.save(args.output)
    except PdfError as exc:
        raise CLIError(f"cannot write {args.output}: {exc}")
    print(f"selected {len(selected)} page(s) → {args.output}")
    return 0


def _cmd_images(args: argparse.Namespace) -> int:
    import os

    doc = _open(args.file)
    outdir = args.output or "."
    os.makedirs(outdir, exist_ok=True)

    seen: set[int] = set()
    written = 0
    for pno in range(doc.page_count):
        page = doc.load_page(pno)
        for img in page.get_images(full=True):
            xref = img[0]
            if xref in seen:
                continue
            seen.add(xref)
            try:
                info = doc.extract_image(xref)
            except PdfError as exc:
                raise CLIError(f"cannot extract image xref {xref}: {exc}")
            ext = info.get("ext", "bin") or "bin"
            out = os.path.join(outdir, f"image-{xref:04d}.{ext}")
            with open(out, "wb") as fh:
                fh.write(info["image"])
            written += 1
    print(f"extracted {written} image(s) to {outdir}")
    return 0


def _cmd_toc(args: argparse.Namespace) -> int:
    doc = _open(args.file)
    toc = doc.get_toc()
    if not toc:
        print("(no outline)")
        return 0
    for level, title, page in toc:
        indent = "  " * (max(level, 1) - 1)
        print(f"{indent}{title}  ..... p.{page}")
    return 0


# --------------------------------------------------------------------------
# argument parser
# --------------------------------------------------------------------------


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="oxide-pdf",
        description="A pure-Rust PyMuPDF-compatible PDF toolkit.",
    )
    parser.add_argument(
        "--version",
        action="store_true",
        help="show the oxide_pdf version and exit",
    )
    sub = parser.add_subparsers(dest="command", metavar="<command>")

    _RANGE_HELP = (
        "1-based page selector, e.g. '1-3,5,8-' "
        "(comma-separated pages/ranges; open-ended ranges allowed)"
    )

    p_info = sub.add_parser("info", help="show document facts")
    p_info.add_argument("file", help="input PDF")
    p_info.set_defaults(func=_cmd_info)

    p_text = sub.add_parser("text", help="extract text")
    p_text.add_argument("file", help="input PDF")
    p_text.add_argument("--pages", help=_RANGE_HELP)
    p_text.add_argument(
        "--format",
        default="text",
        choices=["text", "json", "html", "xhtml", "xml", "blocks", "words", "dict", "rawdict", "rawjson"],
        help="text output format (default: text)",
    )
    p_text.add_argument("-o", "--output", help="write to this file instead of stdout")
    p_text.set_defaults(func=_cmd_text)

    p_render = sub.add_parser("render", help="render pages to PNG")
    p_render.add_argument("file", help="input PDF")
    p_render.add_argument("--pages", help=_RANGE_HELP)
    g = p_render.add_mutually_exclusive_group()
    g.add_argument("--dpi", type=int, help="render resolution in DPI")
    g.add_argument("--zoom", type=float, help="render zoom factor (1.0 = 72 DPI)")
    p_render.add_argument("-o", "--output", help="output directory (default: .)")
    p_render.set_defaults(func=_cmd_render)

    p_merge = sub.add_parser("merge", help="concatenate PDFs")
    p_merge.add_argument("files", nargs="+", help="input PDFs (in order)")
    p_merge.add_argument("-o", "--output", required=True, help="output PDF")
    p_merge.set_defaults(func=_cmd_merge)

    p_split = sub.add_parser("split", help="split into one PDF per page / range")
    p_split.add_argument("file", help="input PDF")
    p_split.add_argument(
        "--ranges",
        help="comma-separated 1-based ranges, e.g. '1-3,4-6' (default: one file per page)",
    )
    p_split.add_argument("-o", "--output", help="output directory (default: .)")
    p_split.set_defaults(func=_cmd_split)

    p_pages = sub.add_parser("pages", help="subset / reorder pages")
    p_pages.add_argument("file", help="input PDF")
    p_pages.add_argument(
        "--select",
        required=True,
        help="1-based pages to keep, in order, e.g. '1,3,5' (may reorder/duplicate)",
    )
    p_pages.add_argument("-o", "--output", required=True, help="output PDF")
    p_pages.set_defaults(func=_cmd_pages)

    p_images = sub.add_parser("images", help="extract embedded images")
    p_images.add_argument("file", help="input PDF")
    p_images.add_argument("-o", "--output", help="output directory (default: .)")
    p_images.set_defaults(func=_cmd_images)

    p_toc = sub.add_parser("toc", help="print the bookmarks / outline")
    p_toc.add_argument("file", help="input PDF")
    p_toc.set_defaults(func=_cmd_toc)

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if getattr(args, "version", False):
        print(f"oxide-pdf {oxide_pdf.__version__}")
        return 0

    if not getattr(args, "command", None):
        parser.print_help(sys.stderr)
        return 2

    try:
        return args.func(args)
    except CLIError as exc:
        print(f"oxide-pdf: {exc}", file=sys.stderr)
        return 1
    except PdfError as exc:
        print(f"oxide-pdf: {exc}", file=sys.stderr)
        return 1
    except (FileNotFoundError, OSError) as exc:
        print(f"oxide-pdf: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
