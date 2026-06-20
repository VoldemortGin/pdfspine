#!/usr/bin/env python3
"""Single source of truth for the PyMuPDF 1.24.x compatibility catalog.

This module enumerates the pinned PyMuPDF **public baseline** symbol set (one
entry per symbol) and each symbol's **disposition** for ``pdfspine``:

    implemented   — present in ``python/`` and does not raise PdfUnsupportedError
    deferred      — known, planned for a later milestone (M3–M6 / post-v1)
    out-of-scope  — intentionally never in v1; raises PdfUnsupportedError

It is *data only*. Two artifacts are generated from it and committed:

    COMPAT.toml          — the machine-readable dispositioned matrix (PRD §7/§9.5)
    compat/compat-baseline.txt
                         — the flat, checked-in baseline symbol list the guard
                           diffs COMPAT.toml against (real PyMuPDF is not
                           installed at CI time, so the baseline is a snapshot)

Regenerate both with::

    python3 scripts/_compat_catalog.py

The symbol list + statuses are derived from ``PARITY.md`` (the 437-item catalog)
cross-checked against the real ``python/pdfspine`` + ``python/fitz`` source: a
PARITY tick **and** a non-stub implementation in source → ``implemented``;
deferred milestones → ``deferred``; out-of-scope / post-v1 → ``out-of-scope``.

Baseline: **PyMuPDF 1.24.x** (1.24.14 / MuPDF 1.24.11), per PRD §17.1.
"""

from __future__ import annotations

from pathlib import Path

BASELINE = "1.24.x"

IMPLEMENTED = "implemented"
DEFERRED = "deferred"
OUT_OF_SCOPE = "out-of-scope"

# Each entry: (symbol, group, disposition, milestone, note)
#   symbol      "Class.member" / "Class" / module-level "name"
#   group       owning class / section (for grouping in COMPAT.toml)
#   disposition implemented | deferred | out-of-scope
#   milestone   M0..M6 / post-v1 / out-of-scope / "" (where N/A)
#   note        short rationale (may be "")
Entry = tuple[str, str, str, str, str]

CATALOG: list[Entry] = []


def add(symbol: str, group: str, disp: str, milestone: str = "", note: str = "") -> None:
    CATALOG.append((symbol, group, disp, milestone, note))


def add_many(group: str, disp: str, milestone: str, members: list[str], note: str = "") -> None:
    for m in members:
        add(f"{group}.{m}" if group not in ("module", "constants") else m, group, disp, milestone, note)


# ---------------------------------------------------------------------------
# 1. Geometry (M0) — pure value types, PARITY-ticked, present in pdfspine.geometry
# ---------------------------------------------------------------------------
add_many("Matrix", IMPLEMENTED, "M0", [
    "Matrix", "concat", "invert", "norm", "prerotate", "prescale", "preshear",
    "pretranslate", "is_rectilinear", "a", "b", "c", "d", "e", "f",
    "__mul__", "__invert__", "__add__", "__sub__", "__eq__", "__abs__",
    "__len__", "__getitem__", "__repr__", "__bool__",
])
add_many("Point", IMPLEMENTED, "M0", [
    "Point", "distance_to", "transform", "norm", "unit", "abs_unit", "x", "y",
    "__add__", "__sub__", "__mul__", "__truediv__", "__invert__", "__eq__",
    "__abs__", "__getitem__", "__len__", "__repr__",
])
add_many("Rect", IMPLEMENTED, "M0", [
    "Rect", "intersect", "include_rect", "include_point", "intersects",
    "contains", "normalize", "transform", "morph", "torect", "round",
    "get_area", "norm", "width", "height", "x0", "y0", "x1", "y1",
    "tl", "tr", "bl", "br", "top_left", "top_right", "bottom_left",
    "bottom_right", "is_empty", "is_infinite", "is_valid", "irect", "quad",
    "__and__", "__or__", "__mul__", "__add__", "__sub__", "__truediv__",
    "__invert__", "__eq__", "__contains__", "__getitem__", "__len__",
    "__repr__", "__abs__",
])
add_many("IRect", IMPLEMENTED, "M0", [
    "IRect", "get_area", "include_point", "include_rect", "intersect",
    "intersects", "morph", "norm", "normalize", "torect", "transform",
    "rect", "width", "height", "x0", "y0", "x1", "y1", "is_empty",
    "is_infinite", "irect", "quad", "__getitem__", "__len__", "__repr__",
])
add_many("Quad", IMPLEMENTED, "M0", [
    "Quad", "transform", "morph", "width", "height", "rect", "ul", "ur",
    "ll", "lr", "is_convex", "is_empty", "is_infinite", "is_rectangular",
    "__mul__", "__invert__", "__eq__",
])
add_many("constants", IMPLEMENTED, "M0", [
    "EMPTY_RECT", "EMPTY_IRECT", "EMPTY_QUAD", "INFINITE_RECT",
    "INFINITE_IRECT", "INFINITE_QUAD", "Identity", "IdentityMatrix",
    "EPSILON", "FLT_EPSILON", "FZ_MIN_INF_RECT", "FZ_MAX_INF_RECT",
    "rect_like", "point_like", "matrix_like", "quad_like",
], note="geometry singletons / numeric constants / type aliases")

