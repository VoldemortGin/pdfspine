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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream-archive", type=Path)
    args = parser.parse_args()
    count = verify(ROOT, args.upstream_archive)
    print(f"Verified {count} tiny-skia source files and the pinned patch")


if __name__ == "__main__":
    main()
