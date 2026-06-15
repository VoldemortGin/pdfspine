# Editing & saving

oxide-pdf supports a broad slice of PyMuPDF's editing surface: page operations
and merging, content and vector insertion, annotations, AcroForm forms,
redaction, metadata, table-of-contents, links, embedded files, and full or
incremental saving.

## Page operations & merging

```python
import oxide_pdf

doc = oxide_pdf.open("input.pdf")

# Insert a blank page (pno=-1 appends; returns the new Page):
page = doc.new_page(width=595, height=842)   # A4 in points

# Delete a page:
doc.delete_page(0)                            # negative index supported

# Keep only selected pages, in the given order (reorder / subset):
doc.select([2, 0, 1])
```

### Merging two PDFs

```python
dst = oxide_pdf.open("a.pdf")
src = oxide_pdf.open("b.pdf")

# Append all of src after dst's pages:
dst.insert_pdf(src)

# Or a page range, at a specific position:
dst.insert_pdf(src, from_page=0, to_page=4, start_at=0)

dst.save("merged.pdf")
```

### Splitting

To split, build new documents and copy ranges in with `insert_pdf`:

```python
src = oxide_pdf.open("input.pdf")

first_half = oxide_pdf.open()                 # empty PDF
first_half.insert_pdf(src, from_page=0, to_page=4)
first_half.save("part1.pdf")
```

## Saving

```python
# Full save. garbage: 0-4 (object GC); deflate: compress streams.
doc.save("output.pdf", garbage=3, deflate=True)

# Preset that turns on GC + deflate:
doc.ez_save("output.pdf")

# Incremental save: append a new revision to the existing file (fast, preserves
# the original bytes — required for signed documents).
doc.save("output.pdf", incremental=True)

# Serialize to bytes instead of a file (write is an alias of tobytes):
data = doc.tobytes(garbage=3, deflate=True)
```

### Encryption on save

```python
doc.save(
    "secure.pdf",
    encryption=oxide_pdf.PDF_ENCRYPT_AES_256,   # AES-256 (authored as R6)
    owner_pw="owner-secret",
    user_pw="open-sesame",
    permissions=-1,                              # all permissions
)
```

Available constants: `PDF_ENCRYPT_NONE`, `PDF_ENCRYPT_RC4_128`,
`PDF_ENCRYPT_AES_128`, `PDF_ENCRYPT_AES_256`.

To open an encrypted document, authenticate first:

```python
doc = oxide_pdf.open("secure.pdf")
if doc.needs_pass:
    doc.authenticate("open-sesame")
```

## Metadata

```python
meta = doc.metadata                     # dict with PyMuPDF keys
doc.set_metadata({"title": "Q3 Report", "author": "Finance"})

# XMP metadata (catalog stream):
xmp = doc.get_xml_metadata()
doc.set_xml_metadata(xmp)
```

## Table of contents

```python
toc = doc.get_toc()                     # [[level, title, page], ...]

doc.set_toc([
    [1, "Introduction", 1],
    [2, "Background", 2],
    [1, "Results", 5],
])
```

`set_toc` builds the `/Outlines` tree and raises on an illegal level jump.

## Links

```python
links = page.get_links()                # list of dicts (each "from" is a Rect)

# Add a URI link:
page.insert_link({
    "kind": 2,                          # 1 = goto, 2 = uri
    "from": oxide_pdf.Rect(72, 72, 200, 90),
    "uri": "https://example.com",
})

# Delete a link (by its dict, which carries an xref):
page.delete_link(links[0])
```

## Annotations

`Page` exposes the full annotation surface. Each `add_*` method returns an
`Annot`:

```python
# Markup over search hits:
quads = page.search_for("confidential", quads=True)
annot = page.add_highlight_annot(quads)
annot.set_colors(stroke=(1, 1, 0))
annot.update()

# Other markups:
page.add_underline_annot(quads)
page.add_strikeout_annot(quads)
page.add_squiggly_annot(quads)

# Shapes & notes:
page.add_rect_annot(oxide_pdf.Rect(72, 72, 200, 120), color=(1, 0, 0))
page.add_circle_annot(oxide_pdf.Rect(72, 72, 200, 120))
page.add_line_annot((72, 72), (200, 120))
page.add_text_annot((72, 72), "A sticky note", icon="Note")
page.add_freetext_annot(oxide_pdf.Rect(72, 72, 300, 120), "Free text")
page.add_stamp_annot(oxide_pdf.Rect(72, 72, 200, 120), stamp="Approved")
page.add_ink_annot([[(10, 10), (20, 30), (40, 20)]])

# Inspect & remove:
for a in page.annots():
    print(a.type, a.rect, a.info)
page.delete_annot(annot)
```

## Content & vector insertion

```python
# Text:
page.insert_text((72, 72), "Hello", fontname="helv", fontsize=12, color=(0, 0, 0))
page.insert_textbox(oxide_pdf.Rect(72, 72, 300, 200), "Wrapped paragraph...")

# Images (provide stream= bytes or filename=):
page.insert_image(oxide_pdf.Rect(72, 72, 200, 200), filename="logo.png")

# Vectors:
page.draw_line((72, 72), (200, 72), color=(0, 0, 0), width=1.0)
page.draw_rect(oxide_pdf.Rect(72, 90, 200, 140), color=(0, 0, 1), fill=(0.9, 0.9, 1))
page.draw_circle((140, 200), 40, color=(0, 0.5, 0))

# Or accumulate a reusable Shape, then commit once:
shape = page.new_shape()
shape.draw_line((10, 10), (100, 10))
shape.draw_rect(oxide_pdf.Rect(10, 20, 100, 60))
shape.finish(color=(0, 0, 0), width=2)
shape.commit()
```

## Forms (AcroForm)

```python
if doc.is_form_pdf:
    for name in doc.form_field_names():
        print(name)

    # Fill a field by name:
    doc.form_fill("customer_name", "Ada Lovelace")

    # Or work with widgets directly:
    for widget in page.widgets():
        print(widget.field_name, widget.field_type_string, widget.field_value)
        widget.field_value = "new value"
        widget.update()

    # Bake (flatten) all fields into static page content:
    doc.form_flatten()
```

## Redaction

Mark regions for redaction, then apply to permanently remove the content:

```python
# Mark a redaction over a search hit's rect:
for quad in page.search_for("SSN", quads=True):
    page.add_redact_annot(quad, fill=(0, 0, 0))

removed = page.apply_redactions()      # returns the number applied
doc.save("redacted.pdf")
```

## Sanitizing & baking

```python
# Remove sensitive content (metadata, JavaScript, attachments, links, XMP):
doc.scrub()

# Bake annotations and/or form widgets into page content:
doc.bake(annots=True, widgets=True)
```

## Embedded files

```python
doc.embfile_add("data.csv", b"a,b,c\n1,2,3\n", desc="sample")
print(doc.embfile_names(), doc.embfile_count())
blob = doc.embfile_get("data.csv")
info = doc.embfile_info("data.csv")
doc.embfile_del("data.csv")
```
