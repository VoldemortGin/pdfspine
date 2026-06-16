#!/usr/bin/env python3
"""JATS / NLM (PMC ``.nxml``) -> ground-truth body text (pure stdlib).

PMC open-access articles ship a JATS/NLM XML file alongside the PDF. That XML is
authored, logically-ordered source text — unlike the PDF, it has no running
heads, footers, page numbers, or column-flow ambiguity. It therefore makes an
*objective* ground truth for "what readable text should the PDF contain, and in
what order", which is exactly what the multi-column reading-order problem needs.

This module turns an ``.nxml`` document into the logical-order body text that
should correspond to the PDF's readable content:

  * article title
  * abstract paragraphs
  * the ``<body>`` — section titles (``<title>``) and paragraphs (``<p>``)

By default it EXCLUDES page-furniture and non-flowing material that the PDF's
running text would not (or that the scorer should not penalise reading order
for): ``<ref-list>`` / references, ``<table-wrap>`` / tables, ``<fig>`` figure
graphics, ``<front>`` metadata beyond the title, footnotes, and inline
``<xref>`` / ``<label>`` cross-references and labels. Figure/table *captions*
are configurable (off by default).

Everything here uses only :mod:`xml.etree.ElementTree`. JATS files vary wildly
in whether (and how) they declare XML namespaces, so all tag matching is done on
the *local name* (the part after any ``}`` or ``:``), never on a fully-qualified
or prefixed name.

CLI::

    python conformance/gt/jats_text.py <file.nxml>          # print body text
    python conformance/gt/jats_text.py --self-test          # run offline self-test
    python conformance/gt/jats_text.py                      # (no args) -> self-test

Run with the project venv::

    .venv/bin/python conformance/gt/jats_text.py <file.nxml>
"""

from __future__ import annotations

import re
import sys
from xml.etree import ElementTree as ET

# --------------------------------------------------------------------------- #
# Configuration of which logical sections to include.
# --------------------------------------------------------------------------- #
DEFAULT_INCLUDE: set[str] = {"title", "abstract", "body"}

# Block elements whose subtree we never descend into when gathering body text —
# they are references, tables, figures (graphics), footnotes, and explicit
# cross-reference / label page-furniture. Captions are handled specially (see
# ``include_captions``). Matched on local-name, case-insensitive.
_SKIP_SUBTREES: frozenset[str] = frozenset(
    {
        "ref-list",
        "ref",
        "table-wrap",
        "table-wrap-foot",
        "table",
        "fig",
        "fig-group",
        "disp-formula",  # display math: no meaningful Unicode flow text
        "inline-formula",
        "tex-math",
        "mml:math",
        "math",
        "graphic",
        "media",
        "fn",  # footnote
        "fn-group",
        "author-notes",
        "xref",  # cross-reference marker, e.g. "[12]" / "Fig. 1"
        "label",  # section/figure label furniture, e.g. "1.", "Figure 1"
        "object-id",
        "supplementary-material",
    }
)

# Inline markup whose text content is kept inline (collapsed) rather than
# treated as a block. Anything not block-level and not skipped is effectively
# inline, but listing these documents intent.
_INLINE_TAGS: frozenset[str] = frozenset(
    {"italic", "bold", "sup", "sub", "sc", "underline", "monospace", "named-content", "styled-content"}
)

_WS = re.compile(r"\s+")


# --------------------------------------------------------------------------- #
# Namespace-robust helpers
# --------------------------------------------------------------------------- #
def _local(tag: object) -> str:
    """Return the lowercase local-name of an element tag, namespace-stripped.

    Handles ``{ns}tag`` (ElementTree expanded form), ``prefix:tag`` (raw), and
    bare ``tag``. Non-str tags (e.g. comments/PIs are callables) yield ``""``.
    """
    if not isinstance(tag, str):
        return ""
    if "}" in tag:
        tag = tag.rsplit("}", 1)[1]
    if ":" in tag:
        tag = tag.rsplit(":", 1)[1]
    return tag.lower()


def _collapse(text: str) -> str:
    """Collapse internal whitespace runs to single spaces and strip ends."""
    return _WS.sub(" ", text).strip()


