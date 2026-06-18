# Document

`pdfspine.Document` (PyMuPDF `fitz.Document`) is a parsed PDF. Obtain one with
`pdfspine.open(...)`. It is a context manager, iterable, indexable, and
`len(doc)` is the page count.

## Opening

### `open(filename=None, *, stream=None, filetype=None) -> Document`

Module-level function. Pass a path positionally, in-memory bytes via `stream=`,
or no arguments to create a new empty PDF.

```python
doc = pdfspine.open("input.pdf")
doc = pdfspine.open(stream=raw_bytes)
doc = pdfspine.open()                      # new empty PDF
```

## Pages

| Member | Returns | Description |
|---|---|---|
| `page_count` | `int` | Number of pages. |
| `len(doc)` | `int` | Same as `page_count`. |
| `load_page(index=0)` | `Page` | Load the page at zero-based `index` (negative ok). |
| `doc[index]` | `Page` | Indexing; same as `load_page`. |
| `for page in doc` | `Page` | Iterate all pages. |
| `new_page(pno=-1, width=595.0, height=842.0)` | `Page` | Insert a blank page (`-1` appends). |
| `delete_page(pno=-1)` | `None` | Delete a page. |
| `select(pages)` | `None` | Keep only `pages`, in the given order. |
| `insert_pdf(docsrc, from_page=None, to_page=None, start_at=None)` | `None` | Insert pages from another document. |

## Document facts

| Member | Returns | Description |
|---|---|---|
| `is_pdf` | `bool` | Whether the document is a PDF. |
| `is_repaired` | `bool` | Whether the parser had to repair it. |
| `is_encrypted` | `bool` | Whether it is encrypted. |
| `needs_pass` | `bool` | Whether a password is required to open. |
| `permissions` | `int` | The permission flags bitfield. |
| `is_form_pdf` | `bool` | Whether it has an AcroForm. |
| `authenticate(password)` | `bool` | Supply a password (str/bytes); `True` on success. |
| `metadata` | `dict[str, str]` | Document metadata (PyMuPDF keys). |

## Text

| Member | Returns | Description |
|---|---|---|
| `get_page_text(pno, option="text", *, flags=None, sort=False)` | varies | Extract text from page `pno`. |

See [Page.get_text](page.md#text-extraction) for the `option` values.

## Save

| Member | Returns | Description |
|---|---|---|
| `save(filename, *, garbage=0, deflate=False, incremental=False, encryption=None, owner_pw=None, user_pw=None, permissions=-1)` | `None` | Write the document. |
| `ez_save(filename, **kwargs)` | `None` | `save` with `garbage=3, deflate=True`. |
| `tobytes(*, garbage=0, deflate=False, incremental=False, encryption=None, owner_pw=None, user_pw=None, permissions=-1)` | `bytes` | Serialize to bytes (alias: `write`). |
| `saveIncr(filename)` | `None` | Incremental save (deprecated PyMuPDF alias). |
| `get_page_pixmap(pno, **kwargs)` | `Pixmap` | Render page `pno` (kwargs as `Page.get_pixmap`). |

`garbage` ranges 0–4; `encryption` is one of the `PDF_ENCRYPT_*` constants.

## Metadata & XMP

| Member | Returns | Description |
|---|---|---|
| `set_metadata(metadata)` | `None` | Write the `/Info` dict. |
| `get_xml_metadata()` | `str` | The catalog XMP stream. |
| `set_xml_metadata(xml)` | `None` | Set the catalog XMP stream. |

## Table of contents

| Member | Returns | Description |
|---|---|---|
| `get_toc(simple=True)` | `list[list]` | `[[level, title, page], ...]`. |
| `set_toc(toc)` | `None` | Build the `/Outlines` tree. |

## Forms

| Member | Returns | Description |
|---|---|---|
| `form_field_names()` | `list[str]` | Fully-qualified field names. |
| `form_fill(name, value)` | `None` | Set a field's value. |
| `form_flatten()` | `None` | Bake all fields into page content. |

## Embedded files

| Member | Returns | Description |
|---|---|---|
| `embfile_add(name, buffer, filename=None, ufilename=None, desc=None)` | `None` | Embed a file. |
| `embfile_get(name)` | `bytes` | Read an embedded file. |
| `embfile_del(name)` | `None` | Delete an embedded file. |
| `embfile_names()` | `list[str]` | All embedded-file names. |
| `embfile_count()` | `int` | Count of embedded files. |
| `embfile_info(name)` | `dict` | Metadata of an embedded file. |

## Sanitize & bake

| Member | Returns | Description |
|---|---|---|
| `scrub(**toggles)` | `None` | Remove sensitive content (subset of PyMuPDF toggles acted on). |
| `bake(*, annots=True, widgets=True)` | `None` | Bake annotations/widgets into content. |

## Optional content / layers

| Member | Returns | Description |
|---|---|---|
| `get_ocgs()` | `dict[int, dict]` | OCGs keyed by xref. |
| `add_ocg(name, config=None, *, on=True, intent="View", usage=None)` | `int` | Add an OCG; returns its xref. |
| `get_layer(config=-1)` | `dict` | `{"on": [...], "off": [...], "locked": [...]}`. |
| `set_layer(config=-1, *, on=None, off=None, locked=None)` | `None` | Bulk-set layer visibility. |
| `layer_ui_configs()` | `list[dict]` | Layer-panel UI rows. |
| `ocg_state(xref)` | `bool` | Whether an OCG is ON. |
| `set_oc(xref, ocg)` | `None` | Bind an object to an OCG. |

## Low-level xref read API

| Member | Returns | Description |
|---|---|---|
| `xref_length()` | `int` | Size of the xref table. |
| `xref_object(xref)` | `str` | Object source for `xref`. |
| `xref_get_key(xref, key)` | varies | A dictionary key from `xref`. |
| `xref_is_stream(xref)` | `bool` | Whether the object is a stream. |
| `xref_stream(xref)` | `bytes` | Decoded stream bytes. |
| `extract_image(xref)` | `dict` | Image XObject as `ext`/`width`/`height`/`image` bytes/… |

## Lifecycle

| Member | Description |
|---|---|
| `close()` | Release the document (drops the Rust handle). |
| `with pdfspine.open(...) as doc:` | Context-manager usage; auto-closes. |

## Not yet implemented

`convert_to_pdf` raises `PdfUnsupportedError` (image-document inputs are planned
for milestone M5). Many low-level write helpers and page-label methods are
deferred — see `COMPAT.toml`.
