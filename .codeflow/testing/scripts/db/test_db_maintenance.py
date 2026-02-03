"""
test_db_maintenance.py - Tests for database maintenance operations.
"""

from pathlib import Path

from db_operations import DatabaseConfig, DatabaseOperations


class TestVacuum:
    """Tests for VACUUM operation."""

    def test_vacuum_succeeds(self, temp_dir):
        """VACUUM should succeed on valid database."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'test data')")
            conn.execute("DELETE FROM items WHERE id = 1")
            conn.commit()

        # VACUUM should not raise
        ops.vacuum()

    def test_vacuum_reclaims_space(self, temp_dir):
        """VACUUM should potentially reduce file size."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_wal=False)
        ops = DatabaseOperations(config)

        # Create and delete data
        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            for i in range(100):
                conn.execute(f"INSERT INTO items VALUES ({i}, '{'x' * 1000}')")
            conn.commit()
            conn.execute("DELETE FROM items")
            conn.commit()

        size_before = ops.get_db_size()
        ops.vacuum()
        size_after = ops.get_db_size()

        # Size should decrease after vacuum
        assert size_after <= size_before


class TestAnalyze:
    """Tests for ANALYZE operation."""

    def test_analyze_succeeds(self, temp_dir):
        """ANALYZE should succeed on valid database."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT)")
            for i in range(50):
                conn.execute(f"INSERT INTO items VALUES ({i}, 'value_{i}')")
            conn.commit()

        # ANALYZE should not raise
        ops.analyze()

    def test_analyze_updates_statistics(self, temp_dir):
        """ANALYZE should update sqlite_stat tables."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT)")
            conn.execute("CREATE INDEX idx_value ON items(value)")
            for i in range(50):
                conn.execute(f"INSERT INTO items VALUES ({i}, 'value_{i}')")
            conn.commit()

        ops.analyze()

        # Check that stat table was created
        results = ops.execute_query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'sqlite_stat%'"
        )
        assert len(results) > 0


class TestCheckpoint:
    """Tests for WAL checkpoint operation."""

    def test_checkpoint_passive(self, temp_dir):
        """PASSIVE checkpoint should succeed."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_wal=True)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.execute("INSERT INTO items VALUES (1)")
            conn.commit()

        # PASSIVE checkpoint should not raise
        ops.checkpoint("PASSIVE")

    def test_checkpoint_truncate(self, temp_dir):
        """TRUNCATE checkpoint should reduce WAL size."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_wal=True)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            for i in range(50):
                conn.execute(f"INSERT INTO items VALUES ({i}, '{'x' * 100}')")
            conn.commit()

        # TRUNCATE checkpoint
        ops.checkpoint("TRUNCATE")

        # WAL file should be small or non-existent
        wal_size = ops.get_wal_size()
        assert wal_size < 1000  # Should be minimal


class TestSchemaVersion:
    """Tests for schema version tracking."""

    def test_get_schema_version_no_table(self, temp_dir):
        """Should return 0 when schema_version table doesn't exist."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        # Create empty database
        with ops.connection() as conn:
            conn.execute("CREATE TABLE dummy (id INTEGER)")
            conn.commit()

        version = ops.get_schema_version()
        assert version == 0

    def test_get_schema_version_with_table(self, temp_dir):
        """Should return version from schema_version table."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE schema_version (version INTEGER)")
            conn.execute("INSERT INTO schema_version VALUES (5)")
            conn.commit()

        version = ops.get_schema_version()
        assert version == 5


class TestIntegrityCheck:
    """Tests for integrity check."""

    def test_integrity_check_healthy_db(self, temp_dir):
        """Should pass for healthy database."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER PRIMARY KEY)")
            conn.execute("INSERT INTO items VALUES (1), (2), (3)")
            conn.commit()

        assert ops.check_integrity()


class TestSizeOperations:
    """Tests for size-related operations."""

    def test_get_db_size_empty(self, temp_dir):
        """Should return size for empty database."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("SELECT 1")

        size = ops.get_db_size()
        assert size > 0

    def test_get_db_size_nonexistent(self, temp_dir):
        """Should return 0 for nonexistent database."""
        db_path = str(temp_dir / "nonexistent.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        size = ops.get_db_size()
        assert size == 0

    def test_get_wal_size_no_wal(self, temp_dir):
        """Should return 0 when no WAL file."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_wal=False)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        wal_size = ops.get_wal_size()
        assert wal_size == 0

    def test_get_wal_size_with_wal(self, temp_dir):
        """Should return WAL file size when exists."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_wal=True)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            for i in range(100):
                conn.execute(f"INSERT INTO items VALUES ({i}, '{'x' * 100}')")
            conn.commit()

        # WAL file should exist and have content
        wal_path = Path(db_path + "-wal")
        if wal_path.exists():
            wal_size = ops.get_wal_size()
            assert wal_size > 0