# ---------------------------------------------------------------------------
# 2. Document
# ---------------------------------------------------------------------------
# Open / lifecycle / save
add_many("Document", IMPLEMENTED, "M1", ["open", "Document", "close"])
add_many("Document", IMPLEMENTED, "M3", [
    "save", "ez_save", "saveIncr", "save_incremental", "write", "tobytes",
])
add("Document.can_save_incrementally", "Document", IMPLEMENTED, "M3", "incremental-safety predicate")
add("Document.save_snapshot", "Document", OUT_OF_SCOPE, "post-v1", "journalling deferred (PRD §3.2 #5)")
# Pages — access / layout
add_many("Document", IMPLEMENTED, "M1", ["load_page", "__getitem__", "pages", "page_count"])
add_many("Document", IMPLEMENTED, "M3", ["new_page", "insert_pdf", "delete_page", "select"])
add_many("Document", IMPLEMENTED, "M3", ["fullcopy_page", "reload_page", "page_xref"])
add_many("Document", IMPLEMENTED, "M3", ["page_cropbox"], "per-page /CropBox accessor")
add_many("Document", IMPLEMENTED, "M3", [
    "insert_page", "copy_page", "move_page", "delete_pages",
])
add("Document.insert_file", "Document", DEFERRED, "M5", "image inputs in scope; non-image unsupported")
add("Document.layout", "Document", OUT_OF_SCOPE, "out-of-scope", "EPUB-class reflow (PRD §3.2 #8)")
# Chapter / location model — PDF is a flat single-chapter model: the trivial
# chapter accessors are implemented; the reflowable-doc bookmark API is out of scope.
add_many("Document", IMPLEMENTED, "M3", [
    "chapter_count", "chapter_page_count", "last_location",
], "single-chapter PDF model (chapter 0 holds every page)")
add_many("Document", OUT_OF_SCOPE, "out-of-scope", [
    "next_location", "prev_location", "location_from_page_number",
    "page_number_from_location", "make_bookmark", "find_bookmark",
], "EPUB-class reflow (PRD §3.2 #8)")
# Metadata / TOC / outline
add_many("Document", IMPLEMENTED, "M1", ["metadata"])
add_many("Document", IMPLEMENTED, "M3", [
    "set_metadata", "get_toc", "set_toc", "get_xml_metadata", "set_xml_metadata",
])
add_many("Document", IMPLEMENTED, "M3", [
    "set_toc_item", "del_toc_item",
], "surgical /Outlines item edits (fitz _update_toc_item / _remove_toc_item primitives)")
add_many("Document", DEFERRED, "M3", [
    "outline", "del_xml_metadata",
])
add("Document.xref_xml_metadata", "Document", IMPLEMENTED, "M3", "xref of catalog /Metadata XML stream (0 if none)")
# Security / permissions
add_many("Document", IMPLEMENTED, "M1", [
    "needs_pass", "authenticate", "permissions", "is_encrypted",
])
add("Document.get_sigflags", "Document", IMPLEMENTED, "M4", "/AcroForm /SigFlags int, -1 when no form")
# Identity / state props
add_many("Document", IMPLEMENTED, "M1", ["is_pdf", "is_repaired"])
add_many("Document", IMPLEMENTED, "M4", ["is_form_pdf"])
add_many("Document", IMPLEMENTED, "M3", [
    "is_dirty", "is_reflowable", "language", "set_language", "markinfo",
    "set_markinfo", "pagelayout", "set_pagelayout", "pagemode", "set_pagemode",
])
add_many("Document", IMPLEMENTED, "M1", [
    "is_fast_webaccess", "is_closed", "name",
])
add("Document.version_count", "Document", IMPLEMENTED, "M1", "cross-reference revision count: startxref/`/Prev`-chain length minus the linearized first-page section (matches fitz exactly)")
add("Document.need_appearances", "Document", IMPLEMENTED, "M4", "/AcroForm /NeedAppearances get/set, None when no form")
add_many("Document", DEFERRED, "M4", ["FormFonts"])
# Conversion / embedded files / fonts
add("Document.convert_to_pdf", "Document", IMPLEMENTED, "M5", "image inputs converted to a 1-page PDF (fitz.open + convert_to_pdf); non-image raises PdfUnsupportedError")
add_many("Document", IMPLEMENTED, "M4", [
    "embfile_add", "embfile_get", "embfile_del", "embfile_info",
    "embfile_count", "embfile_names",
])
add_many("Document", IMPLEMENTED, "M4", ["embfile_upd"], "update embedded-file content/names/desc in place + /Params /ModDate")
add_many("Document", IMPLEMENTED, "M5", ["extract_font"], "embedded /FontFile* program + (basefont, ext, type, buffer) — byte-for-byte vs fitz across cff/ttf/cid/otf")
add_many("Document", DEFERRED, "M5", ["extract_image", "subset_fonts"])
add_many("Document", IMPLEMENTED, "M2", ["get_char_widths"], "font /Widths → (glyph, width) pairs")
add_many("Document", IMPLEMENTED, "M4", ["bake", "scrub"])
add("Document.resolve_link", "Document", IMPLEMENTED, "M3", "URI fragment / named-destination → page index")
add_many("Document", IMPLEMENTED, "M3", [
    "subset",
], "font-subset entry point mirroring fitz's MuPDF subset path (returns None; never corrupts)")
add_many("Document", IMPLEMENTED, "M3", [
    "get_outline_xrefs",
], "walk /Outlines /First→/Next chain → outline-item xref list (fitz JM_outline_xrefs)")
add("Document.resolve_names", "Document", IMPLEMENTED, "M3", "all /Dests names → {page, to, zoom, dest} (fitz-shaped)")
# Low-level xref / object access
add_many("Document", IMPLEMENTED, "M1", [
    "xref_length", "xref_object", "xref_stream", "xref_get_key", "xref_is_stream",
])
add_many("Document", IMPLEMENTED, "M1", [
    "xref_stream_raw", "xref_get_keys", "xref_is_font", "xref_is_image",
    "xref_is_xobject", "pdf_catalog", "pdf_trailer", "is_stream",
    "page_annot_xrefs",
])
add_many("Document", IMPLEMENTED, "M3", [
    "update_object", "update_stream", "get_new_xref", "xref_set_key", "xref_copy",
])
# Optional content (OCG/layers) — core read/write surface implemented (M7)
add_many("Document", IMPLEMENTED, "M7", [
    "add_ocg", "get_ocgs", "get_layer", "set_layer", "layer_ui_configs", "set_oc",
], "OCG read + add/toggle/bind (M7)")
add_many("Document", DEFERRED, "post-v1", [
    "add_layer", "get_layers", "switch_layer", "set_layer_ui_config",
    "get_oc", "get_ocmd", "set_ocmd",
], "OCMD / layer-config nesting deferred (PRD §3.2 #5)")
# Page labels
add_many("Document", IMPLEMENTED, "M3", [
    "get_page_labels", "get_page_numbers", "get_label",
])
add("Document.get_page_label", "Document", IMPLEMENTED, "M3", "delegates to the /PageLabels parser (Page.get_label)")
add("Document.set_page_labels", "Document", IMPLEMENTED, "M3", "writes /Root /PageLabels number tree")
# Document-wide page-content helpers
add_many("Document", IMPLEMENTED, "M2", ["get_page_text"])
add("Document.get_page_pixmap", "Document", IMPLEMENTED, "M5", "full-page render (delegates to Page.get_pixmap)")
add("Document.get_page_xobjects", "Document", IMPLEMENTED, "M2", "per-page XObject inventory (Form + Image)")
add_many("Document", IMPLEMENTED, "M2", [
    "get_page_images", "get_page_fonts", "search_page_for",
])
# Forms helpers actually implemented in source
add_many("Document", IMPLEMENTED, "M4", [
    "form_field_names", "form_fill", "form_flatten",
])
# Journalling — minimal snapshot-based undo/redo (M3)
add_many("Document", IMPLEMENTED, "M3", [
    "journal_enable", "journal_is_enabled", "journal_undo", "journal_redo",
    "journal_can_do",
], "ChangeSet-snapshot undo/redo (enable/checkpoint/undo/redo/can_do)")
add_many("Document", OUT_OF_SCOPE, "post-v1", [
    "journal_start_op", "journal_stop_op",
    "journal_op_name", "journal_position", "journal_save", "journal_load",
], "per-op naming + journal persistence out of scope (PRD §3.2 #5)")

