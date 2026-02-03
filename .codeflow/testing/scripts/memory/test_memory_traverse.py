"""
test_memory_traverse.py - Tests for cf-memory-traverse.py script.

Tests memory graph traversal including traversal by memory ID,
traversal by work ID, and depth-based exploration.
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


@pytest.fixture
def graph_populated_db(test_db):
    """Database with interconnected memory events."""
    import sqlite3

    conn = sqlite3.connect(str(test_db))

    # Memory events with work ID references
    events = [
        (
            "memory-g1",
            "progress",
            "development",
            "TSK-001",
            '{"content": "Working on TSK-001", "refs": ["TSK-002"]}',
            "2026-01-25T10:00:00Z",
        ),
        (
            "memory-g2",
            "decision",
            "planning",
            "TSK-001",
            '{"content": "Decision for TSK-001 affects TSK-003"}',
            "2026-01-25T10:30:00Z",
        ),
        (
            "memory-g3",
            "progress",
            "development",
            "TSK-002",
            '{"content": "Related to TSK-001", "refs": ["TSK-001"]}',
            "2026-01-25T11:00:00Z",
        ),
        (
            "memory-g4",
            "milestone",
            "development",
            "TSK-001",
            '{"content": "Milestone for TSK-001"}',
            "2026-01-25T11:30:00Z",
        ),
        (
            "memory-g5",
            "progress",
            "qa",
            "TSK-003",
            '{"content": "Testing TSK-003, depends on TSK-001"}',
            "2026-01-25T12:00:00Z",
        ),
        (
            "memory-g6",
            "progress",
            "development",
            "TSK-004",
            '{"content": "Isolated task"}',
            "2026-01-25T09:00:00Z",
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


class TestMemoryTraverseArgParsing:
    """Tests for argument parsing."""

    def test_help_flag(self):
        """--help should show usage information."""
        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--help"],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode == 0
        assert "Traverse memory graph" in result.stdout

    def test_requires_start_id_or_work_id(self):
        """Either --start-id or --work-id is required."""
        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
            env=get_script_env(),
        )
        assert result.returncode != 0
        assert "required" in result.stderr.lower()


class TestMemoryTraverseByMemoryId:
    """Tests for traversal starting from a memory ID."""

    def test_traverse_from_memory_id(self, temp_dir, graph_populated_db, monkeypatch):
        """Should traverse from a specific memory ID."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--start-id", "memory-g1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["root_memory_id"] == "memory-g1"
        assert len(output["memories"]) >= 1
        # Should find related memories by work_id
        assert "related_memories" in output

    def test_traverse_nonexistent_memory(
        self, temp_dir, graph_populated_db, monkeypatch
    ):
        """Should return error for nonexistent memory."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--start-id", "memory-nonexistent"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        # Should return success with error in output
        output = json.loads(result.stdout)
        assert "error" in output

    def test_finds_related_by_work_id(self, temp_dir, graph_populated_db, monkeypatch):
        """Should find related memories by work_id."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--start-id", "memory-g1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        # Should find other memories with same work_id (TSK-001)
        work_ids = set()
        for mem in output["memories"] + output.get("related_memories", []):
            if mem.get("work_id"):
                work_ids.add(mem["work_id"])
        # memory-g1 has work_id TSK-001
        assert "TSK-001" in work_ids

    def test_finds_related_by_temporal_proximity(
        self, temp_dir, graph_populated_db, monkeypatch
    ):
        """Should find related memories by domain and time proximity."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--start-id", "memory-g1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        # Check if temporal relations are marked
        for mem in output.get("related_memories", []):
            # May have temporal_domain relation type
            pass  # Just checking it doesn't crash


class TestMemoryTraverseByWorkId:
    """Tests for traversal by work ID."""

    def test_traverse_by_work_id(self, temp_dir, graph_populated_db, monkeypatch):
        """Should find all memories for a work ID."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["root_work_id"] == "TSK-001"
        # Should find all memories with TSK-001
        assert len(output["memories"]) >= 3  # g1, g2, g4

    def test_work_id_finds_related_work(
        self, temp_dir, graph_populated_db, monkeypatch
    ):
        """Should find related work IDs from memory content."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        # TSK-001 memories reference TSK-002 and TSK-003
        assert "related_work" in output

    def test_isolated_work_id(self, temp_dir, graph_populated_db, monkeypatch):
        """Should handle isolated work ID with no relations."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-004"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["root_work_id"] == "TSK-004"
        assert len(output["memories"]) == 1


