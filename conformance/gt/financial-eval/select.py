#!/usr/bin/env python3
"""Freeze annotation-derived financial table drafts; never overwrite a review."""

import argparse
import hashlib
import html
import json
import math
from pathlib import Path
import re
import unicodedata

SEED = "pdfspine-financial-eval-v1"
FORCED = ("ADBE_2011_page_118", "ADI_2010_page_51", "AMP_2015_page_94")
TARGETS = {
    "development": dict(
        multi_table=3,
        col_span=7,
        row_span=5,
        multi_header=5,
        currency=8,
        parenthesized_number=5,
        blank_cell=6,
        dense_rows=2,
        text_source_disagreement=8,
    ),
    "evaluation": dict(
        multi_table=8,
        col_span=21,
        row_span=15,
        multi_header=15,
        currency=24,
        parenthesized_number=16,
        blank_cell=18,
        dense_rows=4,
        text_source_disagreement=24,
    ),
}
DATASET = "fintabnet-c-financial-40-v1"
PREFIX = "../../corpus-fintabnet"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def safe_id(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        raise ValueError(f"invalid source identifier: {value!r}")
    return value


def chosen_text(cell):
    key = "json_text_content" if "json_text_content" in cell else "pdf_text_content"
    text = cell.get(key, "")
    if not isinstance(text, str):
        raise ValueError("cell text must be a string")
    return key, text


def normalize(text):
    return " ".join(unicodedata.normalize("NFC", text).split())


def span(values):
    if not values or any(type(v) is not int or v < 0 for v in values):
        raise ValueError("invalid span indices")
    ordered = sorted(values)
    if ordered != list(range(ordered[0], ordered[-1] + 1)):
        raise ValueError("duplicate or noncontiguous span")
    return ordered


def table_draft(table):
    """Return HTML and lossless source-cell sidecar, rejecting invalid topology."""
    occupied = set()
    anchors = {}
    records = []
    for index, cell in enumerate(table["cells"]):
        rows, cols = span(cell["row_nums"]), span(cell["column_nums"])
        for key in ("pdf_bbox", "pdf_text_tight_bbox"):
            bbox = cell.get(key)
            if bbox not in (None, []) and (
                len(bbox) != 4
                or any(
                    not isinstance(x, (int, float)) or not math.isfinite(x)
                    for x in bbox
                )
            ):
                raise ValueError("invalid cell geometry")
        slots = {(r, c) for r in rows for c in cols}
        if occupied & slots:
            raise ValueError("overlapping cells")
        occupied.update(slots)
        source, raw = chosen_text(cell)
        record = dict(
            source_cell_index=index,
            raw=cell,
            text_source=source,
            normalized_text=normalize(raw),
            canonical_rows=rows,
            canonical_columns=cols,
        )
        records.append(record)
        anchors[rows[0], cols[0]] = record
    if not occupied:
        raise ValueError("empty table")
    height = max(r for r, _ in occupied) + 1
    width = max(c for _, c in occupied) + 1
    if len(occupied) != height * width:
        raise ValueError("grid holes")
    lines = [
        "<!-- Annotation-derived draft: UNREVIEWED; not scoring gold. -->",
        '<table data-review-status="unreviewed">',
        "<tbody>",
    ]
    for row in range(height):
        lines.append("<tr>")
        for col in range(width):
            record = anchors.get((row, col))
            if record is None:
                continue
            cell = record["raw"]
            tag = "th" if cell.get("is_column_header", False) else "td"
            projected = str(bool(cell.get("is_projected_row_header", False))).lower()
            lines.append(
                f'<{tag} data-source-cell-index="{record["source_cell_index"]}" '
                f'data-projected-row-header="{projected}" '
                f'rowspan="{len(record["canonical_rows"])}" '
                f'colspan="{len(record["canonical_columns"])}">'
                f"{html.escape(record['normalized_text'], quote=True)}</{tag}>"
            )
        lines.append("</tr>")
    lines += ["</tbody>", "</table>"]
    return "\n".join(lines) + "\n", records


def features(tables):
    cells = [c for t in tables for c in t["cells"]]
    texts = [chosen_text(c)[1] for c in cells]
    return dict(
        multi_table=len(tables) > 1,
        col_span=any(len(c["column_nums"]) > 1 for c in cells),
        row_span=any(len(c["row_nums"]) > 1 for c in cells),
        multi_header=any(
            len(
                {
                    r
                    for c in t["cells"]
                    if c.get("is_column_header")
                    for r in c["row_nums"]
                }
            )
            > 1
            for t in tables
        ),
        currency=any(re.search(r"[$€£¥]", t) is not None for t in texts),
        parenthesized_number=any(
            re.search(r"\([\s$€£¥]*[\d,.]+\)", t) is not None for t in texts
        ),
        blank_cell=any(not t.strip() for t in texts),
        dense_rows=any(
            max(r for c in t["cells"] for r in c["row_nums"]) >= 29 for t in tables
        ),
        text_source_disagreement=any(
            c.get("json_text_content", "") != c.get("pdf_text_content", "")
            for c in cells
        ),
    )


def load_pool(root):
    source = read(root / "manifest.json")
    pool = []
    seen = set()
    for entry in source["entries"]:
        doc = safe_id(entry["document_id"])
        if doc in seen:
            raise ValueError("duplicate document identity")
        seen.add(doc)
        pdf = root / "pdfs" / f"{doc}.pdf"
        annotation = root / "annotations" / f"{doc}_tables.json"
        tables = read(annotation)
        for table in tables:
            if (
                table["document_id"] != doc
                or table["pdf_page_index"] != entry["pdf_page_index"]
            ):
                raise ValueError("source table identity mismatch")
            table_draft(table)
        pool.append(
            dict(
                document_id=doc,
                issuer=entry["pdf_rel_path"].split("/")[0],
                pdf=f"{PREFIX}/pdfs/{doc}.pdf",
                pdf_sha256=digest(pdf),
                annotation=f"{PREFIX}/annotations/{doc}_tables.json",
                annotation_sha256=digest(annotation),
                pdf_page_index=entry["pdf_page_index"],
                source_pdf_rel_path=entry["pdf_rel_path"],
                pdf_license=entry["pdf_license"],
                annotation_license=entry["anno_license"],
                n_tables=len(tables),
                features=features(tables),
            )
        )
    return source, sorted(pool, key=lambda r: r["document_id"])


def select(pool):
    by_id = {r["document_id"]: r for r in pool}
    used = set()
    selected = {}
    for partition, count in (("development", 10), ("evaluation", 30)):
        chosen = [by_id[x] for x in FORCED] if partition == "development" else []
        used.update(r["issuer"] for r in chosen)
        while len(chosen) < count:
            counts = {
                k: sum(r["features"][k] for r in chosen) for k in TARGETS[partition]
            }

            def rank(row):
                score = sum(
                    (target - counts[k]) * 2520 // target
                    for k, target in TARGETS[partition].items()
                    if counts[k] < target and row["features"][k]
                )
                tie = hashlib.sha256(
                    (SEED + "\0" + partition + "\0" + row["document_id"]).encode()
                ).hexdigest()
                return -score, tie, row["document_id"]

            candidates = [r for r in pool if r["issuer"] not in used]
            if not candidates:
                raise ValueError("not enough distinct issuers")
            winner = min(candidates, key=rank)
            chosen.append(winner)
            used.add(winner["issuer"])
        if any(
            sum(r["features"][k] for r in chosen) < target
            for k, target in TARGETS[partition].items()
        ):
            raise ValueError("feature targets not met")
        selected[partition] = chosen
    return selected


def generate(root, output):
    if output.exists():
        raise ValueError(
            "output exists: use read-only verify, or generate in a new directory"
        )
    source, pool = load_pool(root)
    if len(pool) != 150 or sum(r["n_tables"] for r in pool) != 186:
        raise ValueError("v1 requires the frozen 150-page / 186-table source pool")
    selected = select(pool)
    output.mkdir(parents=True, exist_ok=False)
    selection = dict(
        algorithm="annotation-deficit-issuer-disjoint-v1",
        seed=SEED,
        forced_development=list(FORCED),
        minimum_targets=TARGETS,
        historical_pool_exposure="all 150 pages previously benchmarked",
        source_manifest_sha256=digest(root / "manifest.json"),
        pool=pool,
        selected={k: [r["document_id"] for r in v] for k, v in selected.items()},
    )
    write(output / "selection.json", selection)
    selection_hash = digest(output / "selection.json")
    entries, ledger = [], []
    seen_tables = set()
    for partition, rows in selected.items():
        for row in rows:
            entry = dict(
                row,
                partition=partition,
                review_status="unreviewed",
                historical_benchmark_exposure=True,
                prior_targeted_tuning=row["document_id"] in FORCED,
                tables=[],
            )
            tables = read(root / "annotations" / f"{row['document_id']}_tables.json")
            entry["source_splits"] = sorted({t["split"] for t in tables})
            for index, table in enumerate(tables):
                sid = safe_id(table["structure_id"])
                if sid in seen_tables:
                    raise ValueError("duplicate table identity")
                seen_tables.add(sid)
                markup, records = table_draft(table)
                html_path = output / "html" / f"{sid}.html"
                html_path.parent.mkdir(exist_ok=True)
                html_path.write_text(markup, encoding="utf-8")
                cells_path = output / "cells" / f"{sid}.json"
                write(
                    cells_path,
                    dict(
                        structure_id=sid,
                        review_status="unreviewed",
                        normalization="Unicode NFC + whitespace collapse/strip",
                        source_table_metadata={
                            k: v for k, v in table.items() if k != "cells"
                        },
                        cells=records,
                    ),
                )
                item = dict(
                    structure_id=sid,
                    source_table_index=index,
                    source_split=table["split"],
                    source_annotation_file=table["fintabnet_source_file_name"],
                    source_line_index=table["fintabnet_source_line_index"],
                    source_table_id=table["fintabnet_source_table_id"],
                    html=f"html/{sid}.html",
                    html_sha256=digest(html_path),
                    cells=f"cells/{sid}.json",
                    cells_sha256=digest(cells_path),
                    review_status="unreviewed",
                )
                entry["tables"].append(item)
                ledger.append(
                    dict(
                        item,
                        document_id=row["document_id"],
                        pdf_sha256=row["pdf_sha256"],
                        annotation_sha256=row["annotation_sha256"],
                        human_review=dict(
                            status="unreviewed",
                            reviewer=None,
                            reviewed_at=None,
                            corrections=[],
                        ),
                        ai_visual_review=dict(
                            status="not_performed", reviewer=None, notes=[]
                        ),
                        machine_validation=dict(topology="pass", source_hashes="pass"),
                    )
                )
            entries.append(entry)
    write(
        output / "review-ledger.json",
        dict(dataset_id=DATASET, selection_sha256=selection_hash, tables=ledger),
    )
    provenance = {
        k: source[k]
        for k in (
            "dataset",
            "dataset_paper",
            "annotations_source",
            "annotations_license",
            "pdf_source",
            "pdf_source_original",
            "pdf_license",
        )
    }
    write(
        output / "manifest.json",
        dict(
            schema="pdfspine.financial-eval-drafts.v1",
            dataset_id=DATASET,
            review_status="unreviewed",
            provenance=provenance,
            selection=dict(path="selection.json", sha256=selection_hash),
            review_ledger=dict(
                path="review-ledger.json", sha256=digest(output / "review-ledger.json")
            ),
            entries=entries,
        ),
    )
    return verify(output, root)


def checked_file(directory, reference, expected):
    path = directory / reference
    if digest(path) != expected:
        raise ValueError(f"hash mismatch: {reference}")
    return path


def verify(output, corpus_root=None, acceptance=False, *, selected_only=False):
    manifest = read(output / "manifest.json")
    linked = {}
    for key in ("selection", "review_ledger"):
        ref = manifest[key]
        linked[key] = read(checked_file(output, ref["path"], ref["sha256"]))
    selection, ledger = linked["selection"], linked["review_ledger"]
    if ledger["selection_sha256"] != manifest["selection"]["sha256"]:
        raise ValueError("ledger selection mismatch")
    pool = selection["pool"]
    actual_root = (
        corpus_root if corpus_root is not None else (output / PREFIX).resolve()
    )
    if not selected_only:
        if digest(actual_root / "manifest.json") != selection["source_manifest_sha256"]:
            raise ValueError("source manifest hash mismatch")
        _, actual_pool = load_pool(actual_root)
        if actual_pool != pool:
            raise ValueError("source pool features or identity mismatch")
    if (
        selection["seed"] != SEED
        or selection["minimum_targets"] != TARGETS
        or selection["forced_development"] != list(FORCED)
        or selection["algorithm"] != "annotation-deficit-issuer-disjoint-v1"
    ):
        raise ValueError("selection contract mismatch")
    if (
        manifest["review_status"] != "unreviewed"
        or manifest["dataset_id"] != DATASET
        or ledger["dataset_id"] != DATASET
    ):
        raise ValueError("v1 draft identity/review state mismatch")
    asset_rows = manifest["entries"] if selected_only else pool
    for row in asset_rows:
        for kind, folder, suffix in (
            ("pdf", "pdfs", ".pdf"),
            ("annotation", "annotations", "_tables.json"),
        ):
            if corpus_root is None:
                checked_file(output, row[kind], row[kind + "_sha256"])
            else:
                checked_file(
                    corpus_root,
                    f"{folder}/{safe_id(row['document_id'])}{suffix}",
                    row[kind + "_sha256"],
                )
    if len(pool) != 150 or sum(r["n_tables"] for r in pool) != 186:
        raise ValueError("pool size mismatch")
    expected = select(pool)
    if selection["selected"] != {
        k: [r["document_id"] for r in v] for k, v in expected.items()
    }:
        raise ValueError("selection mismatch")
    entries = manifest["entries"]
    if len(entries) != 40 or len({r["issuer"] for r in entries}) != 40:
        raise ValueError("selected page/issuer count mismatch")
    by_id = {r["document_id"]: r for r in pool}
    ledger_by_id = {r["structure_id"]: r for r in ledger["tables"]}
    total = 0
    seen_tables = set()
    for partition, rows in expected.items():
        actual = [r["document_id"] for r in entries if r["partition"] == partition]
        if actual != [r["document_id"] for r in rows]:
            raise ValueError("entry selection mismatch")
    for entry in entries:
        if (
            entry["review_status"] != "unreviewed"
            or entry["historical_benchmark_exposure"] is not True
            or entry["prior_targeted_tuning"] != (entry["document_id"] in FORCED)
        ):
            raise ValueError("entry review/exposure mismatch")
        source_tables = read(
            actual_root / "annotations" / f"{entry['document_id']}_tables.json"
        )
        if len(entry["tables"]) != len(source_tables) or [
            t["source_table_index"] for t in entry["tables"]
        ] != list(range(len(source_tables))):
            raise ValueError("source table count mismatch")
        for key, value in by_id[entry["document_id"]].items():
            if entry[key] != value:
                raise ValueError("entry source identity mismatch")
        for table in entry["tables"]:
            total += 1
            if table["structure_id"] in seen_tables:
                raise ValueError("duplicate selected table")
            seen_tables.add(table["structure_id"])
            record = ledger_by_id[table["structure_id"]]
            for key, value in table.items():
                if record[key] != value:
                    raise ValueError("ledger table mismatch")
            for key in ("pdf_sha256", "annotation_sha256", "document_id"):
                if record[key] != entry[key]:
                    raise ValueError("ledger source mismatch")
            if (
                table["review_status"] != "unreviewed"
                or record["human_review"]["status"] != "unreviewed"
                or record["ai_visual_review"]["status"] != "not_performed"
            ):
                raise ValueError(
                    "draft review ledger was changed; preserve it in a new reviewed revision"
                )
            for kind in ("html", "cells"):
                checked_file(output, table[kind], table[kind + "_sha256"])
            source_table = source_tables[table["source_table_index"]]
            markup, records = table_draft(source_table)
            sidecar = read(output / table["cells"])
            if (
                source_table["structure_id"] != table["structure_id"]
                or sidecar["source_table_metadata"]
                != {k: v for k, v in source_table.items() if k != "cells"}
                or sidecar["cells"] != records
                or (output / table["html"]).read_text(encoding="utf-8") != markup
            ):
                raise ValueError("derived draft differs from source annotation")
    if total != 60 or len(ledger_by_id) != total or len(ledger["tables"]) != total:
        raise ValueError("table count mismatch")
    if acceptance:
        raise ValueError(
            "v1 is unreviewed annotation-derived data; human acceptance is not satisfied"
        )
    return dict(
        pages=40,
        tables=60,
        pool_pages=150,
        pool_tables=186,
        integrity="pass",
        human_acceptance="unreviewed",
        comparable=False,
        verification_scope="selected-dataset"
        if selected_only
        else "full-selection-pool",
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("generate", "verify", "verify-selected"))
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--corpus-root",
        type=Path,
        help="original corpus-fintabnet cache; hashes remain mandatory",
    )
    parser.add_argument("--acceptance", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "generate":
            if args.corpus_root is None or args.acceptance:
                parser.error(
                    "generate requires --corpus-root and does not perform acceptance"
                )
            result = generate(args.corpus_root, args.output)
        else:
            result = verify(
                args.output,
                args.corpus_root,
                args.acceptance,
                selected_only=args.command == "verify-selected",
            )
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(1, f"validation failed: {error}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
