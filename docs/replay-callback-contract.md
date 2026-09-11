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
Page.run rejects an already closed source document before recording or begin;
a previously captured DisplayList remains replayable after source closure.

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

## TextPage target adapter (slice 2)

`ReplayDevice.for_textpage(target, flags=0)` retains a public `TextPage` and
appends each successful run to that same object. Its original rect and existing
blocks are preserved; only the new segment is laid out. Flags must be an integer
fitting u32 and apply to new segments, not earlier content. Recorded targets keep
their creation flags; ordinary Page targets retain their existing legacy image
visibility when promoted, as with `Page.extend_textpage`. Image resources remain
owned and distinct across documents; dynamic subset-name presentation continues
to use the recorded raw font metadata.

The adapter uses the callback selector's exact operation identities. Area is
still conservative whole-operation selection: unknown text bounds may retain
text far outside the query. After selection, the existing target rectangle clips
transformed glyph origins independently. A partial area intersection never clips
a selected text run to the area. Source content CTM, CropBox/Rotate and caller
matrix are each applied once. Invisible `Tr3` and clipping `Tr7` runs retain their
recorded text semantics; hidden optional content contributes none. A semantic
image without a recorded operation cannot be selected by this replay path.

The target's frozen core model is replaced only after complete successful staging.
An unreadable old visible image fails promotion without partial append. A target
changed by another append during staging causes RuntimeError instead of lost
updates. Empty operation selection leaves the original core untouched. Closing
and concurrent reuse follow the callback device rules. The typed adapter invokes
no user callbacks; each run returns None. Both run catalog entries still remain
deferred pending the RGB/RGBA target adapter, and no native device ABI parity is
claimed.

Validation: a recording-only emit-time operation/range contract covers repeated
identical show operations and Tr3/Tr7 while retaining semantic font normalization.
Focused public tests compare complete RAWDICT results against B for Form/AP
recursion, hidden-content rollback, all four page rotations with CropBox and
caller shear, image flag combinations and source closure. Additional cases cover
area versus target clipping, dynamic subset names, duplicate append/search/block
numbering, failed promotion and a deterministic concurrent-commit interleaving.
