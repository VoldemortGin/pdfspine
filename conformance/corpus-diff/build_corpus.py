"""Build a stable, fingerprinted corpus list from explicit PDF roots."""

import argparse
import hashlib
import json
from datetime import datetime, timezone
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
    parser.add_argument(
        "--refresh-stale",
        action="store_true",
        help=(
            "with --manifest: accept entries whose PDF bytes changed, keep their old "
            "hash/size as previous_*, and --freeze a superseding manifest"
        ),
    )
    args = parser.parse_args()
    if args.refresh_stale and not args.manifest:
        parser.error("--refresh-stale is only valid with --manifest")
    if args.refresh_stale and not args.freeze:
        parser.error("--refresh-stale requires --freeze <new-manifest>")
    if args.manifest and args.freeze and not args.refresh_stale:
        parser.error(
            "--freeze with --manifest requires --refresh-stale; "
            "otherwise select a new corpus with --root"
        )
    if (
        args.manifest
        and args.freeze
        and args.freeze.resolve() == args.manifest.resolve()
    ):
        parser.error("--freeze must name a new manifest; the superseded one is kept")

    entries: list[dict[str, object]] = []
    portable_entries: list[dict[str, object]] = []
    refreshed: list[str] = []
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
            size = path.stat().st_size
            portable_entry = dict(frozen_entry)
            if digest != frozen_entry["sha256"] or size != frozen_entry["size"]:
                if not args.refresh_stale:
                    raise ValueError(
                        f"manifest SHA-256/size mismatch for {relative_path}: "
                        f"expected {frozen_entry['sha256']} ({frozen_entry['size']} "
                        f"bytes), got {digest} ({size} bytes); pass --refresh-stale "
                        "with --freeze to supersede the manifest"
                    )
                portable_entry.update(
                    sha256=digest,
                    size=size,
                    previous_sha256=frozen_entry["sha256"],
                    previous_size=frozen_entry["size"],
                )
                refreshed.append(frozen_entry["path"])
            portable_entries.append(portable_entry)
            entries.append({**portable_entry, "path": str(path)})
        if len(entries) != frozen["count"]:
            raise ValueError("manifest count does not match its file entries")
        if selection_sha256(frozen["files"]) != frozen["selection_sha256"]:
            raise ValueError("manifest selection SHA-256 does not match its entries")
        if args.refresh_stale and not refreshed:
            raise ValueError(f"--refresh-stale: no stale entry in {args.manifest}")
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
    if args.freeze and args.manifest:
        lineage_keys = (
            "schema",
            "count",
            "selection_sha256",
            "supersedes",
            "refreshed_utc",
            "refreshed",
            "files",
        )
        frozen_manifest = {
            "schema": 1,
            "count": len(entries),
            "selection_sha256": selection_digest,
            "supersedes": {
                "manifest": args.manifest.name,
                "selection_sha256": frozen["selection_sha256"],
            },
            "refreshed_utc": datetime.now(timezone.utc).date().isoformat(),
            "refreshed": refreshed,
            **{k: v for k, v in frozen.items() if k not in lineage_keys},
            "files": portable_entries,
        }
    elif args.freeze:
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
    if args.freeze:
        args.freeze.parent.mkdir(parents=True, exist_ok=True)
        args.freeze.write_text(json.dumps(frozen_manifest, indent=2) + "\n")
    print(
        f"total={len(entries)} corpus_txt_sha256={corpus_sha256} "
        f"selection_sha256={selection_digest}"
        + (f" refreshed={len(refreshed)}" if args.refresh_stale else "")
    )


if __name__ == "__main__":
    main()
