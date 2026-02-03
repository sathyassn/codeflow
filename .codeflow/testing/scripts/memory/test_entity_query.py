"""
test_entity_query.py - Tests for cf-entity-query.py script.

Tests entity querying functionality including filtering by name,
entity type, source ID, and category ID.
"""

import json
import subprocess
import sys
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


# Extended schema for entity tests
ENTITY_SCHEMA = """
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    description TEXT,
    source_event_id TEXT,
    category_id TEXT,
    properties TEXT DEFAULT '{}',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    id,
    name,
    entity_type,
    description,
    content='entities',
    content_rowid='rowid'
);
"""


@pytest.fixture
def entity_db(test_db):
    """Database with entities table."""
    import sqlite3

    conn = sqlite3.connect(str(test_db))
    conn.executescript(ENTITY_SCHEMA)
    conn.commit()
    conn.close()
    return test_db


@pytest.fixture
def populated_entity_db(entity_db):
    """Database with sample entities."""
    import sqlite3

    conn = sqlite3.connect(str(entity_db))

    entities = [
        (
            "entity-001",
            "UserAuthentication",
            "class",
            "Handles user login",
            "memory-001",
            "cat-auth",
            '{"visibility": "public"}',
        ),
        (
            "entity-002",
            "authenticate",
            "function",
            "Main auth function",
            "memory-001",
            "cat-auth",
            '{"params": ["user", "pass"]}',
        ),
        (
            "entity-003",
            "UserModel",
            "class",
            "User data model",
            "memory-002",
            "cat-models",
            '{"fields": ["id", "name"]}',
        ),
        (
            "entity-004",
            "validate_token",
            "function",
            "JWT validation",
            "memory-001",
            "cat-auth",
            '{"returns": "bool"}',
        ),
        (
            "entity-005",
            "config.py",
            "file",
            "Configuration file",
            "memory-003",
            "cat-config",
            "{}",
        ),
    ]

    conn.executemany(
        """
        INSERT INTO entities (id, name, entity_type, description, source_event_id, category_id, properties)
        VALUES (?, ?, ?, ?, ?, ?, ?)
    """,
        entities,
    )
    conn.commit()
    conn.close()

    return entity_db


class TestEntityQueryArgParsing:
    """Tests for argument parsing."""

    def test_help_flag(self, temp_dir):
        """--help should show usage information."""
        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode == 0
        assert "Query extracted entities" in result.stdout

    def test_default_format_is_json(self, temp_dir, entity_db, monkeypatch):
        """Default format should be JSON."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        # Should output valid JSON (empty array since no entities)
        assert result.returncode == 0


class TestEntityQueryWithEmptyTable:
    """Tests when entities table is empty."""

    def test_returns_empty_list(self, temp_dir, entity_db, monkeypatch):
        """Empty table should return empty list."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output == []


class TestEntityQueryWithoutTable:
    """Tests when entities table doesn't exist."""

    def test_handles_missing_table(self, temp_dir, test_db, monkeypatch):
        """Should handle missing entities table gracefully."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(test_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output.get("message") == "Entities table not yet populated"


class TestEntityQueryFiltering:
    """Tests for query filtering."""

    def test_filter_by_name(self, temp_dir, populated_entity_db, monkeypatch):
        """Should filter by entity name (partial match)."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--name", "User"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 2  # UserAuthentication and UserModel
        for entity in output:
            assert "User" in entity["name"]

    def test_filter_by_entity_type(self, temp_dir, populated_entity_db, monkeypatch):
        """Should filter by entity type."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-type", "function"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 2  # authenticate and validate_token
        for entity in output:
            assert entity["entity_type"] == "function"

    def test_filter_by_source_id(self, temp_dir, populated_entity_db, monkeypatch):
        """Should filter by source event ID."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--source-id", "memory-001"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 3
        for entity in output:
            assert entity["source_event_id"] == "memory-001"

    def test_filter_by_category_id(self, temp_dir, populated_entity_db, monkeypatch):
        """Should filter by category ID."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--category-id", "cat-auth"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 3
        for entity in output:
            assert entity["category_id"] == "cat-auth"

    def test_combined_filters(self, temp_dir, populated_entity_db, monkeypatch):
        """Should combine multiple filters."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-type",
                "function",
                "--category-id",
                "cat-auth",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 2
        for entity in output:
            assert entity["entity_type"] == "function"
            assert entity["category_id"] == "cat-auth"


class TestEntityQueryLimit:
    """Tests for result limiting."""

    def test_default_limit(self, temp_dir, populated_entity_db, monkeypatch):
        """Default limit should be 100."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 5  # All entities (less than limit)

    def test_custom_limit(self, temp_dir, populated_entity_db, monkeypatch):
        """Should respect custom limit."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--limit", "2"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 2


class TestEntityQueryOutput:
    """Tests for output formatting."""

    def test_json_output_structure(self, temp_dir, populated_entity_db, monkeypatch):
        """JSON output should have correct structure."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--limit", "1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) == 1
        entity = output[0]
        assert "id" in entity
        assert "name" in entity
        assert "entity_type" in entity
        assert "description" in entity
        assert "source_event_id" in entity
        assert "category_id" in entity
        assert "properties" in entity
        assert "created_at" in entity

    def test_properties_parsed_as_json(
        self, temp_dir, populated_entity_db, monkeypatch
    ):
        """Properties field should be parsed as JSON."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--name", "authenticate"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert len(output) >= 1
        entity = output[0]
        assert isinstance(entity["properties"], dict)
        assert "params" in entity["properties"]

    def test_table_format(self, temp_dir, populated_entity_db, monkeypatch):
        """Table format should produce readable output."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--format", "table"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        assert "ID" in result.stdout
        assert "Name" in result.stdout
        assert "Type" in result.stdout
        assert "Total:" in result.stdout


class TestEntityQueryErrors:
    """Tests for error handling."""

    def test_invalid_format(self, temp_dir):
        """Invalid format should show error."""
        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script), "--format", "invalid"],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode != 0


class TestEntityQueryOrdering:
    """Tests for result ordering."""

    def test_ordered_by_created_at_desc(
        self, temp_dir, populated_entity_db, monkeypatch
    ):
        """Results should be ordered by created_at descending."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_entity_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-query.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # Latest created should be first
        dates = [e["created_at"] for e in output]
        assert dates == sorted(dates, reverse=True)
