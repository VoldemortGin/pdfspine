# ADR 0003: bounded tiny-skia mask coverage specialization

- Status: Accepted for the unreleased implementation
- Date: 2026-09-11

## Context and decision

First-seen glyph masks repeatedly enter tiny-skia 0.11.4's general low-precision
RGBA pipeline to blend one byte of white mask coverage. Keep the complete
published crate in `vendor/tiny-skia` and specialize only partial runs where
`RasterPipelineBlitter::is_mask` is true. Transparent/opaque runs, color painting,
scan conversion, antialiasing, clipping and font/cache behavior remain unchanged.

The scalar expression uses u32 intermediates and exactly implements the existing
lowp integer interpolation, including nonzero destination coverage. Exhaustive
65,536-pair comparison against the actual upstream private lowp functions and
7,680 public Mask cases passed; 53 full-page/boundary outputs were byte-identical.
These external experimental checks precede the integration. Committed public
Mask regressions and the ordinary render suite guard the maintained dependency.

Use a direct relative workspace dependency, inherited by pdf-render, and exclude
the vendor from workspace membership. A root `[patch]` is insufficient because
Cargo ignores dependency workspaces' patch sections when another project consumes
pdf-api through a fixed git revision. The vendor travels in that same revision.
No external path or additional remote git dependency is required. The current
Rust crates remain `publish = false`; this does not introduce a crates.io release
strategy for pdf-api.

## Provenance and licensing

The official 0.11.4 archive contains 190 files / 691,235 bytes. It was downloaded
from static.crates.io and its SHA-256 was matched to the official crates.io sparse
index, not inferred from a directory name. The crates.io version API returned 403;
the archive and sparse-index requests succeeded.

`vendor/tiny-skia.provenance.json` records URLs, archive checksum and every
upstream/current file checksum. Exactly one upstream Rust source file differs; the manifest additionally excludes
Cargo’s reserved `.cargo_vcs_info.json` and `Cargo.toml.orig` from `cargo package --list`, which Maturin
uses for path dependencies. The root sdist include still retains that original
metadata, keeping the full 190-file inventory.
`vendor/patches/tiny-skia-0.11.4-mask-coverage.patch` reproduces that delta.
Retain the full BSD-3-Clause license, copyright headers, upstream metadata,
tests and examples. Experimental programs and generated experimental lockfiles
are not imported. `THIRD-PARTY-NOTICES.md` continues to provide binary attribution.

`check_vendored_sources.py` checks the complete inventory and patch checksum;
its optional archive argument verifies the official archive against every
recorded upstream hash. It runs in the Rust gate. Vendor files participate in
extension fingerprints and the sdist includes the vendor/provenance/patch, so
editing a vendor file cannot silently reuse an old installed extension.

## Supply-chain boundary

`policy.tiny-skia.audit-as-crates-io = true` retains upstream 0.11.4 coverage
requirements. Existing exemptions, audit criteria and publisher trust are
unchanged. Isolated positive/negative checks show that removing the upstream
exemption fails vet; the unchanged exemption does **not** certify this local
modification. The local delta is reviewed as part of this change, with its
provenance and correctness evidence. Do not describe it as independently audited
by the upstream exemption's authors.

## Evidence and limits

`conformance/BENCH.md` separates median document latency, median paired ratios
and sum-of-document latencies. Per-document ABBA and reverse-order BAAB repeat
roughly 10% median relative improvement on the 35 available samples. Fresh and
nonfresh Mask microbenchmarks and a text-rich page profile support the mechanism;
an image-only negative control is flat. All measurements are from one ARM64 Mac
with disclosed background application activity. This is not a universal or
cross-platform performance promise; larger rasterization and J2K work stays open.

An initial microbenchmark with shared Cargo target output was rejected when both
copied executables proved identical. Only the separately rebuilt, distinct-source
and distinct-binary run supports the microbenchmark results.

## Maintenance and validation

- Verify source hashes and the exact runtime/packaging delta before dependency updates.
- Preserve permissive license notices and the existing audit policy boundary.
- Run the ordinary full gate, an extracted-sdist wheel build outside the checkout,
  and a separate consumer that fetches an immutable local git revision with
  pdf-api default features disabled and exercises rendering.
- Prefer an equivalent upstream release when available; revalidate pixels and
  performance before removing this vendor. Sending an upstream PR or publishing
  anything is outside this local change.
