# Pixmap & DisplayList

## Pixmap

`oxide_pdf.Pixmap` (PyMuPDF `fitz.Pixmap`) is a native raster buffer. Obtain one
from `Page.get_pixmap(...)`, `DisplayList.get_pixmap(...)`, or construct a blank
one directly.

### Construction

```python
# Pixmap(colorspace, irect, alpha=False)
#   colorspace: component count (1=gray, 3=rgb, 4=cmyk) or a name string
#   irect:      (x0, y0, x1, y1) bounds
pix = oxide_pdf.Pixmap(3, (0, 0, 200, 100))
pix = oxide_pdf.Pixmap("DeviceGray", (0, 0, 64, 64), alpha=True)
```

### Properties

| Member | Type | Description |
|---|---|---|
| `width` / `w` | `int` | Pixel width. |
| `height` / `h` | `int` | Pixel height. |
| `n` | `int` | Components per pixel (incl. alpha). |
| `alpha` | `bool` | Whether the last component is alpha. |
| `stride` | `int` | Bytes per row. |
| `irect` | `tuple` | `(x0, y0, x1, y1)` at the origin. |
| `colorspace` | `str` | `"DeviceGray"` / `"DeviceRGB"` / `"DeviceCMYK"`. |
| `samples` | `bytes` | Owning copy of the raw pixel bytes. |
| `samples_mv` | `memoryview` | Zero-copy view. |
| `size` | `int` | `len(samples)`. |

### Methods

| Member | Returns | Description |
|---|---|---|
| `save(filename, output=None)` | `None` | Save; format from `output` or the extension (PNG default). |
| `tobytes(output="png")` | `bytes` | Encode to `"png"` / `"pam"` / `"ppm"` (`"pnm"`). |
| `pixel(x, y)` | `tuple[int, ...]` | The `n` component values at `(x, y)`. |
| `set_pixel(x, y, value)` | `None` | Write a pixel from `n` component bytes. |
| `set_alpha(value)` | `None` | Set every alpha byte. |
| `clear_with(value=0)` | `None` | Fill the whole buffer. |
| `invert_irect(irect=None)` | `None` | Invert colors (whole pixmap if no `irect`). |
| `len(pix)` | `int` | `len(samples)`. |

### Buffer protocol

`Pixmap` implements the Python buffer protocol for zero-copy interop:

```python
import numpy as np
arr = np.frombuffer(pix, dtype=np.uint8)   # zero-copy
mv = memoryview(pix)                        # zero-copy
```

While a view is alive, the bytes are kept alive past the `Pixmap`'s lifetime and
in-place mutators copy-on-write, so a view can never observe a mutate-under-view
or use-after-free.

## DisplayList

`oxide_pdf.DisplayList` (PyMuPDF `fitz.DisplayList`) is a recorded, replayable
page render. Obtain one from `Page.get_displaylist()`.

| Member | Returns | Description |
|---|---|---|
| `rect` | `tuple` | The source rect (page CropBox) as `(x0, y0, x1, y1)`. |
| `len(dl)` | `int` | Number of recorded drawcalls. |
| `get_pixmap(*, matrix=None, dpi=None, colorspace=None, alpha=False, clip=None)` | `Pixmap` | Replay into a `Pixmap` (same kwargs as `Page.get_pixmap`). |

```python
dl = page.get_displaylist()
thumb = dl.get_pixmap(dpi=36)
full = dl.get_pixmap(dpi=300)
```
