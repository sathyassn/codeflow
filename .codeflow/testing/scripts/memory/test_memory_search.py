"""
test_memory_search.py - Tests for cf-memory-search.py (FTS5)
"""


class TestMemorySearchBasic:
    """Tests for basic FTS search."""

    def test_search_finds_match(self, mock_db_ops):
        """Should find matching events."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-auth",
                    "progress",
                    "development",
                    '{"content": "Implemented user authentication system"}',
                ),
            )
            conn.commit()

        # Search using FTS
        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'authentication'
        """)
        assert len(results) == 1
        assert results[0]["id"] == "mem-auth"

    def test_search_no_match(self, mock_db_ops):
        """Should return empty for no matches."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-1", "progress", "development", '{"content": "hello world"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'nonexistent'
        """)
        assert len(results) == 0


class TestMemorySearchPhrases:
    """Tests for phrase search."""

    def test_phrase_search(self, mock_db_ops):
        """Should support phrase search."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "user authentication system"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-2",
                    "progress",
                    "development",
                    '{"content": "user system authentication"}',
                ),
            )
            conn.commit()

        # Exact phrase should match only mem-1
        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH '"user authentication"'
        """)
        assert len(results) == 1
        assert results[0]["id"] == "mem-1"


class TestMemorySearchPrefix:
    """Tests for prefix search."""

    def test_prefix_search(self, mock_db_ops):
        """Should support prefix search."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "implementing new feature"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-2",
                    "progress",
                    "development",
                    '{"content": "implementation complete"}',
                ),
            )
            conn.commit()

        # Prefix search
        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'impl*'
        """)
        assert len(results) == 2


class TestMemorySearchBoolean:
    """Tests for boolean search operators."""

    def test_and_search(self, mock_db_ops):
        """Should support AND operator."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "user authentication complete"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-2",
                    "progress",
                    "development",
                    '{"content": "user profile updated"}',
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'user AND authentication'
        """)
        assert len(results) == 1
        assert results[0]["id"] == "mem-1"

    def test_or_search(self, mock_db_ops):
        """Should support OR operator."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "authentication system"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-2",
                    "progress",
                    "development",
                    '{"content": "authorization rules"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-3", "progress", "development", '{"content": "database queries"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'authentication OR authorization'
        """)
        assert len(results) == 2

    def test_not_search(self, mock_db_ops):
        """Should support NOT operator."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "user authentication"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-2", "progress", "development", '{"content": "user profile"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'user NOT authentication'
        """)
        assert len(results) == 1
        assert results[0]["id"] == "mem-2"


class TestMemorySearchWithDomainFilter:
    """Tests for search with domain filter."""

    def test_search_with_domain(self, mock_db_ops):
        """Should filter search by domain."""
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "authentication feature"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-2", "progress", "qa", '{"content": "authentication tests"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            """
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'authentication' AND m.domain = :domain
        """,
            {"domain": "development"},
        )
        assert len(results) == 1
        assert results[0]["id"] == "mem-1"


class TestMemorySearchRanking:
    """Tests for search result ranking."""

    def test_returns_ranked_results(self, mock_db_ops):
        """Should return results with ranking scores."""
        with mock_db_ops.connection() as conn:
            # Insert events with different relevance
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                (
                    "mem-1",
                    "progress",
                    "development",
                    '{"content": "authentication authentication authentication"}',
                ),
            )
            conn.execute(
                """
                INSERT INTO memory_events (id, event_type, domain, data)
                VALUES (?, ?, ?, ?)
            """,
                ("mem-2", "progress", "development", '{"content": "authentication"}'),
            )
            conn.commit()

        results = mock_db_ops.execute_query("""
            SELECT m.*, bm25(memory_fts) as score
            FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'authentication'
            ORDER BY score
        """)
        assert len(results) == 2
        # Check that both have scores (bm25 returns negative, lower is better)
        assert "score" in results[0]
        assert "score" in results[1]


class TestMemorySearchLimit:
    """Tests for search result limiting."""

    def test_respects_limit(self, mock_db_ops):
        """Should respect limit parameter."""
        with mock_db_ops.connection() as conn:
            for i in range(10):
                conn.execute(
                    """
                    INSERT INTO memory_events (id, event_type, domain, data)
                    VALUES (?, ?, ?, ?)
                """,
                    (
                        f"mem-{i}",
                        "progress",
                        "development",
                        '{"content": "test content"}',
                    ),
                )
            conn.commit()

        results = mock_db_ops.execute_query("""
            SELECT m.* FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH 'test'
            LIMIT 5
        """)
        assert len(results) == 5
