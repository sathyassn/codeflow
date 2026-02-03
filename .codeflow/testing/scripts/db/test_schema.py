"""
test_schema.py - Tests for database schema.
"""

import sys
from pathlib import Path

import pytest

# Add paths for imports
PY_LIB_PATH = Path(__file__).parent.parent.parent.parent / "scripts" / "codeflow_py_lib"
sys.path.insert(0, str(PY_LIB_PATH))


class TestSchemaCreation:
    """Tests for schema creation."""

    def test_schema_file_exists(self, schema_path):
        """Schema file should exist."""
        assert schema_path.exists(), f"Schema file not found at {schema_path}"

    def test_creates_memory_events_table(self, db_connection):
        """Should create memory_events table."""
        cursor = db_connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='memory_events'"
        )
        assert cursor.fetchone() is not None

    def test_creates_work_claims_table(self, db_connection):
        """Should create work_claims table."""
        cursor = db_connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='work_claims'"
        )
        assert cursor.fetchone() is not None

    def test_creates_sessions_table(self, db_connection):
        """Should create sessions table."""
        cursor = db_connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='sessions'"
        )
        assert cursor.fetchone() is not None

    def test_creates_epics_table(self, db_connection):
        """Should create epics table."""
        cursor = db_connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='epics'"
        )
        assert cursor.fetchone() is not None

    def test_creates_tasks_table(self, db_connection):
        """Should create tasks table."""
        cursor = db_connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='tasks'"
        )
        assert cursor.fetchone() is not None

    def test_creates_fts_index(self, db_connection):
        """Should create FTS5 index for memory search."""
        cursor = db_connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='memory_fts'"
        )
        assert cursor.fetchone() is not None


class TestSchemaConstraints:
    """Tests for schema constraints."""

    def test_memory_events_event_type_constraint(self, db_connection):
        """Should enforce event_type CHECK constraint."""
        with pytest.raises(Exception):  # sqlite3.IntegrityError
            db_connection.execute(
                "INSERT INTO memory_events (id, event_type, domain, data) VALUES (?, ?, ?, ?)",
                ("test-id", "invalid_type", "development", "{}"),
            )

    def test_memory_events_domain_constraint(self, db_connection):
        """Should enforce domain CHECK constraint."""
        with pytest.raises(Exception):
            db_connection.execute(
                "INSERT INTO memory_events (id, event_type, domain, data) VALUES (?, ?, ?, ?)",
                ("test-id", "progress", "invalid_domain", "{}"),
            )

    def test_memory_events_valid_insert(self, db_connection, sample_memory_event):
        """Should accept valid memory event."""
        db_connection.execute(
            "INSERT INTO memory_events (id, event_type, domain, work_id, data, memory_type) VALUES (?, ?, ?, ?, ?, ?)",
            (
                sample_memory_event["id"],
                sample_memory_event["event_type"],
                sample_memory_event["domain"],
                sample_memory_event["work_id"],
                sample_memory_event["data"],
                sample_memory_event["memory_type"],
            ),
        )
        db_connection.commit()

        cursor = db_connection.execute(
            "SELECT * FROM memory_events WHERE id = ?", (sample_memory_event["id"],)
        )
        result = cursor.fetchone()
        assert result is not None
        assert result["event_type"] == "progress"