# ---------------------------------------------------------------------------
# 3. Page
# ---------------------------------------------------------------------------
# Text extraction
add_many("Page", IMPLEMENTED, "M2", ["get_text", "get_textpage"])
add_many("Page", DEFERRED, "M2", [
    "get_text_blocks", "get_text_words", "get_textbox", "get_text_selection",
    "extend_textpage", "get_texttrace",
])
add("Page.get_textpage_ocr", "Page", IMPLEMENTED, "M8", "OCR via pluggable engine (Tesseract default)")
add("Page.TEXTFLAGS", "Page", IMPLEMENTED, "M2", "per-method default flag sets pinned")
# Search & links
add_many("Page", IMPLEMENTED, "M2", ["search_for"])
add_many("Page", IMPLEMENTED, "M4", ["get_links", "insert_link", "delete_link"])
add_many("Page", IMPLEMENTED, "M4", ["load_links", "update_link"])
add_many("Page", DEFERRED, "M4", ["links", "first_link"])
# Rendering — deferred
add("Page.get_pixmap", "Page", IMPLEMENTED, "M6", "image pages (M5) + full vector-page render (M6d)")
add_many("Page", IMPLEMENTED, "M6", ["get_displaylist"], "records the ordered render-op stream (M6d)")
add("Page.get_svg_image", "Page", IMPLEMENTED, "M7", "page → standalone SVG string (M7)")
add("Page.run", "Page", DEFERRED, "M6", "device-callback replay deferred; get_pixmap covers the raster path")
add_many("Page", IMPLEMENTED, "M1", ["bound"])
# Vector / image / font inventory
add_many("Page", IMPLEMENTED, "M4", ["get_drawings", "get_cdrawings"])
add_many("Page", IMPLEMENTED, "M2", ["get_fonts", "get_images"])
add_many("Page", IMPLEMENTED, "M4", ["cluster_drawings"])
add_many("Page", DEFERRED, "M4", ["get_bboxlog"])
add("Page.find_tables", "Page", IMPLEMENTED, "M7", "table detection: lines/lines_strict/text strategies (M7)")
add_many("Page", IMPLEMENTED, "M2", ["get_image_rects", "get_xobjects"])
add_many("Page", IMPLEMENTED, "M2", [
    "get_image_info", "get_image_bbox",
], "per-image placement dicts / single-image bbox lookup")
# Drawing primitives
add_many("Page", IMPLEMENTED, "M4", [
    "draw_line", "draw_rect", "draw_circle", "draw_oval", "draw_bezier",
    "draw_polyline", "new_shape",
    "draw_curve", "draw_quad", "draw_sector", "draw_squiggle", "draw_zigzag",
])
# Text & image insertion
add_many("Page", IMPLEMENTED, "M4", [
    "insert_text", "insert_textbox", "insert_image",
])
add("Page.show_pdf_page", "Page", IMPLEMENTED, "M4", "place another PDF page as a Form XObject (n-up/stamp/watermark)")
add_many("Page", DEFERRED, "M4", [
    "insert_font", "write_text", "replace_image", "delete_image",
])
add("Page.insert_htmlbox", "Page", OUT_OF_SCOPE, "post-v1", "HTML/CSS engine out of scope (PRD §3.2 #2)")
# Annotations
add_many("Page", IMPLEMENTED, "M4", [
    "annots", "first_annot", "annot_names", "annot_xrefs", "delete_annot",
    "add_text_annot", "add_freetext_annot", "add_highlight_annot",
    "add_underline_annot", "add_strikeout_annot", "add_squiggly_annot",
    "add_rect_annot", "add_circle_annot", "add_line_annot", "add_polyline_annot",
    "add_polygon_annot", "add_ink_annot", "add_stamp_annot", "add_file_annot",
    "add_redact_annot", "apply_redactions",
])
add_many("Page", IMPLEMENTED, "M4", ["load_annot"])
add_many("Page", IMPLEMENTED, "M4", ["add_caret_annot"], "/Caret insertion marker (blue, fitz device rect + /AP)")
# Widgets / forms
add_many("Page", IMPLEMENTED, "M4", ["widgets", "first_widget"])
add_many("Page", IMPLEMENTED, "M4", ["load_widget", "delete_widget"])
add_many("Page", IMPLEMENTED, "M4", ["add_widget"], "authors a /Widget field + registers /AcroForm (text/checkbox/combo/list); oracle round-trips")
# Content-stream maintenance
add_many("Page", IMPLEMENTED, "M3", ["get_contents", "read_contents"])
add_many("Page", IMPLEMENTED, "M3", ["set_contents"], "point /Contents at a stream xref (validated)")
add_many("Page", IMPLEMENTED, "M4", ["is_wrapped"])
add_many("Page", DEFERRED, "M4", ["clean_contents", "wrap_contents"])
# Geometry / boxes / rotation
add_many("Page", IMPLEMENTED, "M1", [
    "rect", "mediabox", "cropbox", "rotation", "number",
])
add_many("Page", IMPLEMENTED, "M1", [
    "mediabox_size", "cropbox_position", "artbox", "bleedbox", "trimbox",
    "transformation_matrix", "rotation_matrix", "derotation_matrix", "xref",
    "parent",
])
add_many("Page", IMPLEMENTED, "M3", ["set_rotation"])
add_many("Page", IMPLEMENTED, "M3", ["get_label"])
add_many("Page", IMPLEMENTED, "M3", [
    "set_mediabox", "set_cropbox", "set_artbox", "set_bleedbox", "set_trimbox",
])
add_many("Page", DEFERRED, "M3", [
    "remove_rotation", "refresh",
])
add_many("Page", IMPLEMENTED, "M3", [
    "language", "set_language",
], "inheritable /Lang get; set normalizes to MuPDF ISO-639 (mirrors Annot /Lang)")
add("Page.get_oc_items", "Page", OUT_OF_SCOPE, "post-v1", "OCG out of scope (PRD §3.2 #5)")

