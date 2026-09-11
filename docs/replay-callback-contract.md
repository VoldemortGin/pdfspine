# Callback replay extension (slice 1)

`ReplayDevice(callback)` and immutable `ReplayEvent` are pdfspine extensions.
They are not MuPDF `FzDevice2`, `DeviceWrapper` or native-handle ABI substitutes.
`Page.run(dw,m)` and `DisplayList.run(dw,m,area)` return None. Both compatibility
catalog entries remain deferred until TextPage and RGB/RGBA target adapters also
meet their contracts; this slice provides callbacks only.

Each successful run emits begin, ordered operations, end. On callback failure the
original exception escapes, no end is sent, and existing user side effects remain.
A device may be reused after failure. Recursive/concurrent reuse and closing while
running raise RuntimeError. Otherwise close is idempotent and prevents later runs.
No destructor calls user code. Parameter/geometry validation precedes begin;
resource decoding is lazy and may raise separately when an accessor is used.

Events own the frozen recording. Payload mappings and nested tuples/bytes are
read-only; retained events remain usable after source edits/close. No document,
mutable PDF object or native pointer escapes. Paths and glyph origins/quads already
include the content CTM, in PDF user coordinates. Event.matrix contains only the
page CropBox/Rotate transform followed by the caller matrix. Image/shading CTM
must be composed before event.matrix. Never apply a text/path CTM a second time.
Event.bounds is a conservative bbox in final device coordinates, or None when
unknown. The operation vocabulary mirrors this engine's recording, not every
native-device callback. Resource IDs are recording-local operation indices.

Area is a coarse selection query in final device coordinates, not an added clip.
None is unbounded. Empty/inverted rectangles suppress paint but retain state and
lifecycle events: an explicit normalization of native edge cases. Save/restore
and clips are retained conservatively. Glyph advance cells cannot guarantee italic/Type3 ink bounds, so text paint
bounds also remain unknown (per-glyph cell geometry stays available in payload).
Stroke joins/miter bounds are not fully
recorded, so stroke bounds remain unknown and may cause extra callbacks. Shading
bounds are also unknown; finite shading coordinates alone do not bound extension.

Text payload glyph tuples contain (Unicode string, source code, glyph ID,
source origin, source quad, source text-rendering matrix). Font names are local
resource names, not stable identifiers across Forms. `font_buffer` lazily returns
PDF-filter-decoded original embedded FontFile/FontFile2/FontFile3 bytes without
converting their program format; `font_format` identifies Type1, TrueType or the
FontFile3 Subtype. No-file fonts return None, including built-in/Type3 fonts without
a program; malformed existing resources raise typed errors. `image_bytes` returns
the owned resolver's actual encoded bytes; `image_format` gives the actual extension (for example `png` or `jpg`)
(as produced by the resolver). Accessors on unrelated event kinds return None.

## Slice-1 validation

The 16 focused Python cases cover callback order/lifetime, source-close inside a
callback, immutable payloads, original exception identity, reuse after error,
recursive/concurrent device exclusion, close semantics, CTM + Rotate + caller
matrix, final-coordinate area selection, empty/inverted area, nested clip restore,
hairline/singular matrices, embedded TrueType bytes after source editing, lazy
malformed-font errors, and actual PNG/JPEG bytes after close. Three Rust unit
cases verify transformed curve-control hulls, NaN rejection before min/max can
hide it, and overflow from otherwise finite coordinates.

Related pdf-api/pdf-render/py-bindings tests: 252 passed. Combined callback,
deferred guards, DisplayList text/resource snapshots, extend_textpage and subset
policy tests: 103 passed, 2 empty-parameter skips. That final run includes the five
local oracle cases (an earlier isolated run reported 98 passed / 7 skipped before
pointing at the maintained oracle environment). Workspace/all-target/all-feature
clippy, Rust/Python formatting, Ruff and the 11 configured mypy stub files pass.
Both run catalog entries stay deferred; the guard explicitly checks callable
callback routes and rejected native targets during this temporary transition.
A final complete gate is reserved for the three-slice combined implementation.
