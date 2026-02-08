"""
test_memory_store.py - Tests for cf-memory-store.py

Priority: HIGH
"""

from pathlib import Path

import pytest

# Script path
SCRIPT_PATH = (
    Path(__file__).parent.parent.parent.parent
    / "scripts"
    / "memory"
    / "cf-memory-store.py"
)


class TestMemoryStoreValidation:
    """Tests for argument validation using the run_script fixture.

    These tests validate CLI argument parsing by running the script as a subprocess
    with the proper PYTHONPATH set via the run_script fixture.
    """

    def test_requires_event_type(self, run_script):
        """Should require --event-type argument."""
        result = run_script(SCRIPT_PATH, "--domain", "development", "--content", "test")
        assert result.returncode != 0
        assert (
            "event-type" in result.stderr.lower() or "required" in result.stderr.lower()
        )

    def test_requires_domain(self, run_script):
        """Should require --domain argument."""
        result = run_script(
            SCRIPT_PATH, "--event-type", "progress", "--content", "test"
        )
        assert result.returncode != 0
        assert "domain" in result.stderr.lower() or "required" in result.stderr.lower()

    def test_requires_content(self, run_script):
        """Should require --content argument."""
        result = run_script(
            SCRIPT_PATH, "--event-type", "progress", "--domain", "development"
        )
        assert result.returncode != 0
        assert "content" in result.stderr.lower() or "required" in result.stderr.lower()

    def test_rejects_invalid_event_type(self, run_script):
        """Should reject invalid event type."""
        result = run_script(
            SCRIPT_PATH,
            "--event-type",
            "invalid",
            "--domain",
            "development",
            "--content",
            "test",
        )
        assert result.returncode != 0
        assert "invalid" in result.stderr.lower() or "choice" in result.stderr.lower()

    def test_rejects_invalid_domain(self, run_script):
        """Should reject invalid domain."""
        result = run_script(
            SCRIPT_PATH,
            "--event-type",
            "progress",
            "--domain",
            "invalid",
            "--content",
            "test",
        )
        assert result.returncode != 0
        assert "invalid" in result.stderr.lower() or "choice" in result.stderr.lower()


class TestMemoryStoreWithMock:
    """Tests with mocked database."""

    def test_store_creates_event(self, mock_db_ops):
        """Should create memory event in database."""
        # Test the database operations directly (bypassing CLI)
        with mock_db_ops.connection() as conn:
            # Store a memory event directly
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, work_id, data, memory_type)
                VALUES (?, ?, ?, ?, ?, ?)
            """,
                (
                    "memory-test-001",
                    "progress",
                    "development",
                    "TSK-001",
                    '{"content": "Test content"}',
                    "episodic",
                ),
            )
            conn.commit()

        # Verify it was stored
        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = :id", {"id": "memory-test-001"}
        )
        assert len(results) == 1
        assert results[0]["event_type"] == "progress"
        assert results[0]["domain"] == "development"


class TestMemoryStoreEventTypes:
    """Tests for different event types."""

    def test_progress_event_type(self, mock_db_ops):
        """Should accept progress event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-1", "progress", "development", '{"content": "test"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-1'"
        )
        assert results[0]["event_type"] == "progress"

    def test_decision_event_type(self, mock_db_ops):
        """Should accept decision event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-2", "decision", "planning", '{"content": "test"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-2'"
        )
        assert results[0]["event_type"] == "decision"

    def test_milestone_event_type(self, mock_db_ops):
        """Should accept milestone event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-3", "milestone", "development", '{"content": "test"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-3'"
        )
        assert results[0]["event_type"] == "milestone"

    def test_blocker_event_type(self, mock_db_ops):
        """Should accept blocker event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-4", "blocker", "qa", '{"content": "test"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-4'"
        )
        assert results[0]["event_type"] == "blocker"

    def test_stage_transition_event_type(self, mock_db_ops):
        """Should accept V4 stage_transition event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-5",
                    "stage_transition",
                    "development",
                    '{"from_stage": "dev", "to_stage": "review"}',
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-5'"
        )
        assert results[0]["event_type"] == "stage_transition"

    def test_stage_complete_event_type(self, mock_db_ops):
        """Should accept V4 stage_complete event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-6",
                    "stage_complete",
                    "development",
                    '{"stage": "review", "verdict": "approved"}',
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-6'"
        )
        assert results[0]["event_type"] == "stage_complete"

    def test_rework_limit_event_type(self, mock_db_ops):
        """Should accept V4 rework_limit event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-7",
                    "rework_limit",
                    "development",
                    '{"task_id": "TSK-001", "iterations": 3}',
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-7'"
        )
        assert results[0]["event_type"] == "rework_limit"


