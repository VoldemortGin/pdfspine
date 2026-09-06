"""Markdown export — the engine behind ``Page.to_markdown`` and
``Document.to_markdown`` (pdfspine-original extension, not part of the
PyMuPDF surface).

The source is ``get_text("dict", sort=True)``: block order is the engine's
reading order (column-aware), lines are already baseline-clustered, and each
span carries ``size`` and style ``flags``. This module only classifies and
renders; it never re-orders content.

Heading levels come from font-size clustering relative to the body size (the
size class carrying the most characters): every distinct size at or above
``body_size * heading_ratio`` becomes a level, largest first. Short all-bold
runs at body size become the next-deeper level. Ruled tables found by
``find_tables`` replace the text lines inside their bbox with a GFM table.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, Iterable, Sequence

_FLAG_ITALIC = 1 << 1
_FLAG_MONO = 1 << 3
_FLAG_BOLD = 1 << 4

_MIN_HEADING_CHARS = 3
_MAX_BOLD_HEADING_LINES = 2
_MAX_BOLD_HEADING_WORDS = 15
_MAX_WRAPPED_HEADING_WORDS = 20
_MAX_HEADING_WORDS = 30
_MIN_HEADING_ALNUM = 3
_MAX_LIST_DEPTH = 4
_LIST_INDENT = "    "
_MAX_TABLE_PAGE_FRACTION = 0.9
_MAX_CELL_CHARS = 500

_GLYPH_BULLET_RE = re.compile(r"^[•◦▪●○■□‣·]\s*(?=\S)")
_ASCII_BULLET_RE = re.compile(r"^[–—\-*]\s+(?=\S)")
_NUMBER_RE = re.compile(r"^(?:\((\d{1,3})\)|(\d{1,3})[.)])\s+(?=\S)")
_LABEL_RE = re.compile(
    r"^(\((?:[a-zA-Z]|[ivxIVX]{1,5})\)|[a-z][.)]|[ivx]{1,5}[.)])\s+(?=\S)"
)
_BULLET_ONLY_RE = re.compile(r"^[•◦▪●○■□‣·]$")
_BLOCK_START_RE = re.compile(r"^([#>+\-*]\s|\d{1,3}[.)]\s|`{3})")
_LEADER_RE = re.compile(r"(?:\.\s?){4,}\s*\S{0,6}$")
_SENTENCE_END = (".", ";", ",")
_HEADING_STOP = (".", ":", ";")


@dataclass(frozen=True)
class MarkdownOptions:
    """Rendering knobs shared by page- and document-level export."""

    tables: bool = True
    table_strategy: str = "lines"
    heading_levels: int = 3
    heading_ratio: float = 1.15
    bold_headings: bool = True
    emphasis: bool = True
    images: bool = False

    def __post_init__(self) -> None:
        if not 1 <= self.heading_levels <= 6:
            raise ValueError("heading_levels must be between 1 and 6")
        if self.heading_ratio <= 1.0:
            raise ValueError("heading_ratio must be > 1.0")


@dataclass(frozen=True)
class HeadingScale:
    """Font-size → heading-level mapping derived from one or more pages."""

    body_size: float
    size_levels: dict[float, int]
    bold_level: int | None


@dataclass(frozen=True)
class TableRegion:
    """A detected table: its bbox and the GFM rendering that replaces it."""

    bbox: tuple[float, float, float, float]
    markdown: str


@dataclass
class _Run:
    text: str
    bold: bool = False
    italic: bool = False
    mono: bool = False

    @property
    def style(self) -> tuple[bool, bool, bool]:
        return (self.bold, self.italic, self.mono)


@dataclass
class _Line:
    x0: float
    y0: float
    x1: float
    y1: float
    size: float
    runs: list[_Run]
    level: int | None = None

    @property
    def raw(self) -> str:
        return "".join(run.text for run in self.runs)

    @property
    def text(self) -> str:
        return _collapse(self.raw)

    @property
    def bold(self) -> bool:
        return _all_styled(self.runs, 0)

    @property
    def mono(self) -> bool:
        return _all_styled(self.runs, 2)

    @property
    def center(self) -> tuple[float, float]:
        return ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)


@dataclass
class _ListItem:
    marker: str
    depth: int
    runs: list[_Run] = field(default_factory=list)


def _collapse(text: str) -> str:
    return " ".join(text.split())


def _all_styled(runs: Sequence[_Run], index: int) -> bool:
    seen = False
    for run in runs:
        if not run.text.strip():
            continue
        if not run.style[index]:
            return False
        seen = True
    return seen


def size_class(value: float) -> float:
    """Rounds a font size to the nearest half point (the clustering key)."""
    return round(float(value) * 2.0) / 2.0


def _span_size(span: dict[str, Any]) -> float:
    size = float(span.get("size", 0.0) or 0.0)
    if size <= 0:
        size = float(span.get("rendered_size", 0.0) or 0.0)
    return size


def _iter_text_lines(data: dict[str, Any]) -> Iterable[dict[str, Any]]:
    for block in data.get("blocks", ()):
        if block.get("type") != 0:
            continue
        yield from block.get("lines", ())


def _line_from_dict(line: dict[str, Any]) -> _Line | None:
    spans = [s for s in line.get("spans", ()) if s.get("text") and s.get("bbox")]
    if not spans:
        return None
    spans.sort(key=lambda s: float(s["bbox"][0]))
    runs: list[_Run] = []
    chars: dict[float, int] = {}
    reach: float | None = None
    for span in spans:
        text = str(span["text"])
        x0, _y0, x1, _y1 = (float(v) for v in span["bbox"])
        size = _span_size(span)
        if (
            reach is not None
            and x0 - reach > max(1.0, 0.25 * size)
            and runs
            and not runs[-1].text.endswith(" ")
            and not text.startswith(" ")
        ):
            runs.append(_Run(" "))
        flags = int(span.get("flags", 0) or 0)
        runs.append(
            _Run(
                text,
                bold=bool(flags & _FLAG_BOLD),
                italic=bool(flags & _FLAG_ITALIC),
                mono=bool(flags & _FLAG_MONO),
            )
        )
        stripped = text.strip()
        if stripped:
            key = size_class(size)
            chars[key] = chars.get(key, 0) + len(stripped)
        reach = x1 if reach is None else max(reach, x1)
    if not chars:
        return None
    dominant = max(chars.items(), key=lambda kv: (kv[1], kv[0]))[0]
    x0, y0, x1, y1 = (float(v) for v in line["bbox"])
    return _Line(x0, y0, x1, y1, dominant, runs)


def compute_heading_scale(
    pages: Iterable[dict[str, Any]], options: MarkdownOptions
) -> HeadingScale:
    """Derives the heading scale from the ``dict`` data of one or more pages."""
    chars: dict[float, int] = {}
    for data in pages:
        for line in _iter_text_lines(data):
            for span in line.get("spans", ()):
                text = str(span.get("text", "")).strip()
                if not text:
                    continue
                key = size_class(_span_size(span))
                chars[key] = chars.get(key, 0) + len(text)
    if not chars:
        return HeadingScale(0.0, {}, 1 if options.bold_headings else None)
    body = max(chars.items(), key=lambda kv: (kv[1], -kv[0]))[0]
    threshold = body * options.heading_ratio
    candidates = sorted(
        (
            size
            for size, count in chars.items()
            if size >= threshold and count >= _MIN_HEADING_CHARS
        ),
        reverse=True,
    )
    levels = options.heading_levels
    size_levels = {size: min(i + 1, levels) for i, size in enumerate(candidates)}
    bold_level = min(len(candidates) + 1, levels) if options.bold_headings else None
    return HeadingScale(body, size_levels, bold_level)


def table_is_plausible(
    bbox: tuple[float, float, float, float],
    cells: Sequence[Sequence[object]],
    page_area: float,
) -> bool:
    """Filters ``find_tables`` output before it replaces page text.

    Rejects a grid that covers (almost) the whole page, one with fewer than
    two non-empty cells, and one holding a cell of running prose (more than
    ``_MAX_CELL_CHARS`` characters) — ruled figure frames and page borders
    rather than tables.
    """
    x0, y0, x1, y1 = bbox
    area = max(0.0, x1 - x0) * max(0.0, y1 - y0)
    if page_area > 0 and area >= _MAX_TABLE_PAGE_FRACTION * page_area:
        return False
    texts = [str(cell).strip() for row in cells for cell in row if cell is not None]
    texts = [text for text in texts if text]
    if len(texts) < 2:
        return False
    return all(len(text) <= _MAX_CELL_CHARS for text in texts)


def _heading_size(lines: Sequence[_Line], scale: HeadingScale) -> float | None:
    """The heading font size shared by every line, else ``None``."""
    sizes = {line.size for line in lines}
    if len(sizes) != 1:
        return None
    size = sizes.pop()
    return size if size in scale.size_levels else None


def _classify(lines: list[_Line], scale: HeadingScale) -> None:
    """Assigns ``level`` per line: by size class, then the bold-run rule.

    Dotted-leader lines (tables of contents) are never headings. The bold
    rule promotes the block's *leading* run of all-bold, body-size lines when
    it is short (at most two lines / fifteen words), does not end like a
    sentence, and is not a bullet item.
    """
    for line in lines:
        line.level = scale.size_levels.get(line.size)
        if line.level is not None and _LEADER_RE.search(line.text):
            line.level = None
    if scale.bold_level is None or not lines or lines[0].level is not None:
        return
    run: list[_Line] = []
    for line in lines:
        if line.level is not None or not line.bold or line.mono:
            break
        run.append(line)
    if not run or len(run) > _MAX_BOLD_HEADING_LINES:
        return
    if run[0].size < scale.body_size:
        return
    text = " ".join(line.text for line in run)
    if len(text.split()) > _MAX_BOLD_HEADING_WORDS or text.endswith(_SENTENCE_END):
        return
    if _GLYPH_BULLET_RE.match(text) or _ASCII_BULLET_RE.match(text):
        return
    if _LEADER_RE.search(text):
        return
    for line in run:
        line.level = scale.bold_level


def _demote_unlikely_headings(lines: list[_Line]) -> None:
    """Heading groups that read like paragraphs go back to body text.

    A run of same-level lines is demoted when it exceeds thirty words, when
    it spans several lines and ends with a period (a lead paragraph set in a
    larger face), or when it carries fewer than three alphanumeric characters
    (a stray page number at heading size).
    """
    index = 0
    while index < len(lines):
        level = lines[index].level
        end = index
        while end < len(lines) and lines[end].level == level:
            end += 1
        if level is not None:
            group = lines[index:end]
            text = " ".join(line.text for line in group)
            alnum = sum(ch.isalnum() for ch in text)
            if (
                len(text.split()) > _MAX_HEADING_WORDS
                or (len(group) > 1 and text.endswith("."))
                or alnum < _MIN_HEADING_ALNUM
            ):
                for line in group:
                    line.level = None
        index = end


def _merge_bullet_lines(lines: list[_Line]) -> list[_Line]:
    """Glues a bullet-only line onto the line that follows it."""
    merged: list[_Line] = []
    pending: _Line | None = None
    for line in lines:
        if pending is not None:
            line.runs = pending.runs + [_Run(" ")] + line.runs
            line.x0 = min(line.x0, pending.x0)
            pending = None
        elif _BULLET_ONLY_RE.match(line.text):
            pending = line
            continue
        merged.append(line)
    if pending is not None:
        merged.append(pending)
    return merged


def _emphasizable(text: str) -> bool:
    return len(text) > 1 and any(ch.isalnum() for ch in text)


def _render_runs(runs: Sequence[_Run], emphasis: bool) -> str:
    """Inline text: ``**bold**``, ``_italic_`` and ```code```.

    Adjacent runs of one style merge first, so a bold sentence spanning
    several spans (or lines) gets a single pair of markers; single characters
    and pure punctuation are left unmarked.
    """
    if not emphasis:
        return _collapse("".join(run.text for run in runs))
    merged: list[_Run] = []
    for run in runs:
        blank = not run.text.strip()
        if merged and (blank or merged[-1].style == run.style):
            merged[-1].text += run.text
        elif merged and not merged[-1].text.strip():
            merged[-1] = _Run(merged[-1].text + run.text, *run.style)
        else:
            merged.append(_Run(run.text, *run.style))
    out: list[str] = []
    for run in merged:
        text = _collapse(run.text)
        if not text:
            out.append(" ")
            continue
        if _emphasizable(text):
            if run.mono:
                text = f"`{text}`"
            else:
                if run.italic:
                    text = f"_{text}_"
                if run.bold:
                    text = f"**{text}**"
        lead = " " if run.text[:1].isspace() else ""
        tail = " " if run.text[-1:].isspace() else ""
        out.append(f"{lead}{text}{tail}")
    return _collapse("".join(out))


def _drop_prefix(runs: Sequence[_Run], count: int) -> list[_Run]:
    out: list[_Run] = []
    remaining = count
    for run in runs:
        if remaining >= len(run.text):
            remaining -= len(run.text)
            continue
        out.append(_Run(run.text[remaining:], *run.style))
        remaining = 0
    return out


def _join_mode(tail: str, head: str) -> str:
    """How a wrapped line continues the previous one.

    ``"dehyphenate"`` drops a soft hyphen (``con-`` + ``tinuous``); ``"glue"``
    keeps the hyphen of a compound (``180-million-`` + ``cubic``) but adds no
    space; ``"space"`` is the plain join.
    """
    tail = tail.rstrip()
    if not tail.endswith("-") or not head[:1].islower():
        return "space"
    last_word = tail.split()[-1] if tail.split() else tail
    return "glue" if "-" in last_word[:-1] else "dehyphenate"


def _append_line(runs: list[_Run], line_runs: Sequence[_Run]) -> None:
    """Appends a wrapped line's runs, mending soft hyphenation."""
    tail = next((run for run in reversed(runs) if run.text.strip()), None)
    head = next((run.text.lstrip() for run in line_runs if run.text.strip()), "")
    mode = "space" if tail is None else _join_mode(tail.text, head)
    if mode == "dehyphenate" and tail is not None:
        tail.text = tail.text.rstrip()[:-1]
    elif mode == "space" and runs:
        runs.append(_Run(" "))
    runs.extend(_Run(run.text, *run.style) for run in line_runs)


