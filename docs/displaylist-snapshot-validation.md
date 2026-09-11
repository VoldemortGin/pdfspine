# DisplayList resource snapshots

A recording owns its resource revision before interpretation. Updating an image's
indirect soft mask, Indexed palette or ICC device-alternate metadata cannot change
an existing DisplayList raster. Font/image/Form objects already copied by the old
recorder remain stable; a new TextPage from the recording also keeps captured raw
font names after source edits/close while honoring the current subset-name view.

`DocumentStore::snapshot` shares immutable source bytes and xref tables, copies the
pending-change map with shared immutable object payloads, and captures trailer,
layer and authentication state under simultaneous read guards. All guards remain
held until capture completes. Writers currently hold only one of those state
locks; authentication releases its guard before clearing the source cache. A new
snapshot starts with an empty object arena/interner and generation zero. It is one
store capture point, not a transaction spanning a multi-step authoring operation.
Retained recordings keep their source and captured data alive.

The Rust factories `page_get_displaylist` and
`page_get_displaylist_with_annots` now return `Result<DisplayList>` to propagate
snapshot errors. Rust callers must add `?` or handle the result; Python signatures
are unchanged. This unreleased source change does not modify already published
fixed-revision consumers. `Page.run` and `DisplayList.run` remain deferred.

## Validation

The previous f3572f1 binary fails three targeted raster cases: indirect SMask,
Indexed palette and ICC device-alternate changes. Four existing direct-resource
cases were already stable. The candidate passes eight snapshot Python cases and
existing DisplayList/TextPage/annotation coverage (82 tests), plus 629 related
core/API tests, snapshot shared-pointer/authentication/poison/concurrency tests,
and clippy. The integrated subset-name cross test additionally combines a
post-close raw-name view change with the indirect-mask regression.

The precise f3572f1 baseline native SHA is
`b5de0c540736d01193a126a6dec7e7d4f97d6722407791a81d71a50e7be77201`.
The shared-xref candidate retains all 106 pixel records: 35 extant samples plus
18 CJK/rotation/clip/alpha/Type3 cases, each through Page and DisplayList, compared
by dimensions and full buffer SHA. No GT rescoring. ICC tests cover the existing
N/device-alternate implementation, not full color management.

## Cost observations

An isolated ABBA window compares shared-xref candidate 8ec2f23 with f3572f1, using
20 creations per process and 30 retained recordings. These are narrow cost
observations, not a broad performance confidence interval. MB is decimal.

| Case | Baseline create | Snapshot create | Snapshot retained RSS increase |
|---|---:|---:|---:|
| 10 extra xrefs | 2.02 µs | 3.25 µs | 0.22 MB |
| 100k xrefs, initially cold | 2.24 µs | 3.41 µs | 0.18 MB |
| 100k xrefs, source prewarmed | 1.91 µs | 3.46 µs | 0.20 MB |
| 100k xrefs + 100 pending edits | 2.09 µs | 3.79 µs | 0.30 MB |
| 100k xrefs + 10k pending edits | 1.83 µs | 30.65 µs | 8.67 MB |
| PMC212689 p0, initially cold | 1.136 ms | 1.188 ms | 182.04 MB (baseline 177.47 MB) |
| PMC212689 p0, source prewarmed | 1.121 ms | 1.265 ms | 177.10 MB (baseline 174.11 MB) |

Cold describes the initial source cache; the repeated median includes later calls.
Every snapshot still reparses into its own cache. Observed real-page repeat overhead
is about 5–13%, not zero; existing recording data dominates retained real-page RSS.
The rejected xref-copy prototype needed 40.77 µs and 96.21 MB on the clean 100k case.
Sharing the immutable index removes that structural copy, while overlay map copy
cost remains proportional to pending edits. No rendering speedup is claimed.

Raw evidence is in the external `pdfspine-deferred-plan` bundle:
`resource-snapshot-{red-final,arc-python,related-rust,arc-clippy}.log`,
`snapshot-pixels-{f357-baseline,arc-candidate}.json`, and `snapshot-arc-cost/`
(provenance, scripts, all timings and RSS samples). Final integrated five-phase gate passes on the vendored renderer and dynamic
subset-name view source: **1971 Rust / 1386 Python tests**, 66 existing Python
skips, extension fingerprint `53da3cbce469`, drift and wheel/sdist installation
smoke. The new raw-name/SMask cross test passes, and eight final Page/DL pixel
records across born, PMC, NASA and USGS inputs remain identical. Logs:
`snapshot-integrated-final-gate.log` and `snapshot-pixels-integrated-final.json`.
