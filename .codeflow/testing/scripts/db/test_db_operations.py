"""
test_db_operations.py - Tests for db_operations.py module.
"""

from pathlib import Path

from db_operations import DatabaseConfig, DatabaseOperations, get_db_operations


class TestDatabaseConfig:
    """Tests for DatabaseConfig dataclass."""

    def test_default_timeout(self):
        """Should have default timeout of 5.0."""
        config = DatabaseConfig()
        assert config.timeout == 5.0

    def test_default_retries(self):
        """Should have default retries of 3."""
        config = DatabaseConfig()
        assert config.retries == 3

    def test_default_retry_delay(self):
        """Should have default retry delay of 0.5."""
        config = DatabaseConfig()
        assert config.retry_delay == 0.5

    def test_default_wal_enabled(self):
        """Should have WAL enabled by default."""
        config = DatabaseConfig()
        assert config.enable_wal is True

    def test_default_foreign_keys_enabled(self):
        """Should have foreign keys enabled by default."""
        config = DatabaseConfig()
        assert config.enable_foreign_keys is True

    def test_custom_db_path(self, temp_dir):
        """Should accept custom db path."""
        db_path = str(temp_dir / "custom.db")
        config = DatabaseConfig(db_path=db_path)
        assert config.db_path == db_path

    def test_custom_timeout(self):
        """Should accept custom timeout."""
        config = DatabaseConfig(timeout=10.0)
        assert config.timeout == 10.0


class TestDatabaseOperationsInit:
    """Tests for DatabaseOperations initialization."""

    def test_init_with_default_config(self):
        """Should initialize with default config."""
        ops = DatabaseOperations()
        assert ops.config is not None
        assert isinstance(ops.config, DatabaseConfig)

    def test_init_with_custom_config(self, temp_dir):
        """Should accept custom config."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)
        assert ops.config.db_path == db_path

    def test_creates_directories(self, temp_dir):
        """Should create required directories."""
        db_path = str(temp_dir / "nested" / "dir" / "test.db")
        log_path = str(temp_dir / "logs" / "ops.jsonl")
        config = DatabaseConfig(db_path=db_path, op_log_path=log_path)
        _ops = DatabaseOperations(config)  # noqa: F841 - side effect creates dirs

        assert Path(db_path).parent.exists()
        assert Path(log_path).parent.exists()


class TestDatabaseConnection:
    """Tests for database connection."""

    def test_connection_creates_db(self, temp_dir):
        """Connection should create database file."""
        db_path = str(temp_dir / "new.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("SELECT 1")

        assert Path(db_path).exists()

    def test_connection_uses_row_factory(self, temp_dir):
        """Connection should use Row factory."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE test (id INTEGER, name TEXT)")
            conn.execute("INSERT INTO test VALUES (1, 'test')")
            cursor = conn.execute("SELECT * FROM test")
            row = cursor.fetchone()
            # Row factory allows dict-like access
            assert row["id"] == 1
            assert row["name"] == "test"

    def test_connection_enables_wal(self, temp_dir):
        """Connection should enable WAL mode."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_wal=True)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            cursor = conn.execute("PRAGMA journal_mode")
            mode = cursor.fetchone()[0]
            assert mode == "wal"

    def test_connection_enables_foreign_keys(self, temp_dir):
        """Connection should enable foreign keys."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path, enable_foreign_keys=True)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            cursor = conn.execute("PRAGMA foreign_keys")
            enabled = cursor.fetchone()[0]
            assert enabled == 1


