# Rendering

pdfspine rasterizes any page to a `Pixmap` — text, vector fills and strokes,
images, clips, and axial/radial shadings. It can also record a `DisplayList` for
repeated rendering and export a page to SVG.

## `get_pixmap`

`Page.get_pixmap(*, matrix=None, dpi=None, colorspace=None, alpha=False, clip=None)`
renders the page and returns a [`Pixmap`](#pixmap):

```python
import pdfspine

doc = pdfspine.open("input.pdf")
page = doc[0]

# Resolution via DPI:
pix = page.get_pixmap(dpi=150)

# Resolution via a Matrix (2x scale ≈ 144 DPI):
pix = page.get_pixmap(matrix=pdfspine.Matrix(2, 2))

# Grayscale with alpha:
pix = page.get_pixmap(colorspace="gray", alpha=True)

# Render only a sub-rectangle (device space):
pix = page.get_pixmap(dpi=150, clip=pdfspine.Rect(0, 0, 300, 400))
```

Parameters:

- `matrix` — a `Matrix` (or 6-sequence) transform; mutually informative with `dpi`.
- `dpi` — target resolution in dots per inch.
- `colorspace` — `"gray"`, `"rgb"`, or `"cmyk"` (or a component count).
- `alpha` — add an alpha channel.
- `clip` — a device-space sub-rectangle to render.

There is also a document-level convenience, `doc.get_page_pixmap(pno, **kwargs)`.

## Pixmap

A `Pixmap` is the native raster buffer. Key members:

| Member | Type | Description |
|---|---|---|
| `width` / `w` | `int` | Pixel width. |
| `height` / `h` | `int` | Pixel height. |
| `n` | `int` | Components per pixel (including alpha). |
| `alpha` | `bool` | Whether the last component is alpha. |
| `stride` | `int` | Bytes per row. |
| `irect` | `tuple` | `(x0, y0, x1, y1)` bounds at the origin. |
| `colorspace` | `str` | `"DeviceGray"` / `"DeviceRGB"` / `"DeviceCMYK"`. |
| `samples` | `bytes` | Owning copy of the raw pixel bytes. |
| `samples_mv` | `memoryview` | Zero-copy view of the pixels. |
| `size` | `int` | `len(samples)`. |

### Save & encode

```python
pix.save("page.png")            # format inferred from the extension
pix.save("page.pam", "pam")     # explicit format

png_bytes = pix.tobytes("png")  # "png" (default), "pam", or "ppm"/"pnm"
```

### Per-pixel access & mutation

```python
r, g, b = pix.pixel(0, 0)       # tuple of n component ints
pix.set_pixel(0, 0, [255, 0, 0])
pix.set_alpha(128)              # set every alpha byte
pix.clear_with(0)              # fill the whole buffer
pix.invert_irect()             # invert colors (optionally over an irect)
```

### Buffer protocol (zero-copy)

`Pixmap` implements the Python buffer protocol, so you can hand the pixels to
NumPy / Pillow without copying:

```python
import numpy as np

pix = page.get_pixmap(dpi=150)
arr = np.frombuffer(pix, dtype=np.uint8)        # zero-copy
arr = arr.reshape(pix.height, pix.stride // pix.n if pix.n else pix.width, pix.n)

mv = memoryview(pix)                            # also zero-copy
```

!!! note "Copy-on-write lifetime contract"
    While a buffer view (`memoryview(pix)` / a NumPy array over it) is alive, the
    pixel bytes are kept alive past the `Pixmap`'s own lifetime, and any in-place
    mutator (`set_pixel`, `clear_with`, …) copies-on-write rather than touching
    the bytes the view points at. You can never observe a mutate-under-view or a
    use-after-free.

### Constructing a blank Pixmap

```python
# Pixmap(colorspace, irect, alpha=False)
# colorspace is a component count (1/3/4) or a name string.
pix = pdfspine.Pixmap(3, (0, 0, 200, 100))      # blank RGB 200x100
pix = pdfspine.Pixmap("DeviceGray", (0, 0, 64, 64), alpha=True)
```

## DisplayList

A `DisplayList` records a page's drawcalls once so you can replay them at several
resolutions without re-interpreting the content stream:

```python
dl = page.get_displaylist()
print(dl.rect, len(dl))            # source rect, number of drawcalls

thumb = dl.get_pixmap(dpi=36)      # same kwargs as Page.get_pixmap
full = dl.get_pixmap(dpi=300)
```

## SVG export

```python
svg = page.get_svg_image()                       # standalone SVG document string
svg = page.get_svg_image(matrix=pdfspine.Matrix(2, 2))

with open("page.svg", "w", encoding="utf-8") as fh:
    fh.write(svg)
```

## Image documents & extraction

Embedded raster images can be pulled out of a document by xref:

```python
img = doc.extract_image(xref)     # dict: ext, colorspace, bpc, width, height,
                                  #       n, smask, image (bytes)
with open(f"image.{img['ext']}", "wb") as fh:
    fh.write(img["image"])
```

!!! info "Opening images as documents"
    PyMuPDF's `convert_to_pdf` (treating an image file as a one-page document) is
    on the roadmap (milestone M5) and currently raises `PdfUnsupportedError`.