class TestMemoryTraverseDepth:
    """Tests for traversal depth."""

    def test_depth_parameter(self, temp_dir, graph_populated_db, monkeypatch):
        """Should respect depth parameter."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001", "--depth", "2"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert output["depth"] == 2

    def test_depth_zero(self, temp_dir, graph_populated_db, monkeypatch):
        """Depth 0 should return only direct matches."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001", "--depth", "0"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        # With depth 0, should only get direct TSK-001 memories
        for mem in output["memories"]:
            # All should be at depth 0
            assert mem.get("traverse_depth", 0) == 0


class TestMemoryTraverseOutput:
    """Tests for output formatting."""

    def test_json_output_structure_by_memory(
        self, temp_dir, graph_populated_db, monkeypatch
    ):
        """JSON output for memory traversal should have correct structure."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--start-id", "memory-g1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        assert "root_memory_id" in output
        assert "depth" in output
        assert "memories" in output
        assert "related_memories" in output

    def test_json_output_structure_by_work(
        self, temp_dir, graph_populated_db, monkeypatch
    ):
        """JSON output for work traversal should have correct structure."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        assert "root_work_id" in output
        assert "depth" in output
        assert "memories" in output
        assert "related_work" in output

    def test_data_parsed_as_json(self, temp_dir, graph_populated_db, monkeypatch):
        """Data field should be parsed as JSON."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001"],
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

    def test_summary_format(self, temp_dir, graph_populated_db, monkeypatch):
        """Summary format should produce readable output."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [
                sys.executable,
                str(script),
                "--work-id",
                "TSK-001",
                "--format",
                "summary",
            ],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        assert "Traversal Results" in result.stdout
        assert "Total memories found" in result.stdout


class TestMemoryTraverseDepthTracking:
    """Tests for traverse depth tracking."""

    def test_memories_have_depth_info(self, temp_dir, graph_populated_db, monkeypatch):
        """Memories should include traverse depth information."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001", "--depth", "1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        for mem in output["memories"]:
            assert "traverse_depth" in mem
            assert mem["traverse_depth"] >= 0


class TestMemoryTraverseOrdering:
    """Tests for result ordering."""

    def test_memories_ordered_by_created_at(
        self, temp_dir, graph_populated_db, monkeypatch
    ):
        """Memories within each work ID should be ordered by created_at."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--work-id", "TSK-001"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        # Filter to just TSK-001 memories (depth 0)
        tsk001_mems = [m for m in output["memories"] if m.get("traverse_depth", 0) == 0]
        dates = [m["created_at"] for m in tsk001_mems]
        assert dates == sorted(dates, reverse=True)


class TestMemoryTraverseNoCycle:
    """Tests for cycle prevention."""

    def test_no_duplicate_memories(self, temp_dir, graph_populated_db, monkeypatch):
        """Should not include duplicate memories."""
        monkeypatch.setenv("CODEFLOW_DB_FILE", str(graph_populated_db))

        script = SCRIPTS_PATH / "memory" / "cf-memory-traverse.py"
        result = subprocess.run(
            [sys.executable, str(script), "--start-id", "memory-g1"],
            capture_output=True,
            text=True,
            env=get_script_env(),
            cwd=str(temp_dir),
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)

        all_memories = output["memories"] + output.get("related_memories", [])
        memory_ids = [m["id"] for m in all_memories]
        assert len(memory_ids) == len(set(memory_ids))  # No duplicates