def _join_texts(texts: Sequence[str]) -> str:
    """Joins wrapped plain-text lines (headings) with the same hyphen rules."""
    joined = ""
    for text in texts:
        if not joined:
            joined = text
            continue
        mode = _join_mode(joined, text)
        if mode == "dehyphenate":
            joined = joined.rstrip()[:-1] + text
        elif mode == "glue":
            joined = joined.rstrip() + text
        else:
            joined = f"{joined} {text}"
    return _collapse(joined)


def _escape_block_start(text: str) -> str:
    return f"\\{text}" if _BLOCK_START_RE.match(text) else text


def _list_marker(raw: str) -> tuple[str, int] | None:
    """The Markdown marker for a list line and the raw prefix length to drop."""
    stripped = raw.lstrip()
    offset = len(raw) - len(stripped)
    match = _GLYPH_BULLET_RE.match(stripped) or _ASCII_BULLET_RE.match(stripped)
    if match:
        return "-", offset + match.end()
    match = _NUMBER_RE.match(stripped)
    if match:
        return f"{match.group(1) or match.group(2)}.", offset + match.end()
    match = _LABEL_RE.match(stripped)
    if match:
        return f"- {match.group(1)}", offset + match.end()
    return None


def _marker_kind(marker: str) -> str:
    return "ordered" if marker[:1].isdigit() else "bullet"


