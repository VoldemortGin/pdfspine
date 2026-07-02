//! The block model — a small, layout-ready IR built from the pulldown-cmark
//! event stream (CommonMark + GFM tables / strikethrough / task lists).
//!
//! The builder is a recursive descent over the *balanced* `Start … End` event
//! stream: every container recursion consumes exactly its own `End`, so no
//! explicit tag matching is needed. Inline styling is tracked structurally
//! (nesting `Strong` inside `Emphasis` yields bold-italic runs); images split
//! the enclosing paragraph into `Paragraph / Image / Paragraph` blocks (images
//! render at block level only — an image inside a heading or table cell is
//! dropped).

use pulldown_cmark::{Alignment, Event, Options as CmarkOptions, Parser, Tag};

/// Inline style flags accumulated from the enclosing emphasis / code / link /
/// strikethrough spans.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) code: bool,
    pub(crate) strike: bool,
    pub(crate) link: bool,
}

/// One inline element of a paragraph / heading / table cell.
#[derive(Clone, Debug)]
pub(crate) enum Inline {
    /// A styled text run.
    Text { text: String, style: Style },
    /// A hard line break (`\` or two trailing spaces).
    HardBreak,
}

/// Horizontal alignment of one table column (from the GFM delimiter row).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum CellAlign {
    Left,
    Center,
    Right,
}

/// One list item: an optional GFM task-list checkbox plus its child blocks.
#[derive(Clone, Debug)]
pub(crate) struct ListItem {
    pub(crate) checkbox: Option<bool>,
    pub(crate) blocks: Vec<Block>,
}