# ---------------------------------------------------------------------------
# 4. TextPage
# ---------------------------------------------------------------------------
add_many("TextPage", IMPLEMENTED, "M2", [
    "extractText", "extractTEXT", "extractBLOCKS", "extractWORDS",
    "extractDICT", "extractJSON", "extractRAWDICT", "rect",
])
add_many("TextPage", DEFERRED, "M2", [
    "extractRAWJSON", "extractHTML", "extractXHTML", "extractXML",
    "extractIMGINFO", "extractSelection", "extractTextbox", "search", "poolsize",
])

# ---------------------------------------------------------------------------
# 5. Pixmap — M5, none implemented yet
# ---------------------------------------------------------------------------
add_many("Pixmap", IMPLEMENTED, "M5", [
    "Pixmap", "save", "tobytes", "pixel", "set_pixel", "set_rect",
    "set_alpha", "clear_with", "invert_irect", "shrink", "copy",
    "samples", "samples_mv", "stride", "width", "height", "w", "h",
    "irect", "n", "alpha", "colorspace", "size",
])
add_many("Pixmap", IMPLEMENTED, "M5", [
    "set_origin", "set_dpi", "tint_with", "gamma_with", "color_count",
    "color_topusage", "x", "y", "digest", "xres", "yres",
    "is_monochrome", "is_unicolor",
], "pure-pixel ops + origin/dpi metadata + stable-hash digest")
add_many("Pixmap", DEFERRED, "M5", [
    "warp",
])
add_many("Pixmap", IMPLEMENTED, "M5", [
    "samples_ptr", "__array_interface__",
], "samples_ptr = int address of samples; __array_interface__ wraps the buffer zero-copy (h,w,n) uint8")
add_many("Pixmap", IMPLEMENTED, "M5", [
    "pil_save", "pil_tobytes",
], "PNG/PPM/PAM bytes under the PyMuPDF Pillow-bridge names")
# OCR sandwich export (M8). PyMuPDF catalogs these under `Pixmap`; pdfspine
# exposes them on `Document` (the whole-document sandwich), with the baseline
# `Pixmap.*` names kept implemented so the search/save surface is covered.
add_many("Pixmap", IMPLEMENTED, "M8", [
    "pdfocr_save", "pdfocr_tobytes",
], "OCR sandwich-PDF export (implemented on Document)")
add_many("Document", IMPLEMENTED, "M8", [
    "pdfocr_save", "pdfocr_tobytes",
], "OCR sandwich-PDF export (searchable invisible text layer)")
add("Page.getTextPageOCR", "Page", IMPLEMENTED, "M8", "camelCase alias of get_textpage_ocr")

# ---------------------------------------------------------------------------
# 6. Annot — wrappers implemented in source for the M4 subset
# ---------------------------------------------------------------------------
add_many("Annot", IMPLEMENTED, "M4", [
    "update", "set_rect", "set_colors", "colors", "set_opacity", "opacity",
    "set_border", "border", "set_flags", "flags", "set_info", "info",
    "type", "rect", "xref", "vertices", "has_ap",
])
add_many("Annot", IMPLEMENTED, "M4", [
    "set_line_ends", "line_ends", "set_blendmode", "blendmode",
    "set_name", "set_open", "is_open",
], "/LE, /BM, /Name, /Open getters+setters")
add_many("Annot", IMPLEMENTED, "M4", [
    "set_rotation", "set_popup", "popup_rect", "popup_xref", "has_popup",
    "set_apn_bbox", "apn_bbox", "set_apn_matrix", "apn_matrix",
    "set_irt_xref", "irt_xref", "delete_responses",
    "get_file", "update_file", "file_info", "clean_contents",
    "set_language", "rect_delta", "language",
], "/Rotate, /RD, /Popup, /AP /N BBox+Matrix, /Lang, /IRT, /FileAttachment (PRD §C batch-3)")
add_many("Annot", DEFERRED, "M4", [
    "get_text", "get_textpage", "next", "get_textbox",
], "get_textbox needs the annot's OWN appearance textpage (fitz semantics), not page-region delegation")
add("Annot.get_pixmap", "Annot", OUT_OF_SCOPE, "M6", "annot rasterization needs M6 renderer")
add("Annot.set_oc", "Annot", OUT_OF_SCOPE, "post-v1", "OCG out of scope")
add("Annot.get_oc", "Annot", OUT_OF_SCOPE, "post-v1", "OCG out of scope")
add("Annot.get_sound", "Annot", OUT_OF_SCOPE, "post-v1", "niche; sound annots")

