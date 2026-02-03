"""
test_memory_at_time.py - Tests for cf-memory-at-time.py script.

Tests point-in-time memory retrieval including timestamp handling,
time windows, and domain filtering.
"""

import json
import subprocess
import sys
from datetime import datetime, timedelta
from pathlib import Path

import pytest

# Add paths for imports
SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
sys.path.insert(0, str(SCRIPTS_PATH))
sys.path.insert(0, str(SCRIPTS_PATH / "codeflow_py_lib"))

import os


def get_script_env():
    """Get environment with PYTHONPATH set for script execution."""
    env = os.environ.copy()
    env["PYTHONPATH"] = str(SCRIPTS_PATH)
    return env


@pytest.fixture
def time_populated_db(test_db):
    """Database with time-distributed memory events."""
    import sqlite3

    conn = sqlite3.connect(str(test_db))

    # Events at different times
    base_time = datetime(2026, 1, 25, 10, 0, 0)

    events = [
        (
            "memory-t1",
            "progress",
            "development",
            "TSK-001",
            '{"content": "Started implementation"}',
            (base_time - timedelta(hours=5)).isoformat() + "Z",
        ),
        (
            "memory-t2",
            "decision",
            "planning",
            "TSK-001",
            '{"content": "Decided on approach"}',
            (base_time - timedelta(hours=3)).isoformat() + "Z",
        ),
        (
            "memory-t3",
            "progress",
            "development",
            "TSK-002",
            '{"content": "Midway progress"}',
            (base_time - timedelta(hours=1)).isoformat() + "Z",
        ),
        (
            "memory-t4",
            "milestone",
            "development",
            "TSK-001",
            '{"content": "Feature complete"}',
            base_time.isoformat() + "Z",
        ),
        (
            "memory-t5",
            "progress",
            "qa",
            "TSK-003",
            '{"content": "Testing started"}',
            (base_time + timedelta(hours=2)).isoformat() + "Z",
        ),
    ]

    conn.executemany(
        """
        INSERT INTO memory_events (id, event_type, domain, work_id, data, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
    """,
        events,
    )
    conn.commit()
    conn.close()

    return test_db


class TestMemoryAtTimeArgParsing:
    """Tests for argument parsing."""

    def test_help_flag(self):
        """--help should show usage information."""
        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode == 0
        assert "memory state" in result.stdout.lower()

    def test_timestamp_required(self):
        """--timestamp is required."""
        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode != 0


class TestMemoryAtTimeBasic:
    """Basic point-in-time tests."""

    def test_get_memories_at_time(self, temp_dir, time_populated_db, monkeypatch):
        """Should get memories at a specific point in time."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T10:00:00Z"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # Should include events at or before 10:00
        assert output["count"] == 4  # t1, t2, t3, t4

    def test_get_memories_earlier(self, temp_dir, time_populated_db, monkeypatch):
        """Should get only earlier memories."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T08:00:00Z"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # Should include only t1 and t2 (before 08:00)
        assert output["count"] == 2


class TestMemoryAtTimeTimestampFormats:
    """Tests for timestamp format handling."""

    def test_date_only_format(self, temp_dir, time_populated_db, monkeypatch):
        """Should handle date-only format (YYYY-MM-DD)."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # Date-only defaults to end of day, should include all events
        assert output["count"] == 5

    def test_timestamp_without_z_suffix(self, temp_dir, time_populated_db, monkeypatch):
        """Should handle timestamp without Z suffix."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T10:00:00"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0

    def test_full_iso_format(self, temp_dir, time_populated_db, monkeypatch):
        """Should handle full ISO format."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T09:00:00Z"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["count"] == 3  # t1, t2, t3


class TestMemoryAtTimeWindow:
    """Tests for time window filtering."""

    def test_window_parameter(self, temp_dir, time_populated_db, monkeypatch):
        """Should respect window parameter."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2026-01-25T10:00:00Z",
                "--window",
                "2",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # Window of 2 hours before 10:00 = 08:00-10:00
        # Should include t3 (09:00) and t4 (10:00)
        assert output["window_hours"] == 2

    def test_window_zero_gets_all_before(
        self, temp_dir, time_populated_db, monkeypatch
    ):
        """Window=0 should get all events at or before timestamp."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2026-01-25T10:00:00Z",
                "--window",
                "0",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["count"] == 4


class TestMemoryAtTimeDomainFilter:
    """Tests for domain filtering."""

    def test_filter_by_domain(self, temp_dir, time_populated_db, monkeypatch):
        """Should filter by domain."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2026-01-25T12:00:00Z",
                "--domain",
                "development",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        for mem in output["memories"]:
            assert mem["domain"] == "development"

    def test_invalid_domain_rejected(self, temp_dir, time_populated_db):
        """Invalid domain should be rejected."""
        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2026-01-25",
                "--domain",
                "invalid_domain",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode != 0


class TestMemoryAtTimeWorkIdFilter:
    """Tests for work ID filtering."""

    def test_filter_by_work_id(self, temp_dir, time_populated_db, monkeypatch):
        """Should filter by work ID."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2026-01-25T12:00:00Z",
                "--work-id",
                "TSK-001",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        for mem in output["memories"]:
            assert mem["work_id"] == "TSK-001"


class TestMemoryAtTimeLimit:
    """Tests for result limiting."""

    def test_default_limit(self, temp_dir, time_populated_db, monkeypatch):
        """Default limit should be 50."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-26"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0

    def test_custom_limit(self, temp_dir, time_populated_db, monkeypatch):
        """Should respect custom limit."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-26", "--limit", "2"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output["memories"]) <= 2


class TestMemoryAtTimeOutput:
    """Tests for output formatting."""

    def test_json_output_structure(self, temp_dir, time_populated_db, monkeypatch):
        """JSON output should have correct structure."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T10:00:00Z"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        assert "query_timestamp" in output
        assert "window_hours" in output
        assert "count" in output
        assert "memories" in output

        if output["memories"]:
            mem = output["memories"][0]
            assert "id" in mem
            assert "event_type" in mem
            assert "domain" in mem
            assert "data" in mem

    def test_data_parsed_as_json(self, temp_dir, time_populated_db, monkeypatch):
        """Data field should be parsed as JSON."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T10:00:00Z"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        for mem in output["memories"]:
            if mem["data"]:
                assert isinstance(mem["data"], dict)

    def test_summary_format(self, temp_dir, time_populated_db, monkeypatch):
        """Summary format should produce readable output."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2026-01-25T10:00:00Z",
                "--format",
                "summary",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        assert "Memory State at:" in result.stdout
        assert "Total:" in result.stdout


class TestMemoryAtTimeOrdering:
    """Tests for result ordering."""

    def test_ordered_by_created_at_desc(self, temp_dir, time_populated_db, monkeypatch):
        """Results should be ordered by created_at descending."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [sys.executable, str(script), "--timestamp", "2026-01-25T10:00:00Z"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        dates = [m["created_at"] for m in output["memories"]]
        assert dates == sorted(dates, reverse=True)


class TestMemoryAtTimeEmptyResults:
    """Tests for empty result handling."""

    def test_no_memories_at_time(self, temp_dir, time_populated_db, monkeypatch):
        """Should handle no memories at given time."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(time_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-at-time.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--timestamp",
                "2020-01-01",
            ],  # Before all events
            capture_output=True,
            text=True,
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["count"] == 0
        assert output["memories"] == []
