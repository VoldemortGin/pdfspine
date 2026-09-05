"""Build a stable, fingerprinted corpus list from explicit PDF roots."""

import argparse
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[1]


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def selection_sha256(entries: list[dict[str, object]]) -> str:
    """Hash ordered source-relative names and PDF content hashes."""
    portable_selection = [
        [entry["source_index"], entry["relative"], entry["sha256"]] for entry in entries
    ]
    return hashlib.sha256(
        json.dumps(
            portable_selection, separators=(",", ":"), ensure_ascii=False
        ).encode()
    ).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument(
        "--root",
        action="append",
        type=Path,
        help="PDF file or directory; may be repeated and order is significant",
    )
    source.add_argument(
        "--manifest",
        type=Path,
        help="rebuild the exact corpus from a frozen repository-relative manifest",
    )
    parser.add_argument("--cap", type=int, default=300)
    parser.add_argument(
        "--freeze",
        type=Path,
        help="write the selected paths and hashes as a portable manifest",
    )
    args = parser.parse_args()
    if args.manifest and args.freeze:
        parser.error("--freeze is only valid when selecting a new corpus with --root")

    entries: list[dict[str, object]] = []
    roots: list[Path] = []
    if args.manifest:
        frozen = json.loads(args.manifest.read_text())
        for frozen_entry in frozen["files"]:
            relative_path = Path(frozen_entry["path"])
            if relative_path.is_absolute() or ".." in relative_path.parts:
                raise ValueError(
                    f"manifest path must be repository-relative: {relative_path}"
                )
            path = (REPO_ROOT / relative_path).resolve()
            if not path.is_file():
                raise FileNotFoundError(f"manifest PDF is missing: {relative_path}")
            digest = file_sha256(path)
            if digest != frozen_entry["sha256"]:
                raise ValueError(
                    f"manifest SHA-256 mismatch for {relative_path}: "
                    f"expected {frozen_entry['sha256']}, got {digest}"
                )
            if path.stat().st_size != frozen_entry["size"]:
                raise ValueError(f"manifest size mismatch for {relative_path}")
            entries.append({**frozen_entry, "path": str(path)})
        if len(entries) != frozen["count"]:
            raise ValueError("manifest count does not match its file entries")
        actual_selection_sha256 = selection_sha256(entries)
        if actual_selection_sha256 != frozen["selection_sha256"]:
            raise ValueError("manifest selection SHA-256 does not match its entries")
    else:
        candidates: list[tuple[int, str, Path]] = []
        roots = [root.expanduser().resolve() for root in args.root]
        for root_index, root in enumerate(roots):
            if not root.exists():
                raise FileNotFoundError(f"corpus root is missing: {root}")
            paths = [root] if root.is_file() else root.rglob("*")
            for path in paths:
                if path.is_file() and path.suffix.lower() == ".pdf":
                    candidates.append(
                        (
                            root_index,
                            str(path.relative_to(root) if root.is_dir() else path.name),
                            path.resolve(),
                        )
                    )
        seen_content: set[str] = set()
        for root_index, relative, path in sorted(
            candidates, key=lambda item: (item[0], item[1])
        ):
            digest = file_sha256(path)
            if digest in seen_content:
                continue
            seen_content.add(digest)
            entries.append(
                {
                    "path": str(path),
                    "sha256": digest,
                    "source_index": root_index,
                    "source_id": path.stem,
                    "root": str(roots[root_index]),
                    "relative": relative,
                    "size": path.stat().st_size,
                }
            )
            if len(entries) == args.cap:
                break

    corpus_text = "".join(f"{entry['path']}\n" for entry in entries)
    (HERE / "corpus.txt").write_text(corpus_text)
    corpus_sha256 = hashlib.sha256(corpus_text.encode()).hexdigest()
    selection_digest = selection_sha256(entries)
    local_manifest = {
        "schema": 1,
        "cap": len(entries) if args.manifest else args.cap,
        "count": len(entries),
        "roots": [str(root) for root in roots],
        "corpus_txt_sha256": corpus_sha256,
        "selection_sha256": selection_digest,
        "files": entries,
    }
    (HERE / "corpus-manifest.json").write_text(
        json.dumps(local_manifest, indent=2) + "\n"
    )
    if args.freeze:
        portable_entries = []
        for entry in entries:
            path = Path(str(entry["path"]))
            portable_entries.append(
                {
                    **entry,
                    "path": str(path.relative_to(REPO_ROOT)),
                    "root": str(Path(str(entry["root"])).relative_to(REPO_ROOT)),
                }
            )
        frozen_manifest = {
            "schema": 1,
            "count": len(entries),
            "selection_sha256": selection_digest,
            "files": portable_entries,
        }
        args.freeze.parent.mkdir(parents=True, exist_ok=True)
        args.freeze.write_text(json.dumps(frozen_manifest, indent=2) + "\n")
    print(
        f"total={len(entries)} corpus_txt_sha256={corpus_sha256} "
        f"selection_sha256={selection_digest}"
    )


if __name__ == "__main__":
    main()