# ---------------------------------------------------------------------------
# 7. Widget — wrappers implemented for the M4 subset
# ---------------------------------------------------------------------------
add_many("Widget", IMPLEMENTED, "M4", [
    "field_name", "field_label", "field_value", "field_type",
    "field_type_string", "field_flags", "rect", "xref", "choice_values",
    "button_states", "update",
    # Widget appearance (PRD §C batch-3) — read properties, verified vs fitz 1.27.
    "field_display", "border_color", "border_style", "border_width",
    "border_dashes", "fill_color", "text_color", "text_font", "text_fontsize",
    "text_maxlen", "text_format", "button_caption", "is_signed", "rb_parent",
    "reset", "on_state", "next",
])
add_many("Widget", OUT_OF_SCOPE, "M4", [
    "script", "script_calc", "script_change", "script_format", "script_blur",
    "script_focus", "script_stroke",
], "field JavaScript stored, not executed (PRD §3.2)")

# ---------------------------------------------------------------------------
# 8. Link — M4, deferred
# ---------------------------------------------------------------------------
add_many("Link", DEFERRED, "M4", [
    "rect", "dest", "uri", "page", "is_external", "border", "set_border",
    "colors", "set_colors", "flags", "set_flags", "next", "xref", "linkDest",
])

# ---------------------------------------------------------------------------
# 9. Outline / TOC — M3, deferred
# ---------------------------------------------------------------------------
add_many("Outline", DEFERRED, "M3", [
    "title", "dest", "page", "uri", "is_external", "is_open", "next", "down",
    "x", "y", "destination",
])

# ---------------------------------------------------------------------------
# 10. DisplayList — recorded/replayable page render (M6d)
# ---------------------------------------------------------------------------
add_many("DisplayList", IMPLEMENTED, "M6", [
    "DisplayList", "get_pixmap", "rect",
], "Page.get_displaylist records the render-op stream; replay via get_pixmap (M6d)")
add_many("DisplayList", DEFERRED, "M6", [
    "get_textpage", "run",
], "TextPage-from-displaylist / device-callback replay deferred")

# ---------------------------------------------------------------------------
# 11. Shape — wrappers implemented for the M4 subset
# ---------------------------------------------------------------------------
add_many("Shape", IMPLEMENTED, "M4", [
    "draw_line", "draw_rect", "draw_circle", "draw_oval", "draw_bezier",
    "draw_curve", "draw_polyline", "finish", "commit",
])
# Batch 3 (Track C): shape drawing primitives, text, and parent/geometry props.
# The four drawing primitives match real PyMuPDF (1.27) content streams
# operator-for-operator; the properties mirror Shape.__init__ semantics.
add_many("Shape", IMPLEMENTED, "M4", [
    "draw_quad", "draw_sector", "draw_squiggle", "draw_zigzag", "insert_text",
    "insert_textbox", "update_rect", "horizontal_angle", "doc", "page",
    "height", "width", "x", "y", "rect",
])

# ---------------------------------------------------------------------------
# 12. Font — M2, deferred
# ---------------------------------------------------------------------------
add_many("Font", DEFERRED, "M2", [
    "Font", "text_length", "char_lengths", "glyph_advance", "glyph_bbox",
    "has_glyph", "valid_codepoints", "glyph_name_to_unicode",
    "unicode_to_glyph_name", "name", "ascender", "descender", "bbox",
    "glyph_count", "flags", "buffer", "is_bold", "is_italic", "is_serif",
    "is_monospaced", "is_writable", "Base14_fontnames",
])
add("Font.css_for_pymupdf_font", "Font", OUT_OF_SCOPE, "post-v1", "Story-only; HTML engine out of scope")

# ---------------------------------------------------------------------------
# 13. TextWriter — M4, deferred
# ---------------------------------------------------------------------------
add_many("TextWriter", DEFERRED, "M4", [
    "TextWriter", "append", "appendv", "fill_textbox", "write_text",
    "clean_rtl", "text_rect", "last_point", "color", "opacity",
])

# ---------------------------------------------------------------------------
# 14. Story / Xml / Archive — entire subsystem out of scope
# ---------------------------------------------------------------------------
add_many("Story", OUT_OF_SCOPE, "post-v1", [
    "Story", "place", "draw", "write", "write_with_links", "write_stabilized",
    "write_stabilized_with_links", "fit", "fit_height", "fit_width",
    "fit_scale", "element_positions", "reset", "add_pdf_links",
    "add_header_ids", "document", "body",
], "HTML/CSS layout engine out of scope (PRD §3.2 #2)")
add_many("Xml", OUT_OF_SCOPE, "post-v1", [
    "builder_api", "tree_ops", "styling", "navigation_props",
], "HTML/CSS layout engine out of scope (PRD §3.2 #2); grouped families")
add_many("Archive", OUT_OF_SCOPE, "post-v1", [
    "Archive", "add", "has_entry", "read_entry", "entry_list",
], "HTML/CSS layout engine out of scope (PRD §3.2 #2)")

# ---------------------------------------------------------------------------
# 15. Colorspace — M5, deferred
# ---------------------------------------------------------------------------
add_many("Colorspace", DEFERRED, "M5", [
    "Colorspace", "n", "name", "csGRAY", "csRGB", "csCMYK",
])