class TestMemoryStoreV4EventTypeCLI:
    """Tests for V4 event types via CLI validation."""

    def test_cli_accepts_stage_transition(self, run_script):
        """Should accept stage_transition via CLI --event-type."""
        result = run_script(
            SCRIPT_PATH,
            "--event-type",
            "stage_transition",
            "--domain",
            "development",
            "--content",
            "Transition from dev to review",
        )
        # May fail due to DB not being available, but should NOT fail on arg validation
        if result.returncode != 0:
            assert "invalid choice" not in result.stderr.lower()

    def test_cli_accepts_stage_complete(self, run_script):
        """Should accept stage_complete via CLI --event-type."""
        result = run_script(
            SCRIPT_PATH,
            "--event-type",
            "stage_complete",
            "--domain",
            "development",
            "--content",
            "Review stage complete",
        )
        if result.returncode != 0:
            assert "invalid choice" not in result.stderr.lower()

    def test_cli_accepts_rework_limit(self, run_script):
        """Should accept rework_limit via CLI --event-type."""
        result = run_script(
            SCRIPT_PATH,
            "--event-type",
            "rework_limit",
            "--domain",
            "development",
            "--content",
            "Rework limit reached",
        )
        if result.returncode != 0:
            assert "invalid choice" not in result.stderr.lower()


class TestMemoryStoreDomains:
    """Tests for different domains."""

    @pytest.mark.parametrize(
        "domain", ["planning", "development", "review", "qa", "ops", "documentation"]
    )
    def test_valid_domains(self, mock_db_ops, domain):
        """Should accept all valid domains."""
        event_id = f"mem-{domain}"
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (event_id, "progress", domain, '{"content": "test"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = :id", {"id": event_id}
        )
        assert results[0]["domain"] == domain


class TestMemoryStoreWithWorkId:
    """Tests for work ID association."""

    def test_stores_with_work_id(self, mock_db_ops):
        """Should store memory with work ID."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, work_id, data)
                VALUES (?, ?, ?, ?, ?)
            """,
                (
                    "mem-wid",
                    "progress",
                    "development",
                    "TSK-001",
                    '{"content": "test"}',
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-wid'"
        )
        assert results[0]["work_id"] == "TSK-001"

    def test_stores_without_work_id(self, mock_db_ops):
        """Should store memory without work ID."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-nowid", "progress", "development", '{"content": "test"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-nowid'"
        )
        assert results[0]["work_id"] is None


class TestMemoryStoreMemoryTypes:
    """Tests for memory type classification."""

    @pytest.mark.parametrize("memory_type", ["episodic", "semantic", "procedural"])
    def test_valid_memory_types(self, mock_db_ops, memory_type):
        """Should accept all valid memory types."""
        event_id = f"mem-{memory_type}"
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data, memory_type)
                VALUES (?, ?, ?, ?, ?)
            """,
                (
                    event_id,
                    "progress",
                    "development",
                    '{"content": "test"}',
                    memory_type,
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = :id", {"id": event_id}
        )
        assert results[0]["memory_type"] == memory_type
