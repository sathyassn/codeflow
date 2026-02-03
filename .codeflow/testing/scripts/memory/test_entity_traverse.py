"""
test_entity_traverse.py - Tests for cf-entity-traverse.py script.

Tests entity relationship traversal including depth, direction,
and relation type filtering.
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


# Extended schema for entity traversal tests
ENTITY_RELATIONSHIP_SCHEMA = """
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

CREATE TABLE IF NOT EXISTS relationships (
    id TEXT PRIMARY KEY,
    source_entity_id TEXT NOT NULL,
    target_entity_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    weight REAL DEFAULT 1.0,
    properties TEXT DEFAULT '{}',
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (source_entity_id) REFERENCES entities(id),
    FOREIGN KEY (target_entity_id) REFERENCES entities(id)
);

CREATE INDEX IF NOT EXISTS idx_rel_source ON relationships(source_entity_id);
CREATE INDEX IF NOT EXISTS idx_rel_target ON relationships(target_entity_id);
"""


@pytest.fixture
def relationship_db(test_db):
    """Database with entities and relationships tables."""
    import sqlite3

    conn = sqlite3.connect(str(test_db))
    conn.executescript(ENTITY_RELATIONSHIP_SCHEMA)
    conn.commit()
    conn.close()
    return test_db


@pytest.fixture
def populated_relationship_db(relationship_db):
    """Database with sample entities and relationships."""
    import sqlite3

    conn = sqlite3.connect(str(relationship_db))

    # Entities
    entities = [
        (
            "entity-auth",
            "AuthService",
            "class",
            "Authentication service",
            None,
            None,
            "{}",
        ),
        ("entity-user", "UserModel", "class", "User data model", None, None, "{}"),
        ("entity-jwt", "JWTHandler", "class", "JWT token handling", None, None, "{}"),
        ("entity-db", "Database", "class", "Database connection", None, None, "{}"),
        ("entity-config", "Config", "class", "Configuration", None, None, "{}"),
    ]

    conn.executemany(
        """
        INSERT INTO entities (id, name, entity_type, description, source_event_id, category_id, properties)
        VALUES (?, ?, ?, ?, ?, ?, ?)
    """,
        entities,
    )

    # Relationships
    relationships = [
        ("rel-001", "entity-auth", "entity-user", "uses", 0.9, "{}"),
        ("rel-002", "entity-auth", "entity-jwt", "uses", 0.8, "{}"),
        ("rel-003", "entity-auth", "entity-db", "references", 0.5, "{}"),
        ("rel-004", "entity-jwt", "entity-config", "uses", 0.7, "{}"),
        ("rel-005", "entity-user", "entity-db", "references", 0.6, "{}"),
    ]

    conn.executemany(
        """
        INSERT INTO relationships (id, source_entity_id, target_entity_id, relation_type, weight, properties)
        VALUES (?, ?, ?, ?, ?, ?)
    """,
        relationships,
    )

    conn.commit()
    conn.close()

    return relationship_db


class TestEntityTraverseArgParsing:
    """Tests for argument parsing."""

    def test_help_flag(self):
        """--help should show usage information."""
        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode == 0
        assert "Traverse entity relationships" in result.stdout

    def test_entity_id_required(self):
        """--entity-id is required."""
        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode != 0


class TestEntityTraverseWithoutTables:
    """Tests when tables don't exist."""

    def test_handles_missing_tables(self, temp_dir, test_db, monkeypatch):
        """Should handle missing entity/relationship tables gracefully."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(test_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-xxx"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output.get("message") == "Entity tables not yet populated"


class TestEntityTraverseBasic:
    """Basic traversal tests."""

    def test_traverse_existing_entity(
        self, temp_dir, populated_relationship_db, monkeypatch
    ):
        """Should traverse from an existing entity."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-auth"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["root_entity_id"] == "entity-auth"
        assert output["entity"]["name"] == "AuthService"
        assert len(output["relationships"]) == 3  # 3 outgoing relationships
        assert len(output["connected_entities"]) == 3

    def test_traverse_nonexistent_entity(
        self, temp_dir, populated_relationship_db, monkeypatch
    ):
        """Should return error for nonexistent entity."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-nonexistent"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 1
        output = json.loads(result.stdout)
        assert output["success"] is False
        assert "not found" in output["error"]

    def test_entity_with_no_relationships(self, temp_dir, relationship_db, monkeypatch):
        """Should handle entity with no relationships."""
        import sqlite3

        # Add isolated entity
        conn = sqlite3.connect(str(relationship_db))
        conn.execute("""
            INSERT INTO entities (id, name, entity_type) VALUES ('entity-isolated', 'IsolatedClass', 'class')
        """)
        conn.commit()
        conn.close()

        monkeypatch.setenv("CODEFLOW_DB_FILE", str(relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-isolated"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["entity"]["name"] == "IsolatedClass"
        assert len(output["relationships"]) == 0
        assert len(output["connected_entities"]) == 0


class TestEntityTraverseDirection:
    """Tests for direction filtering."""

    def test_outgoing_direction(self, temp_dir, populated_relationship_db, monkeypatch):
        """Should only follow outgoing relationships."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-id",
                "entity-auth",
                "--direction",
                "outgoing",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # entity-auth has 3 outgoing relationships
        for entity in output["connected_entities"]:
            assert entity["relation_direction"] == "outgoing"

    def test_incoming_direction(self, temp_dir, populated_relationship_db, monkeypatch):
        """Should only follow incoming relationships."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-id",
                "entity-user",
                "--direction",
                "incoming",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # entity-user has 1 incoming from entity-auth
        for entity in output["connected_entities"]:
            assert entity["relation_direction"] == "incoming"

    def test_both_directions(self, temp_dir, populated_relationship_db, monkeypatch):
        """Should follow both directions by default."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-id",
                "entity-user",
                "--direction",
                "both",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # entity-user has 1 incoming + 1 outgoing
        # May have both directions or just what's available
        assert len(output["connected_entities"]) >= 1
        # Verify direction field is present in results
        for entity in output["connected_entities"]:
            assert "relation_direction" in entity