class TestExecuteQuery:
    """Tests for execute_query method."""

    def test_returns_list_of_dicts(self, temp_dir):
        """Should return list of dictionaries."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, value TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'a'), (2, 'b')")
            conn.commit()

        results = ops.execute_query("SELECT * FROM items ORDER BY id")
        assert isinstance(results, list)
        assert len(results) == 2
        assert results[0]["id"] == 1
        assert results[0]["value"] == "a"

    def test_empty_result(self, temp_dir):
        """Should return empty list for no matches."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        results = ops.execute_query("SELECT * FROM items")
        assert results == []

    def test_with_named_parameters(self, temp_dir):
        """Should support named parameters."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, value INTEGER)")
            conn.execute("INSERT INTO items VALUES (1, 10), (2, 20), (3, 30)")
            conn.commit()

        results = ops.execute_query(
            "SELECT * FROM items WHERE value > :min_val", {"min_val": 15}
        )
        assert len(results) == 2


class TestExecuteWrite:
    """Tests for execute_write method."""

    def test_insert_returns_rowcount(self, temp_dir):
        """INSERT should return 1 for single row."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, name TEXT)")
            conn.commit()

        count = ops.execute_write("INSERT INTO items VALUES (1, 'test')")
        assert count == 1

    def test_update_returns_affected_rows(self, temp_dir):
        """UPDATE should return number of affected rows."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, value INTEGER)")
            conn.execute("INSERT INTO items VALUES (1, 10), (2, 20), (3, 30)")
            conn.commit()

        count = ops.execute_write("UPDATE items SET value = 99 WHERE value < 25")
        assert count == 2


class TestTransaction:
    """Tests for transaction context manager."""

    def test_commits_on_success(self, temp_dir):
        """Transaction should commit on success."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        with ops.transaction() as conn:
            conn.execute("INSERT INTO items VALUES (1)")
            conn.execute("INSERT INTO items VALUES (2)")

        results = ops.execute_query("SELECT COUNT(*) as cnt FROM items")
        assert results[0]["cnt"] == 2

    def test_rollback_on_error(self, temp_dir):
        """Transaction should rollback on error."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER PRIMARY KEY)")
            conn.commit()

        try:
            with ops.transaction() as conn:
                conn.execute("INSERT INTO items VALUES (1)")
                conn.execute("INSERT INTO items VALUES (1)")  # Duplicate, should fail
        except Exception:
            pass

        results = ops.execute_query("SELECT COUNT(*) as cnt FROM items")
        assert results[0]["cnt"] == 0


class TestUtilityMethods:
    """Tests for utility methods."""

    def test_get_value(self, temp_dir):
        """get_value should return single value."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, value TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'hello')")
            conn.commit()

        result = ops.get_value("SELECT value FROM items WHERE id = 1")
        assert result == "hello"

    def test_get_value_default(self, temp_dir):
        """get_value should return default for no results."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        result = ops.get_value("SELECT id FROM items WHERE id = 999", default="default")
        assert result == "default"

    def test_row_exists(self, temp_dir):
        """row_exists should check for row existence."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, name TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'test')")
            conn.commit()

        assert ops.row_exists("items", {"id": 1})
        assert not ops.row_exists("items", {"id": 999})

    def test_table_exists(self, temp_dir):
        """table_exists should check for table existence."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE existing_table (id INTEGER)")
            conn.commit()

        assert ops.table_exists("existing_table")
        assert not ops.table_exists("nonexistent_table")

    def test_get_table_count(self, temp_dir):
        """get_table_count should return row count."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.execute("INSERT INTO items VALUES (1), (2), (3)")
            conn.commit()

        count = ops.get_table_count("items")
        assert count == 3


class TestHealthMethods:
    """Tests for health check methods."""

    def test_check_integrity(self, temp_dir):
        """check_integrity should return True for healthy db."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        assert ops.check_integrity()

    def test_get_db_size(self, temp_dir):
        """get_db_size should return file size."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        size = ops.get_db_size()
        assert size > 0


class TestSingleton:
    """Tests for get_db_operations singleton."""

    def test_returns_instance(self):
        """Should return DatabaseOperations instance."""
        import db_operations as db_module

        db_module._db_ops = None  # Reset singleton

        ops = get_db_operations()
        assert isinstance(ops, DatabaseOperations)
        db_module._db_ops = None

    def test_singleton_behavior(self):
        """Should return same instance on repeated calls."""
        import db_operations as db_module

        db_module._db_ops = None  # Reset singleton

        ops1 = get_db_operations()
        ops2 = get_db_operations()
        assert ops1 is ops2
        db_module._db_ops = None
