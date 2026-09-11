"""Coverage-window dates are advisory, including at/after the exclusive end."""

from __future__ import annotations

import importlib.util
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "check_vet_trust_windows.py"
spec = importlib.util.spec_from_file_location("vet_windows", SCRIPT)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


@pytest.mark.parametrize("remaining", [31, 30, 1, 0, -1])
def test_advisory_boundaries_never_fail(tmp_path, capsys, monkeypatch, remaining):
    monkeypatch.delenv("GITHUB_STEP_SUMMARY", raising=False)
    end = date(2027, 9, 5)
    audits = tmp_path / "audits.toml"
    audits.write_text('[[trusted.sample]]\nuser-id=1\nend="2027-09-05"\n')
    today = end - timedelta(days=remaining)
    assert checker.main(["--audits", str(audits), "--today", str(today)]) == 0
    output = capsys.readouterr().out
    assert ("::warning::" in output) is (remaining <= 30)
    if remaining == 0:
        assert "exclusive end today" in output
    elif remaining < 0:
        assert "ended 1 day(s) ago" in output
    elif remaining <= 30:
        assert f"ends in {remaining} day(s)" in output


def test_multiple_records_dates_and_empty_store(tmp_path):
    audits = tmp_path / "audits.toml"
    audits.write_text(
        '[[trusted.z]]\nuser-id=2\nend="2028-01-01"\n'
        "[[trusted.z]]\nuser-id=1\nend=2027-09-05\n"
        '[[trusted.a]]\nuser-id=3\nend="2027-09-05"\n'
    )
    assert checker.read_windows(audits) == [
        ("a", 3, date(2027, 9, 5)),
        ("z", 1, date(2027, 9, 5)),
        ("z", 2, date(2028, 1, 1)),
    ]
    audits.write_text("[audits]\n")
    assert checker.read_windows(audits) == []


@pytest.mark.parametrize(
    "content",
    [
        "not toml",
        '[trusted.sample]\nuser-id=1\nend="2027-09-05"',
        '[[trusted.sample]]\nuser-id=1\nend="bad-date"',
        '[[trusted.sample]]\nuser-id=true\nend="2027-09-05"',
    ],
)
def test_invalid_input_is_an_operational_error(tmp_path, capsys, content):
    audits = tmp_path / "audits.toml"
    audits.write_text(content)
    assert checker.main(["--audits", str(audits)]) == 1
    assert "could not read/report" in capsys.readouterr().err


def test_warning_cannot_inject_workflow_commands(tmp_path, capsys, monkeypatch):
    audits = tmp_path / "audits.toml"
    audits.write_text(
        '[[trusted."bad%\\r\\n::error::injected"]]\nuser-id=1\nend="2027-09-05"'
    )
    summary = tmp_path / "summary.md"
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))
    assert checker.main(["--audits", str(audits), "--today", "2027-09-05"]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 2
    assert "bad%25%0D%0A::error::injected" in lines[1]
    assert not any(line.startswith("::error::") for line in lines)
    assert "publication-date coverage" in summary.read_text()


def test_default_today_uses_utc_at_local_day_boundary(monkeypatch):
    class Clock:
        @staticmethod
        def now(zone):
            assert zone is timezone.utc
            return datetime(2027, 9, 5, 0, 1, tzinfo=timezone.utc)

    monkeypatch.setattr(checker, "datetime", Clock)
    assert checker.utc_today() == date(2027, 9, 5)


def test_negative_lookahead_is_an_operational_error(tmp_path, monkeypatch):
    monkeypatch.delenv("GITHUB_STEP_SUMMARY", raising=False)
    audits = tmp_path / "audits.toml"
    audits.write_text("[audits]\n")
    assert checker.main(["--audits", str(audits), "--warn-days", "-1"]) == 1