# ---------------------------------------------------------------------------
# 16. Module-level functions
# ---------------------------------------------------------------------------
add_many("module", IMPLEMENTED, "M1", ["open", "version", "identity_matrix"])
add_many("module", IMPLEMENTED, "M0", ["paper_size", "paper_rect", "paper_sizes"])
# Module-level helper functions — Task 1 (pdfspine.helpers). Pure-Python ports of
# fitz's util functions; every value/return-shape cross-checked vs real PyMuPDF
# 1.27 (.venv-oracle; see test_longtail11.py). Symbol/ZapfDingbats text widths
# bundled for exact get_text_length parity; recover_* reproduce fitz's quad
# geometry exactly (incl. rotated quadrants); the message/log shims port fitz's
# _make_output destination handling.
add_many("module", IMPLEMENTED, "M2", [
    "get_text_length", "sRGB_to_rgb", "sRGB_to_pdf", "glyph_name_to_unicode",
    "unicode_to_glyph_name", "recover_quad", "recover_char_quad",
    "recover_line_quad", "recover_span_quad", "recover_bbox_quad",
    "planish_line", "ConversionHeader", "ConversionTrailer",
], "fitz util helpers, exact 1.27 parity")
add_many("module", IMPLEMENTED, "M3", ["get_pdf_now", "get_pdf_str"], "PDF date/string formatting, fitz-exact")
add_many("module", IMPLEMENTED, "M5", ["image_profile"], "raster header profile dict (width/height/xres/yres/colorspace/bpc/ext/cs-name/transform)")
add_many("module", IMPLEMENTED, "M1", [
    "set_messages", "message", "set_log", "log",
], "message/log output shims (fitz _make_output destinations)")
add_many("module", DEFERRED, "M1", ["Tools", "TOOLS"])
add("module.css_for_pymupdf_font", "module", OUT_OF_SCOPE, "post-v1", "Story-only")
add_many("module", IMPLEMENTED, "M7", ["find_tables"], "table detection via Page.find_tables (M7)")
add_many("module", OUT_OF_SCOPE, "post-v1", [
    "make_table", "get_tessdata",
], "table-builder / OCR out of scope (PRD §3.2 #3/#4)")

# ---------------------------------------------------------------------------
# 17. Tools / TOOLS singleton
# ---------------------------------------------------------------------------
add_many("Tools", DEFERRED, "M3", ["gen_id", "set_annot_stem"])
add_many("Tools", DEFERRED, "M1", [
    "mupdf_warnings", "reset_mupdf_warnings", "mupdf_version", "store_shrink",
    "store_maxsize", "store_size", "mupdf_display_errors",
    "mupdf_display_warnings", "glyph_cache_empty",
    "fitz_config",
])
add_many("Tools", IMPLEMENTED, "M5", ["image_profile"], "raster header profile dict (shared with module-level image_profile)")
add_many("Tools", DEFERRED, "M4", ["set_subset_fontnames"])
add_many("Tools", OUT_OF_SCOPE, "M6", [
    "set_aa_level", "show_aa_level", "set_small_glyph_heights",
    "set_graphics_min_line_width", "set_font_width", "set_icc", "set_low_memory",
], "render/cache-era tuning; no-op + warn until M6")
add("Tools.mupdf_raw_access", "Tools", OUT_OF_SCOPE, "out-of-scope", "raw mupdf.* access raises PdfUnsupportedError (PRD §3.6)")

# ---------------------------------------------------------------------------
# 18. Constant families & exceptions
# ---------------------------------------------------------------------------
# Implemented constants (exposed today)
add_many("constants", IMPLEMENTED, "M1", [
    "PDF_ENCRYPT_NONE", "PDF_ENCRYPT_RC4_128", "PDF_ENCRYPT_AES_128",
    "PDF_ENCRYPT_AES_256",
], "encryption-method constants exposed in pdfspine + fitz")
# Module-level constant families — Task 1 (pdfspine.constants). Every real
# PyMuPDF 1.27 name in each family is implemented with the EXACT fitz value
# (cross-checked vs a real PyMuPDF 1.27 install; see test_longtail11.py).
add_many("constants", IMPLEMENTED, "M2", [
    "TEXT_flags", "TEXTFLAGS_bundles", "TEXT_FONT_flags",
], "fitz TEXT_*/TEXTFLAGS_*/TEXT_FONT_* flags, exact 1.27 values")
add_many("constants", IMPLEMENTED, "M4", [
    "TEXT_ALIGN", "PDF_ANNOT_types", "PDF_ANNOT_IS_flags", "PDF_ANNOT_LE",
    "PDF_WIDGET_TYPE", "PDF_WIDGET_TX_FORMAT", "PDF_FIELD_IS_flags",
    "PDF_BM_blendmodes", "PDF_REDACT_options", "STAMP_icons",
    "PDF_BORDER_STYLE", "PDF_SIGNATURE_flags",
], "fitz annot/widget/blend/redact/stamp/border/signature constants, exact 1.27 values")
add_many("constants", IMPLEMENTED, "M3", [
    "ENCRYPT_methods", "PERM_flags", "PDF_PAGE_LABEL",
], "fitz PDF_ENCRYPT_*/PDF_PERM_*/PDF_PAGE_LABEL_* constants, exact 1.27 values")
add_many("constants", IMPLEMENTED, "M5", ["CS_colorspace"],
         "fitz CS_RGB/CS_GRAY/CS_CMYK, exact 1.27 values")
add_many("constants", IMPLEMENTED, "M1", ["version_info", "PDF_TOK_objects"],
         "fitz version/VersionBind/VersionFitz tuple + PDF_TOK_* token constants")
# Implemented exceptions (exposed today)
add_many("exceptions", IMPLEMENTED, "M1", [
    "PdfUnsupportedError", "PdfDecodeError", "PdfRedactionError", "PdfError",
    "PdfSyntaxError", "PdfPasswordError", "PdfLimitError", "FileDataError",
    "EmptyFileError", "FileNotFoundError",
], "pdfspine-typed hierarchy + PyMuPDF exception-name aliases")
add("constants.UCDN_SCRIPT", "constants", OUT_OF_SCOPE, "M6", "full shaping out of scope (PRD §3.2 #10)")
add("constants.PdfUnsupportedError_catchall", "constants", OUT_OF_SCOPE, "M1",
    "every unlisted PyMuPDF symbol raises PdfUnsupportedError (PRD §7 catch-all + §17.2)")