def _inline_text(elem: ET.Element, include_captions: bool) -> str:
    """Flatten an element's full inline text in document order.

    Collapses inline markup (italic/bold/sup/sub/sc/...) to their text content,
    and DROPS any skipped subtree (xref, label, inline math, ...) entirely while
    preserving the surrounding ``.text``/``.tail`` so words don't fuse together.
    Used for leaf-ish blocks like ``<title>`` and ``<p>``.
    """
    parts: list[str] = []
    if elem.text:
        parts.append(elem.text)
    for child in elem:
        name = _local(child.tag)
        if name in _SKIP_SUBTREES and not (include_captions and name == "caption"):
            # Skip the child's own content, but keep its tail (text that follows
            # the marker still belongs to the running sentence).
            if child.tail:
                parts.append(child.tail)
            continue
        # Recurse: inline markup and any other nested inline content.
        inner = _inline_text(child, include_captions)
        if inner:
            parts.append(inner)
        if child.tail:
            parts.append(child.tail)
    return _collapse("".join(parts))


# --------------------------------------------------------------------------- #
# Block-level walkers
# --------------------------------------------------------------------------- #
def _walk_body(elem: ET.Element, include_captions: bool, out: list[str]) -> None:
    """Walk a ``<body>``/``<sec>`` subtree in document order.

    Emits each ``<title>`` and ``<p>`` (and, if enabled, ``<caption>``) as its
    own logical line, descending into ``<sec>`` recursively and skipping the
    non-flowing subtrees (refs/tables/figs/footnotes/...).
    """
    for child in elem:
        name = _local(child.tag)
        if name in _SKIP_SUBTREES:
            if include_captions and name in ("fig", "fig-group", "table-wrap"):
                # Even when we skip the figure/table itself, we may still want
                # its caption text. Pull only caption descendants.
                for cap in child.iter():
                    if _local(cap.tag) == "caption":
                        _emit_block_paragraphs(cap, include_captions, out)
            continue
        if name == "caption":
            if include_captions:
                _emit_block_paragraphs(child, include_captions, out)
            continue
        if name in ("title", "p"):
            text = _inline_text(child, include_captions)
            if text:
                out.append(text)
            # A <p> can in malformed JATS contain nested <sec>; recurse to be safe.
            for sub in child:
                if _local(sub.tag) in ("sec", "boxed-text"):
                    _walk_body(sub, include_captions, out)
            continue
        if name in ("sec", "boxed-text", "body", "list", "list-item", "abstract", "trans-abstract"):
            _walk_body(child, include_captions, out)
            continue
        # Unknown wrapper: descend so we don't lose nested paragraphs.
        if len(child):
            _walk_body(child, include_captions, out)


def _emit_block_paragraphs(elem: ET.Element, include_captions: bool, out: list[str]) -> None:
    """Emit ``<title>``/``<p>`` lines found anywhere under ``elem`` (for captions/abstracts)."""
    found_any = False
    for node in elem.iter():
        if node is elem:
            continue
        if _local(node.tag) in ("title", "p"):
            text = _inline_text(node, include_captions)
            if text:
                out.append(text)
                found_any = True
    if not found_any:
        # Caption/abstract with bare inline text and no <p> wrapper.
        text = _inline_text(elem, include_captions)
        if text:
            out.append(text)


def _find_article_title(root: ET.Element, include_captions: bool) -> str:
    """Find the main article title (first ``<article-title>`` in document order)."""
    for node in root.iter():
        if _local(node.tag) == "article-title":
            return _inline_text(node, include_captions)
    return ""


def _find_abstract_paragraphs(root: ET.Element, include_captions: bool) -> list[str]:
    """Collect abstract paragraph text. Excludes any ``trans-abstract`` is kept too.

    We take every ``<abstract>`` (and translated abstract) but skip a
    ``<abstract abstract-type="...">`` that is graphical/teaser only when it has
    no paragraph text.
    """
    out: list[str] = []
    for node in root.iter():
        if _local(node.tag) in ("abstract", "trans-abstract"):
            _walk_body(node, include_captions, out)
    return out


