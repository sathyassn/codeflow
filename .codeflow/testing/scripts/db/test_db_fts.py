"""
test_db_fts.py - Tests for FTS5 operations.
"""

import pytest
from db_operations import DatabaseConfig, DatabaseOperations


@pytest.fixture
def fts_db(temp_dir):
    """Create database with FTS5 table."""
    db_path = str(temp_dir / "fts_test.db")
    config = DatabaseConfig(db_path=db_path)
    ops = DatabaseOperations(config)

    with ops.connection() as conn:
        # Create content table
        conn.execute("""
            CREATE TABLE documents (
                id INTEGER PRIMARY KEY,
                title TEXT,
                content TEXT
            )
        """)

        # Create FTS5 virtual table
        conn.execute("""
            CREATE VIRTUAL TABLE documents_fts USING fts5(
                title,
                content,
                content='documents',
                content_rowid='id'
            )
        """)

        # Create triggers for FTS sync
        conn.execute("""
            CREATE TRIGGER documents_ai AFTER INSERT ON documents BEGIN
                INSERT INTO documents_fts(rowid, title, content)
                VALUES (new.id, new.title, new.content);
            END
        """)

        conn.execute("""
            CREATE TRIGGER documents_ad AFTER DELETE ON documents BEGIN
                INSERT INTO documents_fts(documents_fts, rowid, title, content)
                VALUES ('delete', old.id, old.title, old.content);
            END
        """)

        conn.execute("""
            CREATE TRIGGER documents_au AFTER UPDATE ON documents BEGIN
                INSERT INTO documents_fts(documents_fts, rowid, title, content)
                VALUES ('delete', old.id, old.title, old.content);
                INSERT INTO documents_fts(rowid, title, content)
                VALUES (new.id, new.title, new.content);
            END
        """)

        # Insert test data
        docs = [
            (1, "Python Tutorial", "Learn Python programming from scratch"),
            (2, "JavaScript Guide", "Modern JavaScript development practices"),
            (3, "Database Design", "Best practices for database schema design"),
            (4, "Python Advanced", "Advanced Python patterns and techniques"),
            (5, "Web Development", "Full stack web development with Python and JS"),
        ]
        conn.executemany("INSERT INTO documents VALUES (?, ?, ?)", docs)
        conn.commit()

    return ops


class TestFTSSearch:
    """Tests for FTS5 search functionality."""

    def test_basic_search(self, fts_db):
        """Basic FTS search should work."""
        results = fts_db.execute_query(
            "SELECT title FROM documents_fts WHERE documents_fts MATCH 'Python'"
        )
        assert len(results) == 3  # Python Tutorial, Python Advanced, Web Development

    def test_phrase_search(self, fts_db):
        """Phrase search should work."""
        results = fts_db.execute_query(
            "SELECT title FROM documents_fts WHERE documents_fts MATCH '\"database schema\"'"
        )
        assert len(results) == 1
        assert results[0]["title"] == "Database Design"

    def test_prefix_search(self, fts_db):
        """Prefix search should work."""
        results = fts_db.execute_query(
            "SELECT title FROM documents_fts WHERE documents_fts MATCH 'develop*'"
        )
        # Should match "development"
        assert len(results) >= 2

    def test_boolean_and_search(self, fts_db):
        """AND search should work."""
        results = fts_db.execute_query(
            "SELECT title FROM documents_fts WHERE documents_fts MATCH 'Python AND advanced'"
        )
        assert len(results) == 1
        assert results[0]["title"] == "Python Advanced"

    def test_boolean_or_search(self, fts_db):
        """OR search should work."""
        results = fts_db.execute_query(
            "SELECT title FROM documents_fts WHERE documents_fts MATCH 'Tutorial OR Guide'"
        )
        assert len(results) == 2

    def test_column_specific_search(self, fts_db):
        """Column-specific search should work."""
        results = fts_db.execute_query(
            "SELECT title FROM documents_fts WHERE documents_fts MATCH 'title:Python'"
        )
        assert len(results) == 2  # Python Tutorial, Python Advanced


class TestFTSRebuild:
    """Tests for FTS5 rebuild operation."""

    def test_rebuild_succeeds(self, fts_db):
        """FTS rebuild should succeed."""
        # Should not raise
        fts_db.rebuild_fts("documents_fts")

    def test_rebuild_after_direct_content_change(self, temp_dir):
        """Rebuild should fix FTS after direct content changes."""
        db_path = str(temp_dir / "fts_test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        # Create external content FTS
        with ops.connection() as conn:
            conn.execute("CREATE TABLE docs (id INTEGER PRIMARY KEY, text TEXT)")
            conn.execute("""
                CREATE VIRTUAL TABLE docs_fts USING fts5(
                    text,
                    content='docs',
                    content_rowid='id'
                )
            """)
            conn.execute("INSERT INTO docs VALUES (1, 'hello world')")
            # Manually populate FTS
            conn.execute("INSERT INTO docs_fts(docs_fts) VALUES('rebuild')")
            conn.commit()

        # Verify search works
        results = ops.execute_query(
            "SELECT * FROM docs_fts WHERE docs_fts MATCH 'hello'"
        )
        assert len(results) == 1

        # Now add data without trigger
        with ops.connection() as conn:
            conn.execute("INSERT INTO docs VALUES (2, 'goodbye world')")
            conn.commit()

        # Rebuild FTS
        ops.rebuild_fts("docs_fts")

        # Should now find new content
        results = ops.execute_query(
            "SELECT * FROM docs_fts WHERE docs_fts MATCH 'goodbye'"
        )
        assert len(results) == 1


class TestFTSOptimize:
    """Tests for FTS5 optimize operation."""

    def test_optimize_succeeds(self, fts_db):
        """FTS optimize should succeed."""
        # Should not raise
        fts_db.optimize_fts("documents_fts")

    def test_optimize_after_many_updates(self, temp_dir):
        """Optimize should work after many updates."""
        db_path = str(temp_dir / "fts_test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER PRIMARY KEY, text TEXT)")
            conn.execute("CREATE VIRTUAL TABLE items_fts USING fts5(text)")

            # Many inserts
            for i in range(100):
                conn.execute(f"INSERT INTO items_fts VALUES ('text number {i}')")
            conn.commit()

        # Optimize should not raise
        ops.optimize_fts("items_fts")


class TestFTSHighlight:
    """Tests for FTS5 highlighting."""

    def test_highlight_function(self, fts_db):
        """highlight() function should work."""
        results = fts_db.execute_query("""
            SELECT highlight(documents_fts, 0, '<b>', '</b>') as highlighted
            FROM documents_fts
            WHERE documents_fts MATCH 'Python'
            LIMIT 1
        """)
        assert len(results) == 1
        assert "<b>" in results[0]["highlighted"]
        assert "</b>" in results[0]["highlighted"]

    def test_snippet_function(self, fts_db):
        """snippet() function should work."""
        results = fts_db.execute_query("""
            SELECT snippet(documents_fts, 1, '<b>', '</b>', '...', 10) as snippet
            FROM documents_fts
            WHERE documents_fts MATCH 'programming'
            LIMIT 1
        """)
        assert len(results) == 1


class TestFTSRanking:
    """Tests for FTS5 ranking."""

    def test_bm25_ranking(self, fts_db):
        """bm25() ranking should work."""
        results = fts_db.execute_query("""
            SELECT title, bm25(documents_fts) as rank
            FROM documents_fts
            WHERE documents_fts MATCH 'Python'
            ORDER BY rank
        """)
        assert len(results) == 3
        # Results should be ordered by rank
        ranks = [r["rank"] for r in results]
        assert ranks == sorted(ranks)