def _render_code(lines: Sequence[_Line]) -> str:
    left = min(line.x0 for line in lines)
    rows: list[str] = []
    for line in lines:
        unit = max(0.6 * line.size, 1.0)
        indent = int((line.x0 - left) / unit + 0.5)
        rows.append(" " * indent + line.text)
    return "```\n" + "\n".join(rows) + "\n```"


def _render_body(
    lines: Sequence[_Line], scale: HeadingScale, options: MarkdownOptions
) -> list[str]:
    """Paragraphs and lists for a run of body lines (one block)."""
    chunks: list[str] = []
    paragraph: list[_Run] = []
    items: list[_ListItem] = []
    base_x0 = 0.0
    indent_unit = max(1.5 * scale.body_size, 8.0)

    def flush_paragraph() -> None:
        if paragraph:
            text = _render_runs(paragraph, options.emphasis)
            if text:
                chunks.append(_escape_block_start(text))
            paragraph.clear()

    def flush_list() -> None:
        if items:
            rows = [
                f"{_LIST_INDENT * item.depth}{item.marker} "
                f"{_render_runs(item.runs, options.emphasis)}"
                for item in items
            ]
            chunks.append("\n".join(rows))
            items.clear()

    for line in lines:
        marker = _list_marker(line.raw)
        if marker is not None:
            flush_paragraph()
            if not items:
                base_x0 = line.x0
            depth = int((line.x0 - base_x0) / indent_unit + 0.5)
            depth = min(_MAX_LIST_DEPTH, max(depth, 0))
            items.append(
                _ListItem(marker[0], depth, _drop_prefix(line.runs, marker[1]))
            )
        elif items:
            _append_line(items[-1].runs, line.runs)
        else:
            _append_line(paragraph, line.runs)
    flush_paragraph()
    flush_list()
    return chunks