# --------------------------------------------------------------------------- #
# Public API
# --------------------------------------------------------------------------- #
def nxml_to_text(
    nxml: bytes | str,
    include: set[str] | None = None,
    *,
    include_captions: bool = False,
) -> str:
    """Extract logical-order ground-truth text from a JATS/NLM ``.nxml`` document.

    Parameters
    ----------
    nxml:
        The XML, as ``bytes`` (preferred — preserves the declared encoding) or
        ``str``.
    include:
        Which logical sections to emit. Subset of ``{"title", "abstract",
        "body"}``. Defaults to all three.
    include_captions:
        If ``True``, figure/table ``<caption>`` text is included (default
        ``False`` — captions usually float and would hurt reading-order scoring).

    Returns
    -------
    str
        Newline-joined logical text (title, then abstract paragraphs, then the
        body's section titles and paragraphs), in document order. References,
        tables, figure graphics, footnotes, and ``<xref>``/``<label>``
        page-furniture are excluded.
    """
    include = DEFAULT_INCLUDE if include is None else {s.lower() for s in include}

    # ElementTree.fromstring accepts both str and bytes. For bytes it honours the
    # XML declaration's encoding; for str the document must not declare a
    # conflicting encoding, so callers passing decoded text are responsible for that.
    if isinstance(nxml, (bytes, bytearray)):
        root = ET.fromstring(bytes(nxml))
    else:
        root = ET.fromstring(nxml)

    lines: list[str] = []

    if "title" in include:
        title = _find_article_title(root, include_captions)
        if title:
            lines.append(title)

    if "abstract" in include:
        lines.extend(_find_abstract_paragraphs(root, include_captions))

    if "body" in include:
        for node in root.iter():
            if _local(node.tag) == "body":
                _walk_body(node, include_captions, lines)
                break  # a JATS article has exactly one <body>

    # De-blank and return.
    return "\n".join(line for line in lines if line)


def nxml_body_paragraphs(
    nxml: bytes | str,
    *,
    include_captions: bool = False,
) -> list[str]:
    """Return the ordered ``<body>`` paragraph/title strings (for order scoring).

    Unlike :func:`nxml_to_text` this returns only the body block list (no title
    line, no abstract) — handy as the reference sequence when measuring
    reading-order against the PDF.
    """
    if isinstance(nxml, (bytes, bytearray)):
        root = ET.fromstring(bytes(nxml))
    else:
        root = ET.fromstring(nxml)
    out: list[str] = []
    for node in root.iter():
        if _local(node.tag) == "body":
            _walk_body(node, include_captions, out)
            break
    return [line for line in out if line]


# --------------------------------------------------------------------------- #
# Self-test (offline, no network, no PDF)
# --------------------------------------------------------------------------- #
_SELFTEST_NXML = """<?xml version="1.0" encoding="UTF-8"?>
<article xmlns:xlink="http://www.w3.org/1999/xlink"
         xmlns:mml="http://www.w3.org/1998/Math/MathML">
  <front>
    <journal-meta>
      <journal-title>Journal of Synthetic Conformance</journal-title>
    </journal-meta>
    <article-meta>
      <title-group>
        <article-title>On the <italic>Reading Order</italic> of Multi-Column Documents</article-title>
      </title-group>
      <contrib-group>
        <contrib><name><surname>Doe</surname><given-names>Jane</given-names></name></contrib>
      </contrib-group>
      <abstract>
        <p>This abstract paragraph describes the synthetic study in one sentence.</p>
        <p>A second abstract sentence about columns and order.</p>
      </abstract>
    </article-meta>
  </front>
  <body>
    <sec id="s1">
      <label>1</label>
      <title>Introduction Section</title>
      <p>The introduction body paragraph mentions reading order <xref ref-type="bibr" rid="b1">[1]</xref> explicitly.</p>
      <p>The introduction continues in a second <bold>bold</bold> paragraph.</p>
    </sec>
    <sec id="s2">
      <title>Methods Section</title>
      <p>The methods body paragraph explains the procedure with H<sub>2</sub>O and E=mc<sup>2</sup>.</p>
      <table-wrap id="t1">
        <label>Table 1</label>
        <caption><p>TABLECAPTIONSHOULDBEEXCLUDED by default.</p></caption>
        <table><tbody><tr><td>TABLECELLEXCLUDED</td></tr></tbody></table>
      </table-wrap>
      <fig id="f1">
        <label>Figure 1</label>
        <caption><p>FIGCAPTIONEXCLUDED by default.</p></caption>
        <graphic xlink:href="f1.jpg"/>
      </fig>
    </sec>
  </body>
  <back>
    <ref-list>
      <title>References</title>
      <ref id="b1"><mixed-citation>REFERENCETEXTSHOULDBEEXCLUDED, 2020.</mixed-citation></ref>
    </ref-list>
    <fn-group>
      <fn id="fn1"><p>FOOTNOTETEXTEXCLUDED.</p></fn>
    </fn-group>
  </back>
</article>
"""