/// A block-level element, in document order.
#[derive(Clone, Debug)]
pub(crate) enum Block {
    /// ATX / setext heading, `level` in `1..=6`.
    Heading { level: u8, inlines: Vec<Inline> },
    /// A paragraph of inline content.
    Paragraph(Vec<Inline>),
    /// A fenced / indented code block (verbatim text, newlines preserved).
    Code(String),
    /// An ordered or unordered list. `start` is the first ordinal (ordered).
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    /// A blockquote wrapping child blocks.
    Quote(Vec<Block>),
    /// A thematic break (`---`).
    Rule,
    /// A GFM table: header row + body rows of inline cells.
    Table {
        aligns: Vec<CellAlign>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// A block-level image. `id` indexes the prepared-image list (assigned by
    /// the image-resolution pass; `usize::MAX` until then).
    Image { src: String, id: usize },
}

/// Parses `markdown` into the block model with the GFM extensions enabled.
pub(crate) fn parse_blocks(markdown: &str) -> Vec<Block> {
    let opts = CmarkOptions::ENABLE_TABLES
        | CmarkOptions::ENABLE_STRIKETHROUGH
        | CmarkOptions::ENABLE_TASKLISTS;
    let mut b = Builder {
        iter: Parser::new_ext(markdown, opts).peekable(),
        pending_task: None,
    };
    b.blocks(None)
}

/// Whether `ev` belongs to inline (paragraph) content when seen at block level
/// — i.e. a tight list item's bare inline stream.
fn is_inline_event(ev: &Event) -> bool {
    matches!(
        ev,
        Event::Text(_)
            | Event::Code(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::InlineHtml(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::TaskListMarker(_)
            | Event::FootnoteReference(_)
            | Event::Start(
                Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            )
    )
}

struct Builder<'a> {
    iter: std::iter::Peekable<Parser<'a>>,
    /// The most recent `TaskListMarker` seen and not yet claimed by a list item.
    pending_task: Option<bool>,
}

impl<'a> Builder<'a> {
    /// Parses blocks until the enclosing container's `End` event (consumed) or
    /// end of input. `task` (list-item context) claims the first task marker
    /// seen inside the item.
    fn blocks(&mut self, mut task: Option<&mut Option<bool>>) -> Vec<Block> {
        let mut out: Vec<Block> = Vec::new();
        while let Some(ev) = self.iter.peek() {
            if matches!(ev, Event::End(_)) {
                // Balanced stream: any End at this level closes our container.
                self.iter.next();
                break;
            }
            if is_inline_event(ev) {
                out.extend(self.implicit_paragraph());
            } else {
                let ev = self.iter.next().unwrap_or(Event::Rule);
                self.block_event(ev, &mut out);
            }
            // A task marker surfaces from the first inline content of the item.
            if let Some(t) = task.as_deref_mut() {
                if t.is_none() {
                    *t = self.pending_task.take();
                }
            }
            self.pending_task = None;
        }
        out
    }

    /// Handles one consumed block-level event.
    fn block_event(&mut self, ev: Event<'a>, out: &mut Vec<Block>) {
        match ev {
            Event::Start(Tag::Paragraph) => out.extend(self.paragraph()),
            Event::Start(Tag::Heading { level, .. }) => {
                let mut inlines = Vec::new();
                self.inlines(Style::default(), &mut inlines, None);
                out.push(Block::Heading {
                    level: level as u8,
                    inlines,
                });
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let blocks = self.blocks(None);
                out.push(Block::Quote(blocks));
            }
            Event::Start(Tag::CodeBlock(_kind)) => {
                // The fenced info string (language) is ignored — no highlighting.
                out.push(Block::Code(self.code_block_text()));
            }
            Event::Start(Tag::List(start)) => {
                let ordered = start.is_some();
                let start = start.unwrap_or(1);
                let items = self.list_items();
                out.push(Block::List {
                    ordered,
                    start,
                    items,
                });
            }
            Event::Start(Tag::Table(aligns)) => {
                let aligns = aligns
                    .iter()
                    .map(|a| match a {
                        Alignment::Center => CellAlign::Center,
                        Alignment::Right => CellAlign::Right,
                        Alignment::None | Alignment::Left => CellAlign::Left,
                    })
                    .collect();
                let (head, rows) = self.table_rows();
                out.push(Block::Table { aligns, head, rows });
            }
            Event::Rule => out.push(Block::Rule),
            Event::Html(_) => {} // raw block HTML chunks arrive as bare events
            Event::Start(_) => {
                // Unhandled container (HtmlBlock, footnote definition, …):
                // parse its children and splice any usable blocks through (raw
                // HTML inside arrives as bare `Event::Html` and is dropped).
                out.extend(self.blocks(None));
            }
            _ => {}
        }
    }

    /// Parses an explicit paragraph (`Start(Paragraph)` consumed); images split
    /// it into multiple blocks.
    fn paragraph(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut cur = Vec::new();
        self.inlines(Style::default(), &mut cur, Some(&mut blocks));
        flush_paragraph(&mut cur, &mut blocks);
        blocks
    }

    /// Parses a *tight* list item's bare inline stream as a paragraph, stopping
    /// (without consuming) at the first block-level boundary.
    fn implicit_paragraph(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut cur = Vec::new();
        while let Some(ev) = self.iter.peek() {
            if !is_inline_event(ev) {
                break;
            }
            let ev = self.iter.next().unwrap_or(Event::SoftBreak);
            self.inline_event(ev, Style::default(), &mut cur, Some(&mut blocks));
        }
        flush_paragraph(&mut cur, &mut blocks);
        blocks
    }

    /// Parses inline events until the enclosing container's `End` (consumed).
    /// `sink` (paragraph context) receives image blocks; `None` drops them.
    fn inlines(&mut self, style: Style, out: &mut Vec<Inline>, mut sink: Option<&mut Vec<Block>>) {
        while let Some(ev) = self.iter.next() {
            if matches!(ev, Event::End(_)) {
                break;
            }
            self.inline_event(ev, style, out, sink.as_deref_mut());
        }
    }

    /// Handles one consumed inline event under the accumulated `style`.
    fn inline_event(
        &mut self,
        ev: Event<'a>,
        style: Style,
        out: &mut Vec<Inline>,
        sink: Option<&mut Vec<Block>>,
    ) {
        match ev {
            Event::Text(s) => push_text(out, &s, style),
            Event::Code(s) | Event::InlineMath(s) | Event::DisplayMath(s) => {
                let mut st = style;
                st.code = true;
                push_text(out, &s, st);
            }
            Event::SoftBreak => push_text(out, " ", style),
            Event::HardBreak => out.push(Inline::HardBreak),
            Event::TaskListMarker(b) => self.pending_task = Some(b),
            Event::InlineHtml(_) | Event::Html(_) | Event::FootnoteReference(_) => {}
            Event::Start(Tag::Emphasis) => {
                let mut st = style;
                st.italic = true;
                self.inlines(st, out, sink);
            }
            Event::Start(Tag::Strong) => {
                let mut st = style;
                st.bold = true;
                self.inlines(st, out, sink);
            }
            Event::Start(Tag::Strikethrough) => {
                let mut st = style;
                st.strike = true;
                self.inlines(st, out, sink);
            }
            Event::Start(Tag::Link { .. }) => {
                let mut st = style;
                st.link = true;
                self.inlines(st, out, sink);
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.skip_container(); // alt text is not rendered
                if let Some(sink) = sink {
                    flush_paragraph(out, sink);
                    sink.push(Block::Image {
                        src: dest_url.to_string(),
                        id: usize::MAX,
                    });
                }
            }
            Event::Start(_) => self.skip_container(), // stray nested block
            Event::End(_) | Event::Rule => {}
        }
    }

    /// Collects the verbatim text of a code block until its `End`.
    fn code_block_text(&mut self) -> String {
        let mut text = String::new();
        for ev in self.iter.by_ref() {
            match ev {
                Event::End(_) => break,
                Event::Text(s) => text.push_str(&s),
                _ => {}
            }
        }
        // Fenced blocks carry a trailing newline; drop exactly one.
        if text.ends_with('\n') {
            text.pop();
        }
        text
    }

    /// Parses the items of a list until its `End`.
    fn list_items(&mut self) -> Vec<ListItem> {
        let mut items = Vec::new();
        loop {
            match self.iter.peek() {
                Some(Event::Start(Tag::Item)) => {
                    self.iter.next();
                    let mut checkbox = None;
                    let blocks = self.blocks(Some(&mut checkbox));
                    items.push(ListItem { checkbox, blocks });
                }
                Some(Event::End(_)) => {
                    self.iter.next();
                    break;
                }
                Some(_) => {
                    self.iter.next(); // defensive: skip unexpected events
                }
                None => break,
            }
        }
        items
    }

    /// Parses `TableHead` + `TableRow`s until the table's `End`.
    fn table_rows(&mut self) -> (Vec<Vec<Inline>>, Vec<Vec<Vec<Inline>>>) {
        let mut head = Vec::new();
        let mut rows = Vec::new();
        loop {
            match self.iter.next() {
                Some(Event::Start(Tag::TableHead)) => head = self.row_cells(),
                Some(Event::Start(Tag::TableRow)) => rows.push(self.row_cells()),
                Some(Event::End(_)) | None => break,
                Some(_) => {}
            }
        }
        (head, rows)
    }

    /// Parses the cells of one header / body row until the row's `End`.
    fn row_cells(&mut self) -> Vec<Vec<Inline>> {
        let mut cells = Vec::new();
        loop {
            match self.iter.peek() {
                Some(Event::Start(Tag::TableCell)) => {
                    self.iter.next();
                    let mut inlines = Vec::new();
                    self.inlines(Style::default(), &mut inlines, None);
                    cells.push(inlines);
                }
                Some(Event::End(_)) => {
                    self.iter.next();
                    break;
                }
                Some(_) => {
                    self.iter.next();
                }
                None => break,
            }
        }
        cells
    }

    /// Consumes a balanced container (its `Start` already consumed) entirely.
    fn skip_container(&mut self) {
        let mut depth = 1u32;
        for ev in self.iter.by_ref() {
            match ev {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Appends a text run, merging into the previous run when the style matches.
fn push_text(out: &mut Vec<Inline>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text {
        text: prev,
        style: ps,
    }) = out.last_mut()
    {
        if *ps == style {
            prev.push_str(text);
            return;
        }
    }
    out.push(Inline::Text {
        text: text.to_string(),
        style,
    });
}

/// Flushes accumulated inline content into `sink` as a paragraph (if any of it
/// is non-whitespace).
fn flush_paragraph(cur: &mut Vec<Inline>, sink: &mut Vec<Block>) {
    let has_content = cur.iter().any(|i| match i {
        Inline::Text { text, .. } => !text.trim().is_empty(),
        Inline::HardBreak => true,
    });
    if has_content {
        sink.push(Block::Paragraph(std::mem::take(cur)));
    } else {
        cur.clear();
    }
}