def render_lines(
    lines: list[_Line], scale: HeadingScale, options: MarkdownOptions
) -> list[str]:
    """Renders the lines of one block group into Markdown chunks."""
    lines = _merge_bullet_lines(lines)
    if not lines:
        return []
    _classify(lines, scale)
    _demote_unlikely_headings(lines)
    if all(line.mono for line in lines) and all(line.level is None for line in lines):
        return [_render_code(lines)]
    chunks: list[str] = []
    index = 0
    while index < len(lines):
        level = lines[index].level
        end = index
        while end < len(lines) and lines[end].level == level:
            end += 1
        group = lines[index:end]
        if level is None:
            chunks.extend(_render_body(group, scale, options))
        else:
            chunks.append(f"{'#' * level} {_join_texts([ln.text for ln in group])}")
        index = end
    return chunks


def _all_mono(lines: Sequence[_Line]) -> bool:
    return all(line.mono for line in lines)


def _joinable(
    pending: Sequence[_Line], lines: Sequence[_Line], scale: HeadingScale
) -> bool:
    """Whether a new block continues the pending group.

    Consecutive all-monospace blocks form one fenced code block; a block
    opening with a list marker continues a pending list of the same kind (so
    nesting depth is measured against the first item even when the engine
    split the items into blocks); and a wrapped heading — the next block at
    the same heading size, within 1.5 line heights, whose text has not ended
    — is joined into one heading.
    """
    if _all_mono(pending) and _all_mono(lines):
        return True
    head = _list_marker(lines[0].raw)
    if head is not None:
        tail = next(
            (m for m in (_list_marker(ln.raw) for ln in reversed(pending)) if m), None
        )
        return tail is not None and _marker_kind(tail[0]) == _marker_kind(head[0])
    size = _heading_size(pending, scale)
    if size is None or _heading_size(lines, scale) != size:
        return False
    last = pending[-1].text
    if not last or last[-1].isdigit() or last.endswith(_HEADING_STOP):
        return False
    if sum(ch.isalnum() for ch in last) < _MIN_HEADING_ALNUM:
        return False
    gap = lines[0].y0 - pending[-1].y1
    if not -0.5 * size <= gap <= 1.5 * size:
        return False
    if any(_LEADER_RE.search(line.text) for line in (*pending, *lines)):
        return False
    words = sum(len(line.text.split()) for line in (*pending, *lines))
    return words <= _MAX_WRAPPED_HEADING_WORDS