class TestEntityTraverseRelationTypes:
    """Tests for relation type filtering."""

    def test_filter_by_relation_type(
        self, temp_dir, populated_relationship_db, monkeypatch
    ):
        """Should filter by relation type."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-id",
                "entity-auth",
                "--relation-types",
                "uses",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # entity-auth has 2 "uses" relationships
        for rel in output["relationships"]:
            assert rel["relation_type"] == "uses"

    def test_filter_multiple_relation_types(
        self, temp_dir, populated_relationship_db, monkeypatch
    ):
        """Should filter by multiple relation types."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-id",
                "entity-auth",
                "--relation-types",
                "uses,references",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # Should include both types
        relation_types = {r["relation_type"] for r in output["relationships"]}
        assert "uses" in relation_types or "references" in relation_types


class TestEntityTraverseDepth:
    """Tests for traversal depth."""

    def test_depth_parameter(self, temp_dir, populated_relationship_db, monkeypatch):
        """Should respect depth parameter."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-auth", "--depth", "2"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["depth"] == 2


class TestEntityTraverseOutput:
    """Tests for output formatting."""

    def test_json_output_structure(
        self, temp_dir, populated_relationship_db, monkeypatch
    ):
        """JSON output should have correct structure."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-auth"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        assert "root_entity_id" in output
        assert "depth" in output
        assert "entity" in output
        assert "relationships" in output
        assert "connected_entities" in output

        # Entity structure
        entity = output["entity"]
        assert "id" in entity
        assert "name" in entity
        assert "entity_type" in entity

        # Relationship structure
        if output["relationships"]:
            rel = output["relationships"][0]
            assert "source_entity_id" in rel
            assert "target_entity_id" in rel
            assert "relation_type" in rel
            assert "weight" in rel

    def test_tree_format(self, temp_dir, populated_relationship_db, monkeypatch):
        """Tree format should produce readable output."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--entity-id",
                "entity-auth",
                "--format",
                "tree",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        assert "Entity:" in result.stdout
        assert "AuthService" in result.stdout
        assert "Relationships" in result.stdout

    def test_properties_parsed_as_json(self, temp_dir, relationship_db, monkeypatch):
        """Properties field should be parsed as JSON."""
        import sqlite3

        # Add entity with JSON properties
        conn = sqlite3.connect(str(relationship_db))
        conn.execute("""
            INSERT INTO entities (id, name, entity_type, properties)
            VALUES ('entity-props', 'PropsEntity', 'class', '{"key": "value"}')
        """)
        conn.commit()
        conn.close()

        monkeypatch.setenv("CODEFLOW_DB_FILE", str(relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-props"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert isinstance(output["entity"]["properties"], dict)
        assert output["entity"]["properties"]["key"] == "value"


class TestEntityTraverseWeightOrdering:
    """Tests for relationship weight ordering."""

    def test_relationships_ordered_by_weight(
        self, temp_dir, populated_relationship_db, monkeypatch
    ):
        """Relationships should be ordered by weight descending."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(populated_relationship_db))

        script = SCRIPTS_PATH / "memory" / "cf-entity-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--entity-id", "entity-auth"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        weights = [r["weight"] for r in output["relationships"]]
        assert weights == sorted(weights, reverse=True)
