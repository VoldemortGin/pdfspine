# Pinned source dependency

`tiny-skia/` is the complete official tiny-skia 0.11.4 source archive, with one
reviewed partial-mask-coverage optimization and a manifest-only exclusion for
Cargo’s reserved source metadata (still retained in the root sdist). See
[ADR 0003](../docs/adr/0003-vendored-mask-coverage.md) and
[tiny-skia.provenance.json](tiny-skia.provenance.json) for the exact source,
license, checksums and audit boundary.

Verify the checkout without network access:

```sh
python scripts/check_vendored_sources.py
# After building a source distribution:
python scripts/check_vendored_sources.py --sdist /path/to/pdfspine-0.8.0.tar.gz
```

To additionally authenticate an already downloaded upstream archive:

```sh
python scripts/check_vendored_sources.py --upstream-archive /path/to/tiny-skia-0.11.4.crate
```

To reproduce the source delta, unpack the pinned official archive in a temporary
directory and run this from its `tiny-skia-0.11.4` directory:

```sh
git apply /path/to/pdfspine/vendor/patches/tiny-skia-0.11.4-mask-coverage.patch
```

The archive's SHA-256 must first match the provenance record. The patched
inventory must then match all current hashes in that record. Do not regenerate
the manifest to conceal an unreviewed source change.