# ---------------------------------------------------------------------------
# Drift reconciliation — long-tail batches 3 & 4 (commits 308db11, ec98835)
# ---------------------------------------------------------------------------
# Those batches implemented this surface (Colorspace / Font / Link / Outline /
# TextWriter / Tools / xref-write / text-trace / Page text helpers) and updated
# COMPAT.toml directly, but did NOT update this generator, so the add_many()
# blocks above still mark them deferred. Re-sync here so a regeneration
# reproduces the committed coverage instead of regressing ~63.7% -> ~53.7%.
# Verified present + non-stub in python/ at reconciliation time.
_BATCH34_IMPLEMENTED = {
    "Annot.get_text", "Annot.get_textpage", "Annot.next", "Colorspace.Colorspace",
    "Colorspace.csCMYK", "Colorspace.csGRAY", "Colorspace.csRGB", "Colorspace.n",
    "Colorspace.name", "Document.del_xml_metadata", "Document.subset_fonts", "Document.xref_copy",
    "Document.xref_is_font", "Document.xref_is_image", "Document.xref_set_key", "Font.Font",
    "Font.ascender", "Font.bbox", "Font.char_lengths", "Font.descender",
    "Font.flags", "Font.glyph_advance", "Font.glyph_count", "Font.glyph_name_to_unicode",
    "Font.has_glyph", "Font.is_bold", "Font.is_italic", "Font.is_monospaced",
    "Font.is_serif", "Font.name", "Font.text_length", "Font.unicode_to_glyph_name",
    "Font.Base14_fontnames", "Font.is_writable",
    "Font.valid_codepoints",
    # Font.buffer and Font.glyph_bbox: IMPLEMENTED via the program-backed handle
    # (P4-1). Font(fontfile=)/Font(fontbuffer=) load the REAL /FontFile* program
    # (no silent Helvetica fallback), so buffer returns the program bytes and
    # glyph_bbox returns the real per-glyph outline box; valid_codepoints reflects
    # the program's true cmap. A metrics-only Core-14 handle (built from a name,
    # no program) still raises PdfUnsupportedError for these two, since shipping
    # empty bytes / a constant font-level bbox would be misleading.
    "Font.buffer", "Font.glyph_bbox",
    "Link.border", "Link.colors", "Link.dest", "Link.flags",
    "Link.is_external", "Link.linkDest", "Link.next", "Link.page",
    "Link.rect", "Link.set_border", "Link.set_colors", "Link.set_flags",
    "Link.uri", "Link.xref", "Outline.dest", "Outline.destination",
    "Outline.down", "Outline.is_external", "Outline.is_open", "Outline.next",
    "Outline.page", "Outline.title", "Outline.uri", "Outline.x",
    "Outline.y", "Page.clean_contents", "Page.delete_image", "Page.get_bboxlog",
    "Page.get_text_blocks", "Page.get_text_selection", "Page.get_text_words", "Page.get_textbox",
    "Page.get_texttrace", "Page.replace_image", "Page.wrap_contents", "TOOLS",
    "TextPage.extractRAWJSON", "TextPage.extractHTML", "TextPage.extractXHTML",
    "TextPage.extractXML", "TextPage.extractIMGINFO", "TextPage.extractSelection",
    "TextPage.extractTextbox", "TextPage.search", "TextPage.poolsize",
    "TextWriter.TextWriter", "TextWriter.append", "TextWriter.appendv",
    "TextWriter.clean_rtl", "TextWriter.color", "TextWriter.fill_textbox", "TextWriter.last_point",
    "TextWriter.opacity", "TextWriter.text_rect", "TextWriter.write_text", "Tools",
    "Tools.fitz_config", "Tools.gen_id", "Tools.glyph_cache_empty", "Tools.mupdf_display_errors",
    "Tools.mupdf_display_warnings", "Tools.mupdf_version", "Tools.mupdf_warnings", "Tools.reset_mupdf_warnings",
    "Tools.set_small_glyph_heights", "Tools.store_maxsize", "Tools.store_shrink", "Tools.store_size",
    # Live + non-stub in python/pdfspine/document.py but historically marked
    # deferred: Page.links/first_link (link iteration), Document.outline (TOC
    # tree), Document.extract_image (image XObject -> dict). Verified present
    # and working on sample PDFs at reconciliation time.
    "Page.links", "Page.first_link", "Document.outline", "Document.extract_image",
}


def _reconcile_batch34() -> None:
    """Override the deferred disposition of the batch-3/4 symbols to implemented."""
    for i, (sym, grp, _disp, ms, note) in enumerate(CATALOG):
        if sym in _BATCH34_IMPLEMENTED:
            CATALOG[i] = (sym, grp, IMPLEMENTED, ms, note)


_reconcile_batch34()


# ---------------------------------------------------------------------------
# Emitters
# ---------------------------------------------------------------------------
def _toml_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def render_baseline() -> str:
    """The flat, sorted, checked-in baseline symbol list (one per line)."""
    syms = sorted({e[0] for e in CATALOG})
    header = [
        "# PyMuPDF compatibility baseline — pinned symbol snapshot.",
        f"# Baseline: PyMuPDF {BASELINE} (1.24.14 / MuPDF 1.24.11).",
        "#",
        "# This is the checked-in public-API symbol set the compat-symbol-guard",
        "# diffs COMPAT.toml against (real PyMuPDF is NOT installed at CI time).",
        "# Generated from scripts/_compat_catalog.py — do not edit by hand.",
        "# One symbol per line: 'Class.member', 'Class', or a module-level name.",
        "",
    ]
    return "\n".join(header) + "\n".join(syms) + "\n"


