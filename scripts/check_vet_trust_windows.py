#!/usr/bin/env python3
"""Advisory reminder for trusted-publisher publication coverage windows.

The exclusive ``end`` date bounds publication coverage, not the lifetime of
already covered locked dependencies. Reminders never renew trust or fail a
build because a window ended. Invalid input is an operational error.
"""

from __future__ import annotations

import argparse
import html
import os
import sys
import tomllib
from datetime import date, datetime, timezone
from pathlib import Path

DEFAULT_AUDITS = Path(__file__).resolve().parents[1] / "supply-chain" / "audits.toml"


def utc_today() -> date:
    return datetime.now(timezone.utc).date()


def read_windows(path: Path) -> list[tuple[str, int, date]]:
    with path.open("rb") as stream:
        trusted = tomllib.load(stream).get("trusted", {})
    if not isinstance(trusted, dict):
        raise ValueError("trusted must be a table")
    windows = []
    for crate, records in trusted.items():
        if not isinstance(records, list):
            raise ValueError("trusted entries must be arrays of tables")
        for record in records:
            if not isinstance(record, dict) or type(record.get("user-id")) is not int:
                raise ValueError("trusted entry requires an integer user-id")
            end = record.get("end")
            if isinstance(end, str):
                end = date.fromisoformat(end)
            if type(end) is not date:
                raise ValueError("trusted entry requires an ISO end date")
            windows.append((crate, record["user-id"], end))
    return sorted(windows, key=lambda entry: (entry[2], entry[0], entry[1]))


def warning_text(crate: str, publisher: int, end: date, today: date) -> str:
    remaining = (end - today).days
    if remaining > 0:
        state = f"ends in {remaining} day(s)"
    elif remaining == 0:
        state = "has reached its exclusive end today"
    else:
        state = f"ended {-remaining} day(s) ago"
    return (
        f"{crate} (publisher {publisher}): publication coverage window {state} "
        f"({end.isoformat()}). Review coverage for future dependency updates; "
        "already covered locked dependencies are not invalidated by this date."
    )


def escape_command_data(value: str) -> str:
    return value.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def report(windows: list[tuple[str, int, date]], today: date, warn_days: int) -> str:
    if warn_days < 0:
        raise ValueError("warn-days must be nonnegative")
    due = [entry for entry in windows if (entry[2] - today).days <= warn_days]
    heading = (
        f"Checked {len(windows)} trusted-publisher coverage windows on "
        f"{today.isoformat()} (UTC); {len(due)} within {warn_days} days or ended."
    )
    print(heading)
    summary = ["## cargo-vet coverage-window advisory", "", heading, ""]
    for crate, publisher, end in due:
        message = warning_text(crate, publisher, end, today)
        print(f"::warning::{escape_command_data(message)}")
        summary.append(
            "- " + html.escape(message).replace("\r", " ").replace("\n", " ")
        )
    summary.extend(
        [
            "",
            "This is an advisory about publication-date coverage, not a failure "
            "of fixed dependencies. No trust entries or audit criteria were changed.",
        ]
    )
    return "\n".join(summary) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audits", type=Path, default=DEFAULT_AUDITS)
    parser.add_argument(
        "--today",
        type=date.fromisoformat,
        help="explicit UTC date (YYYY-MM-DD), for reproducible checks",
    )
    parser.add_argument("--warn-days", type=int, default=30)
    args = parser.parse_args(argv)
    try:
        summary = report(
            read_windows(args.audits), args.today or utc_today(), args.warn_days
        )
        if path := os.environ.get("GITHUB_STEP_SUMMARY"):
            with Path(path).open("a", encoding="utf-8") as stream:
                stream.write(summary)
    except (OSError, ValueError) as error:
        print(
            f"coverage-window check could not read/report its input: {error!r}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
