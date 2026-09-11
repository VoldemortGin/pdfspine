"""Contract checks, without a renderer/model or optional corpus downloads."""

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "financial_selection", Path(__file__).with_name("select.py")
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def cell(rows, cols, text="value", **extra):
    return dict(
        row_nums=rows,
        column_nums=cols,
        json_text_content=text,
        pdf_text_content="PDF fallback",
        **extra,
    )


class DraftTests(unittest.TestCase):
    def test_empty_key_presence_and_explicit_fallback(self):
        self.assertEqual(
            MODULE.chosen_text(cell([0], [0], "")), ("json_text_content", "")
        )
        self.assertEqual(
            MODULE.chosen_text({"pdf_text_content": "fallback"}),
            ("pdf_text_content", "fallback"),
        )

    def test_escaped_text_and_normalization_preserve_financial_symbols(self):
        markup, records = MODULE.table_draft(
            {"cells": [cell([0], [0], ' e\u0301\n ($1,200.00) <&" ')]}
        )
        self.assertIn("é ($1,200.00) &lt;&amp;&quot;", markup)
        self.assertEqual(
            records[0]["raw"]["json_text_content"], ' e\u0301\n ($1,200.00) <&" '
        )
        self.assertNotIn("<thead", markup)

    def test_reverse_contiguous_indices_preserved(self):
        original = {"cells": [cell([0], [1, 0], "", pdf_text_tight_bbox=[])]}
        before = copy.deepcopy(original)
        markup, records = MODULE.table_draft(original)
        self.assertEqual(original, before)
        self.assertIn('colspan="2"', markup)
        self.assertEqual(records[0]["raw"]["column_nums"], [1, 0])
        self.assertEqual(records[0]["canonical_columns"], [0, 1])

    def test_invalid_topology_and_nonfinite_geometry_fail(self):
        cases = [
            [cell([0], [0, 0])],
            [cell([0], [0, 2])],
            [cell([0], [0]), cell([0], [0])],
            [cell([1], [0])],
            [cell([0], [0], pdf_bbox=[0, 0, float("inf"), 1])],
        ]
        for cells in cases:
            with self.subTest(cells=cells), self.assertRaises(ValueError):
                MODULE.table_draft({"cells": cells})

    def test_annotation_header_and_projected_marker_only(self):
        markup, _ = MODULE.table_draft(
            {
                "cells": [
                    cell([0], [0], is_column_header=True),
                    cell([1], [0], is_projected_row_header=True),
                ]
            }
        )
        self.assertIn("<th ", markup)
        self.assertIn('data-projected-row-header="true"', markup)
        self.assertNotIn("<thead", markup)

    def test_unknown_or_missing_schema_rejected_before_assets(self):
        for schema in ("unknown.schema.v1", None):
            with (
                self.subTest(schema=schema),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                manifest = {"dataset_id": MODULE.DATASET, "review_status": "unreviewed"}
                if schema is not None:
                    manifest["schema"] = schema
                (root / "manifest.json").write_text(json.dumps(manifest))
                for selected_only in (False, True):
                    with self.assertRaisesRegex(ValueError, "schema"):
                        MODULE.verify(root, selected_only=selected_only)

    def test_existing_output_review_is_never_reset(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ledger = root / "review-ledger.json"
            ledger.write_text('{"human_review":"reviewed"}')
            with self.assertRaisesRegex(ValueError, "output exists"):
                MODULE.generate(root / "nonexistent-assets", root)
            self.assertEqual(ledger.read_text(), '{"human_review":"reviewed"}')

    def test_missing_and_modified_declared_files_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(FileNotFoundError):
                MODULE.checked_file(root, "missing.pdf", "0" * 64)
            path = root / "draft.html"
            path.write_text("original")
            expected = MODULE.digest(path)
            path.write_text("human correction")
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                MODULE.checked_file(root, "draft.html", expected)
            self.assertEqual(path.read_text(), "human correction")

    def test_frozen_selection_deterministic_and_issuer_disjoint(self):
        selection = json.loads(
            (Path(__file__).parent / "v1/selection.json").read_text()
        )
        pool = selection["pool"]
        result = MODULE.select(pool)
        reversed_result = MODULE.select(list(reversed(pool)))
        self.assertEqual(result, reversed_result)
        self.assertEqual(
            [r["document_id"] for r in result["development"][:3]], list(MODULE.FORCED)
        )
        self.assertEqual(
            {k: [r["document_id"] for r in rows] for k, rows in result.items()},
            selection["selected"],
        )
        self.assertEqual(
            [len(result[k]) for k in ("development", "evaluation")], [10, 30]
        )
        self.assertEqual(
            len({r["issuer"] for rows in result.values() for r in rows}), 40
        )


if __name__ == "__main__":
    unittest.main()
