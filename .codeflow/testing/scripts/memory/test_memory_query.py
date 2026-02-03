"""
test_memory_query.py - Tests for cf-memory-query.py
"""

import json


class TestMemoryQueryByDomain:
    """Tests for querying by domain."""

    def test_query_by_domain(self, mock_db_ops):
        """Should filter by domain."""
        # Insert test data
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-dev", "progress", "development", '{"content": "dev"}'),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-qa", "progress", "qa", '{"content": "qa"}'),
            )
            conn.commit()

        # Query by domain
        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE domain = :domain",
            {"domain": "development"},
        )
        assert len(results) == 1
        assert results[0]["id"] == "mem-dev"

    def test_query_all_domains(self, mock_db_ops):
        """Should return all events without domain filter."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-1", "progress", "development", "{}"),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-2", "progress", "qa", "{}"),
            )
            conn.commit()

        results = mock_db_ops.execute_query("SELECT * FROM memory_events")
        assert len(results) == 2


class TestMemoryQueryByEventType:
    """Tests for querying by event type."""

    def test_query_by_event_type(self, mock_db_ops):
        """Should filter by event type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-progress", "progress", "development", "{}"),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-decision", "decision", "development", "{}"),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE event_type = :event_type",
            {"event_type": "progress"},
        )
        assert len(results) == 1
        assert results[0]["id"] == "mem-progress"


class TestMemoryQueryByWorkId:
    """Tests for querying by work ID."""

    def test_query_by_work_id(self, mock_db_ops):
        """Should filter by work ID."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, work_id, data)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-1", "progress", "development", "TSK-001", "{}"),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, work_id, data)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-2", "progress", "development", "TSK-002", "{}"),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, work_id, data)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-3", "progress", "development", "TSK-001", "{}"),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE work_id = :work_id",
            {"work_id": "TSK-001"},
        )
        assert len(results) == 2


class TestMemoryQueryByMemoryType:
    """Tests for querying by memory type."""

    def test_query_by_memory_type(self, mock_db_ops):
        """Should filter by memory type."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data, memory_type)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-ep", "progress", "development", "{}", "episodic"),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data, memory_type)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-sem", "decision", "development", "{}", "semantic"),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE memory_type = :memory_type",
            {"memory_type": "episodic"},
        )
        assert len(results) == 1
        assert results[0]["id"] == "mem-ep"


class TestMemoryQueryLimit:
    """Tests for query limit."""

    def test_respects_limit(self, mock_db_ops):
        """Should respect limit parameter."""
        with mock_db_ops.connection() as conn:
            for i in range(10):
                conn.execute(
                    """
                    INSERT INTO memory_events (id, event_type, domain, data)
                    VALUES (?, ?, ?, ?)
                """,
                    (f"mem-{i}", "progress", "development", "{}"),
                )
            conn.commit()

        results = mock_db_ops.execute_query("SELECT * FROM memory_events LIMIT 5")
        assert len(results) == 5


class TestMemoryQueryOrdering:
    """Tests for query ordering."""

    def test_orders_by_created_at(self, mock_db_ops):
        """Should order by created_at desc."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data, created_at)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-old", "progress", "development", "{}", "2024-01-01T00:00:00"),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data, created_at)
                VALUES (?, ?, ?, ?, ?)
            """,
                ("mem-new", "progress", "development", "{}", "2024-12-01T00:00:00"),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events ORDER BY created_at DESC"
        )
        assert results[0]["id"] == "mem-new"
        assert results[1]["id"] == "mem-old"


class TestMemoryQueryCombinedFilters:
    """Tests for combined query filters."""

    def test_multiple_filters(self, mock_db_ops):
        """Should handle multiple filters."""
        with mock_db_ops.connection() as conn:
            # Insert various events
            events = [
                ("mem-1", "progress", "development", "TSK-001", "{}"),
                ("mem-2", "progress", "qa", "TSK-001", "{}"),
                ("mem-3", "decision", "development", "TSK-001", "{}"),
                ("mem-4", "progress", "development", "TSK-002", "{}"),
            ]
            for e in events:
                conn.execute(
                    """
                    INSERT INTO memory_events (id, event_type, domain, work_id, data)
                    VALUES (?, ?, ?, ?, ?)
                """,
                    e,
                )
            conn.commit()

        # Query with multiple filters
        results = mock_db_ops.execute_query(
            """SELECT * FROM memory_events
               WHERE domain = :domain AND event_type = :event_type AND work_id = :work_id""",
            {"domain": "development", "event_type": "progress", "work_id": "TSK-001"},
        )
        assert len(results) == 1
        assert results[0]["id"] == "mem-1"


class TestMemoryQueryDataParsing:
    """Tests for data field parsing."""

    def test_returns_json_data(self, mock_db_ops):
        """Should return data as JSON."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-data",
                    "progress",
                    "development",
                    '{"content": "test", "details": {"key": "value"}}',
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM memory_events WHERE id = 'mem-data'"
        )
        data = json.loads(results[0]["data"])
        assert data["content"] == "test"
        assert data["details"]["key"] == "value"
