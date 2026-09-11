#!/usr/bin/env python3
"""Verify the complete pinned tiny-skia source inventory and reviewed patch."""

import argparse
import hashlib
import json
from pathlib import Path
import tarfile


ROOT = Path(__file__).resolve().parents[1]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def verify(root: Path, archive: Path | None = None) -> int:
    vendor = root / "vendor"
    source = vendor / "tiny-skia"
    manifest = json.loads((vendor / "tiny-skia.provenance.json").read_text())
    expected = manifest["files"]
    actual = {
        p.relative_to(source).as_posix() for p in source.rglob("*") if p.is_file()
    }
    if actual != set(expected):
        raise ValueError("vendored source inventory differs from the pinned release")
    modified = []
    for name, entry in expected.items():
        path = source / name
        if path.is_symlink() or sha256(path.read_bytes()) != entry["sha256"]:
            raise ValueError(f"vendored source checksum mismatch: {name}")
        if entry["sha256"] != entry["upstream_sha256"]:
            modified.append(name)
    if sorted(modified) != sorted(manifest["modified_files"]):
        raise ValueError("vendored modification list differs from provenance")
    if sha256((vendor / manifest["patch"]).read_bytes()) != manifest["patch_sha256"]:
        raise ValueError("vendored patch checksum mismatch")
    if archive is not None:
        if sha256(archive.read_bytes()) != manifest["archive_sha256"]:
            raise ValueError("upstream archive checksum mismatch")
        prefix = f"{manifest['package']}-{manifest['version']}/"
        with tarfile.open(archive) as tar:
            upstream = {}
            for member in tar.getmembers():
                if not member.isfile():
                    continue
                if not member.name.startswith(prefix):
                    raise ValueError("unexpected upstream archive prefix")
                file = tar.extractfile(member)
                if file is None:
                    raise ValueError("unreadable upstream archive member")
                upstream[member.name[len(prefix) :]] = sha256(file.read())
        if upstream != {
            name: entry["upstream_sha256"] for name, entry in expected.items()
        }:
            raise ValueError("upstream archive inventory differs from provenance")
    return len(expected)


def verify_sdist(root: Path, archive: Path) -> int:
    """Check actual tar members, including files excluded from Cargo's list."""
    manifest_bytes = (root / "vendor/tiny-skia.provenance.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    with tarfile.open(archive) as tar:
        members = [member for member in tar.getmembers() if member.isfile()]
        names = [member.name for member in members]
        if len(names) != len(set(names)):
            raise ValueError("duplicate sdist file members")
        manifests = [
            name for name in names if name.endswith("/vendor/tiny-skia.provenance.json")
        ]
        if len(manifests) != 1:
            raise ValueError("sdist must contain the vendor provenance record")
        prefix = manifests[0].removesuffix("vendor/tiny-skia.provenance.json")

        def read(name: str) -> bytes:
            file = tar.extractfile(name)
            if file is None:
                raise ValueError(f"unreadable sdist member: {name}")
            return file.read()

        if read(manifests[0]) != manifest_bytes:
            raise ValueError("sdist vendor provenance differs from the checkout")
        source_prefix = prefix + "vendor/tiny-skia/"
        inventory = {
            name[len(source_prefix) :]: sha256(read(name))
            for name in names
            if name.startswith(source_prefix)
        }
        if inventory != {
            name: entry["sha256"] for name, entry in manifest["files"].items()
        }:
            raise ValueError("sdist vendor inventory or checksums differ")
        if (
            sha256(read(prefix + "vendor/" + manifest["patch"]))
            != manifest["patch_sha256"]
        ):
            raise ValueError("sdist vendor patch checksum mismatch")
    return len(inventory)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream-archive", type=Path)
    parser.add_argument("--sdist", type=Path)
    args = parser.parse_args()
    count = verify(ROOT, args.upstream_archive)
    print(f"Verified {count} tiny-skia source files and the pinned patch")
    if args.sdist is not None:
        count = verify_sdist(ROOT, args.sdist)
        print(f"Verified all {count} source files, provenance and patch in the sdist")


if __name__ == "__main__":
    main()