def render_deferred_py() -> str:
    """A generated ``pdfspine`` module exposing the deferred-symbol set.

    The runtime wrappers (``Page``/``Document``) route ``__getattr__`` through
    this set so that *every* deferred baseline symbol raises
    ``PdfUnsupportedError`` (never a bare ``AttributeError``), per the §7
    fitz-migration contract. It is derived from the same catalog as
    ``COMPAT.toml`` (one source of truth) and ships inside the wheel, where
    ``COMPAT.toml`` is not available at import time.
    """
    deferred = sorted(sym for sym, _g, disp, _m, _n in CATALOG if disp == DEFERRED)
    lines = [
        '"""Generated deferred-symbol set — do not edit by hand.',
        "",
        "Regenerate with ``python3 scripts/_compat_catalog.py`` (derived from the",
        "same catalog as COMPAT.toml). One entry per deferred baseline symbol,",
        "spelled ``Class.member`` (or a bare module-level name).",
        '"""',
        "",
        "from __future__ import annotations",
        "",
        "DEFERRED: frozenset[str] = frozenset(",
        "    {",
    ]
    lines.extend(f'        "{_toml_escape(sym)}",' for sym in deferred)
    lines.append("    }")
    lines.append(")")
    lines.append("")
    return "\n".join(lines)


def render_toml() -> str:
    counts = {IMPLEMENTED: 0, DEFERRED: 0, OUT_OF_SCOPE: 0}
    for e in CATALOG:
        counts[e[2]] += 1
    total = len(CATALOG)
    cov = 100.0 * counts[IMPLEMENTED] / total if total else 0.0

    lines: list[str] = []
    lines.append("# COMPAT.toml — pdfspine ↔ PyMuPDF compatibility map")
    lines.append("#")
    lines.append("# WHAT THIS IS")
    lines.append("#   The machine-readable disposition matrix for every public symbol in the")
    lines.append("#   pinned PyMuPDF baseline (PRD §7 / §9.5). Every PyMuPDF symbol not present")
    lines.append("#   here is out-of-scope by default and must raise PdfUnsupportedError (never")
    lines.append("#   AttributeError). The compat-symbol-guard fails CI if any baseline symbol")
    lines.append("#   (compat/compat-baseline.txt) is missing an entry below — forcing an")
    lines.append("#   explicit disposition for any newly-surfaced API (baseline evolution, §17.2).")
    lines.append("#")
    lines.append(f"#   Pinned baseline: PyMuPDF {BASELINE} (1.24.14 / MuPDF 1.24.11).")
    lines.append("#")
    lines.append("# FORMAT")
    lines.append("#   [[symbol]]")
    lines.append('#   name        = "Class.member" | "Class" | "module_level_name"')
    lines.append('#   group       = owning class / section (for grouping)')
    lines.append('#   disposition = "implemented" | "deferred" | "out-of-scope"')
    lines.append('#   milestone   = "M0".."M6" | "post-v1" | "out-of-scope" (optional)')
    lines.append('#   note        = short rationale (optional)')
    lines.append("#")
    lines.append("#   disposition meanings:")
    lines.append("#     implemented  — present in python/ and does NOT raise PdfUnsupportedError")
    lines.append("#     deferred     — known, planned for a later milestone (M3–M6 / post-v1)")
    lines.append("#     out-of-scope — intentionally never in v1; raises PdfUnsupportedError")
    lines.append("#")
    lines.append("# Generated from scripts/_compat_catalog.py — regenerate, do not hand-edit.")
    lines.append("")
    lines.append("[meta]")
    lines.append(f'baseline = "{BASELINE}"')
    lines.append('baseline_detail = "PyMuPDF 1.24.14 / MuPDF 1.24.11"')
    lines.append(f"total = {total}")
    lines.append(f"implemented = {counts[IMPLEMENTED]}")
    lines.append(f"deferred = {counts[DEFERRED]}")
    lines.append(f'out_of_scope = {counts[OUT_OF_SCOPE]}')
    lines.append(f"coverage_pct = {cov:.1f}")
    lines.append("")

    # Group order: keep declaration order of first appearance.
    group_order: list[str] = []
    for e in CATALOG:
        if e[1] not in group_order:
            group_order.append(e[1])

    seen: set[str] = set()
    for grp in group_order:
        lines.append(f"# ── {grp} ──")
        for sym, group, disp, milestone, note in CATALOG:
            if group != grp:
                continue
            if sym in seen:
                # guard against accidental duplicate symbol declarations
                raise ValueError(f"duplicate symbol in catalog: {sym}")
            seen.add(sym)
            lines.append("[[symbol]]")
            lines.append(f'name = "{_toml_escape(sym)}"')
            lines.append(f'group = "{_toml_escape(group)}"')
            lines.append(f'disposition = "{disp}"')
            if milestone:
                lines.append(f'milestone = "{_toml_escape(milestone)}"')
            if note:
                lines.append(f'note = "{_toml_escape(note)}"')
            lines.append("")
    return "\n".join(lines)


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    compat_toml = repo_root / "COMPAT.toml"
    baseline_dir = repo_root / "compat"
    baseline_dir.mkdir(exist_ok=True)
    baseline_txt = baseline_dir / "compat-baseline.txt"
    deferred_py = repo_root / "python" / "pdfspine" / "_compat_deferred.py"

    # Integrity: no duplicate symbols.
    syms = [e[0] for e in CATALOG]
    dupes = {s for s in syms if syms.count(s) > 1}
    if dupes:
        raise SystemExit(f"duplicate symbols in catalog: {sorted(dupes)}")

    compat_toml.write_text(render_toml(), encoding="utf-8")
    baseline_txt.write_text(render_baseline(), encoding="utf-8")
    deferred_py.write_text(render_deferred_py(), encoding="utf-8")

    counts = {IMPLEMENTED: 0, DEFERRED: 0, OUT_OF_SCOPE: 0}
    for e in CATALOG:
        counts[e[2]] += 1
    total = len(CATALOG)
    n_deferred = sum(1 for e in CATALOG if e[2] == DEFERRED)
    print(f"wrote {compat_toml} ({total} symbols)")
    print(f"wrote {baseline_txt} ({len({e[0] for e in CATALOG})} unique symbols)")
    print(f"wrote {deferred_py} ({n_deferred} deferred symbols)")
    print(
        f"  implemented={counts[IMPLEMENTED]} deferred={counts[DEFERRED]} "
        f"out-of-scope={counts[OUT_OF_SCOPE]} "
        f"coverage={100.0 * counts[IMPLEMENTED] / total:.1f}%"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
