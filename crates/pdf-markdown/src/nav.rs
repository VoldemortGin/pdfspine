//! Navigation metadata: heading titles, anchor slugs and outline levels.
//!
//! Headings are collected in **document order** (descending into blockquotes
//! and list items exactly as the layouter does), so the *k*-th heading here is
//! the *k*-th heading the layouter positions. Each heading gets a GitHub-style
//! anchor slug (lower-cased; letters, digits, `_` and `-` kept; spaces → `-`;
//! everything else dropped; duplicates suffixed `-1`, `-2`, …) unless it
//! carries an explicit `{#id}` attribute. `[text](#anchor)` links resolve
//! against those slugs (percent-decoded, then also re-slugified, so
//! `#My Heading` and `#my-heading` both hit `## My Heading`).

use crate::model::{Block, Inline};

/// One heading of the document, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadingMeta {
    /// Markdown level, `1..=6`.
    pub(crate) level: u8,
    /// Plain-text title (inline styling flattened, whitespace collapsed).
    pub(crate) title: String,
    /// The anchor slug / explicit id.
    pub(crate) slug: String,
}

/// Collects every heading in document order with unique slugs.
pub(crate) fn collect_headings(blocks: &[Block]) -> Vec<HeadingMeta> {
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

fn walk(blocks: &[Block], out: &mut Vec<HeadingMeta>) {
    for block in blocks {
        match block {
            Block::Heading { level, inlines, id } => {
                let title = plain_text(inlines);
                let slug = match id {
                    Some(id) => id.clone(),
                    None => unique_slug(&slugify(&title), out),
                };
                out.push(HeadingMeta {
                    level: *level,
                    title,
                    slug,
                });
            }
            Block::Quote(children) => walk(children, out),
            Block::List { items, .. } => {
                for item in items {
                    walk(&item.blocks, out);
                }
            }
            _ => {}
        }
    }
}

/// The flattened, whitespace-collapsed text of an inline sequence.
pub(crate) fn plain_text(inlines: &[Inline]) -> String {
    let mut raw = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => raw.push_str(text),
            Inline::HardBreak => raw.push(' '),
        }
    }
    let mut out = String::with_capacity(raw.len());
    for word in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// GitHub-style slug of a heading title (no de-duplication).
pub(crate) fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else if ch == ' ' {
            out.push('-');
        }
    }
    out
}

/// `slug`, suffixed `-1`, `-2`, … until it collides with no earlier heading.
fn unique_slug(slug: &str, earlier: &[HeadingMeta]) -> String {
    let taken = |s: &str| earlier.iter().any(|h| h.slug == s);
    if !taken(slug) {
        return slug.to_string();
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{slug}-{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Resolves the fragment of a `#anchor` link (without the `#`) to a heading
/// index: exact slug / explicit-id match on the percent-decoded fragment
/// first, then on its re-slugified form. `None` when nothing matches.
pub(crate) fn resolve_anchor(fragment: &str, headings: &[HeadingMeta]) -> Option<usize> {
    let decoded = percent_decode(fragment);
    if decoded.is_empty() {
        return None;
    }
    let find = |s: &str| headings.iter().position(|h| h.slug == s);
    find(&decoded).or_else(|| {
        let slug = slugify(&decoded);
        if slug.is_empty() {
            None
        } else {
            find(&slug)
        }
    })
}

/// Outline nesting levels for [`pdf_edit::set_toc`] (which rejects jumps).
/// Each heading nests under its nearest preceding *shallower* heading, so the
/// tree keeps every relative depth while never jumping more than one level:
/// `#` → `###` becomes 1 → 2, and `## → #### → ###` becomes 1 → 2 → 2.
pub(crate) fn outline_levels(headings: &[HeadingMeta]) -> Vec<i32> {
    let mut out = Vec::with_capacity(headings.len());
    // Markdown levels of the current ancestor chain (strictly increasing).
    let mut chain: Vec<u8> = Vec::new();
    for h in headings {
        while chain.last().is_some_and(|&top| top >= h.level) {
            chain.pop();
        }
        chain.push(h.level);
        out.push(chain.len() as i32);
    }
    out
}

/// Decodes `%XX` escapes as UTF-8 (lossy); malformed escapes are kept verbatim.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Byte-sliced (never `&s[..]`): a multi-byte char after `%` must
            // not panic on a non-boundary slice.
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(level: u8, title: &str, slug: &str) -> HeadingMeta {
        HeadingMeta {
            level,
            title: title.to_string(),
            slug: slug.to_string(),
        }
    }

    #[test]
    fn slugify_is_github_style() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("A  B"), "a--b");
        assert_eq!(slugify("snake_case & dash-ed"), "snake_case--dash-ed");
        assert_eq!(slugify("中文 标题"), "中文-标题");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn collect_headings_dedups_and_descends() {
        let parsed = crate::model::parse_blocks(
            "# Intro\n\n> ## Intro\n\n- ### Intro {#custom}\n- ### Intro\n",
        );
        let hs = collect_headings(&parsed.blocks);
        let slugs: Vec<&str> = hs.iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(slugs, ["intro", "intro-1", "custom", "intro-2"]);
        assert_eq!(hs[2].title, "Intro");
        let levels: Vec<u8> = hs.iter().map(|h| h.level).collect();
        assert_eq!(levels, [1, 2, 3, 3]);
    }

    #[test]
    fn plain_text_flattens_and_collapses() {
        let parsed = crate::model::parse_blocks("**Bold**  and\\\n`code`\n=====\n");
        let hs = collect_headings(&parsed.blocks);
        assert_eq!(hs[0].title, "Bold and code");
    }

    #[test]
    fn resolve_anchor_matches_decoded_and_reslugified() {
        let hs = vec![meta(1, "My Heading", "my-heading"), meta(2, "中文", "中文")];
        assert_eq!(resolve_anchor("my-heading", &hs), Some(0));
        assert_eq!(resolve_anchor("My%20Heading", &hs), Some(0));
        assert_eq!(resolve_anchor("My Heading", &hs), Some(0));
        assert_eq!(resolve_anchor("%E4%B8%AD%E6%96%87", &hs), Some(1));
        assert_eq!(resolve_anchor("missing", &hs), None);
        assert_eq!(resolve_anchor("", &hs), None);
        assert_eq!(resolve_anchor("%", &hs), None);
        assert_eq!(resolve_anchor("%zz", &hs), None);
        assert_eq!(resolve_anchor("%中文", &hs), Some(1)); // kept verbatim, re-slugified
        assert_eq!(resolve_anchor("中文", &hs), Some(1));
    }

    #[test]
    fn outline_levels_never_jump() {
        let hs = vec![
            meta(2, "a", "a"),
            meta(6, "b", "b"),
            meta(3, "c", "c"),
            meta(1, "d", "d"),
            meta(4, "e", "e"),
        ];
        assert_eq!(outline_levels(&hs), [1, 2, 2, 1, 2]);
        let ladder = vec![meta(1, "a", "a"), meta(3, "b", "b"), meta(2, "c", "c")];
        assert_eq!(outline_levels(&ladder), [1, 2, 2]);
        assert!(outline_levels(&[]).is_empty());
    }
}