def _inside(
    point: tuple[float, float], bbox: tuple[float, float, float, float]
) -> bool:
    x, y = point
    return bbox[0] <= x <= bbox[2] and bbox[1] <= y <= bbox[3]


def render_page(
    data: dict[str, Any],
    tables: Sequence[TableRegion],
    scale: HeadingScale,
    options: MarkdownOptions,
    *,
    page_number: int = 0,
) -> str:
    """Renders one page's ``dict`` data (plus detected tables) as Markdown.

    Text lines whose center lies inside a table bbox are replaced by that
    table's GFM rendering, emitted where the first such line occurs in reading
    order; tables no line belongs to are appended in top-to-bottom order.
    Blocks are separated by blank lines; the result ends with a newline or is
    ``""`` for a page without content.
    """
    chunks: list[str] = []
    emitted = [False] * len(tables)
    image_index = 0
    pending: list[_Line] = []

    def flush() -> None:
        chunks.extend(render_lines(pending, scale, options))
        pending.clear()

    def emit_table(index: int) -> None:
        if not emitted[index]:
            emitted[index] = True
            chunks.append(tables[index].markdown.strip())

    def extend(lines: list[_Line]) -> None:
        if pending and lines and not _joinable(pending, lines, scale):
            flush()
        pending.extend(lines)

    for block in data.get("blocks", ()):
        block_type = block.get("type")
        if block_type == 1:
            image_index += 1
            if options.images:
                flush()
                ext = str(block.get("ext", "") or "bin")
                chunks.append(
                    f"![image](page-{page_number + 1}-image-{image_index}.{ext})"
                )
            continue
        if block_type != 0:
            continue
        group: list[_Line] = []
        for raw in block.get("lines", ()):
            line = _line_from_dict(raw)
            if line is None:
                continue
            table_index = next(
                (i for i, t in enumerate(tables) if _inside(line.center, t.bbox)), None
            )
            if table_index is None:
                group.append(line)
                continue
            extend(group)
            group = []
            flush()
            emit_table(table_index)
        extend(group)
    flush()

    for index in sorted(range(len(tables)), key=lambda i: tables[i].bbox[1]):
        emit_table(index)
    body = "\n\n".join(chunk for chunk in chunks if chunk)
    return f"{body}\n" if body else ""