def _self_test() -> int:
    text = nxml_to_text(_SELFTEST_NXML)

    # 1. Title is present, with inline <italic> collapsed.
    assert "On the Reading Order of Multi-Column Documents" in text, "article title missing/markup not collapsed"

    # 2. Abstract paragraphs present, in order.
    assert "This abstract paragraph describes the synthetic study in one sentence." in text, "abstract p1 missing"
    assert "A second abstract sentence about columns and order." in text, "abstract p2 missing"

    # 3. Body section titles + paragraphs present, in document order.
    intro_p1 = "The introduction body paragraph mentions reading order explicitly."  # <xref> dropped, words not fused
    assert "Introduction Section" in text, "intro section title missing"
    assert intro_p1 in text, f"intro p1 wrong (xref handling). got around: {text!r}"
    assert "The introduction continues in a second bold paragraph." in text, "intro p2 / <bold> collapse failed"
    assert "Methods Section" in text, "methods section title missing"
    assert "with H2O and E=mc2" in text, "sub/sup collapse failed"

    # 4. Ordering: abstract before body; intro before methods; p1 before p2.
    def pos(s: str) -> int:
        i = text.find(s)
        assert i >= 0, f"expected substring not found: {s!r}"
        return i

    assert pos("This abstract paragraph") < pos("Introduction Section"), "abstract must precede body"
    assert pos("Introduction Section") < pos("Methods Section"), "intro must precede methods"
    assert pos(intro_p1) < pos("The introduction continues"), "intro paragraphs out of order"

    # 5. EXCLUSIONS — none of these may appear by default.
    for forbidden in (
        "REFERENCETEXTSHOULDBEEXCLUDED",
        "TABLECELLEXCLUDED",
        "TABLECAPTIONSHOULDBEEXCLUDED",
        "FIGCAPTIONEXCLUDED",
        "FOOTNOTETEXTEXCLUDED",
        "Table 1",
        "Figure 1",
        "References",  # ref-list title excluded
        "[1]",  # xref marker furniture
    ):
        assert forbidden not in text, f"excluded content leaked into output: {forbidden!r}"

    # 6. The <label>1</label> section furniture must not prefix the title.
    assert "1\nIntroduction Section" not in text, "section <label> leaked"

    # 7. Helper returns ordered body blocks only (no title/abstract line).
    paras = nxml_body_paragraphs(_SELFTEST_NXML)
    assert paras[0] == "Introduction Section", f"body[0] should be intro title, got {paras[0]!r}"
    assert "On the Reading Order" not in "\n".join(paras), "body paragraphs must not include the article title"
    assert "This abstract paragraph" not in "\n".join(paras), "body paragraphs must not include the abstract"
    assert intro_p1 in paras, "body helper missing intro p1"

    # 8. include= filtering works (body-only).
    body_only = nxml_to_text(_SELFTEST_NXML, include={"body"})
    assert "On the Reading Order" not in body_only, "title leaked with include={body}"
    assert "This abstract paragraph" not in body_only, "abstract leaked with include={body}"
    assert "Introduction Section" in body_only, "body missing with include={body}"

    # 9. include_captions=True opt-in surfaces caption text.
    with_caps = nxml_to_text(_SELFTEST_NXML, include_captions=True)
    assert "FIGCAPTIONEXCLUDED" in with_caps, "include_captions=True should surface figure caption"
    assert "TABLECAPTIONSHOULDBEEXCLUDED" in with_caps, "include_captions=True should surface table caption"
    assert "TABLECELLEXCLUDED" not in with_caps, "table CELL must still be excluded even with captions on"
    assert "REFERENCETEXTSHOULDBEEXCLUDED" not in with_caps, "references must stay excluded with captions on"

    # 10. Bytes input (with declared encoding) parses identically.
    text_bytes = nxml_to_text(_SELFTEST_NXML.encode("utf-8"))
    assert text_bytes == text, "bytes vs str input produced different output"

    print("jats_text.py self-test OK")
    return 0


def _cli(argv: list[str]) -> int:
    args = [a for a in argv[1:] if a != "--"]
    if not args or args[0] == "--self-test":
        return _self_test()
    path = args[0]
    with open(path, "rb") as fh:
        data = fh.read()
    sys.stdout.write(nxml_to_text(data))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(_cli(sys.argv))
