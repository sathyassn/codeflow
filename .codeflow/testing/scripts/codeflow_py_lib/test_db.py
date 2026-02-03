"""
test_db.py - Tests for database operations module.

Tests DatabaseOperations, DatabaseConfig, and database helper functions.
Also tests the stub implementation fallback when db_operations is unavailable.
"""

import importlib
import sqlite3
import sys

import pytest
from codeflow_py_lib.db import DatabaseConfig, DatabaseOperations, dict_factory, get_db


@pytest.fixture
def temp_db(tmp_path):
    """Create a temporary database file."""
    db_path = tmp_path / "test.db"
    return db_path


@pytest.fixture
def db_with_table(temp_db):
    """Create database with test table."""
    conn = sqlite3.connect(str(temp_db))
    conn.execute("""
        CREATE TABLE test_table (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            value INTEGER
        )
    """)
    conn.execute("INSERT INTO test_table (name, value) VALUES ('item1', 10)")
    conn.execute("INSERT INTO test_table (name, value) VALUES ('item2', 20)")
    conn.execute("INSERT INTO test_table (name, value) VALUES ('item3', 30)")
    conn.commit()
    conn.close()
    return temp_db


@pytest.fixture
def reset_db_singleton():
    """Reset database singleton between tests."""
    import codeflow_py_lib.db as db_module

    db_module._db_ops = None
    yield
    db_module._db_ops = None


@pytest.fixture
def force_stub_implementation():
    """
    Force the db module to use stub implementation by blocking db_operations import.

    This simulates the scenario where db_operations.py doesn't exist or can't be imported,
    triggering the fallback stub code in db.py.
    """
    # Save original state
    original_modules = {}
    modules_to_remove = [
        "db_operations",
        "codeflow_py_lib.db",
    ]

    for mod in modules_to_remove:
        if mod in sys.modules:
            original_modules[mod] = sys.modules[mod]

    # Block db_operations import by setting it to None
    # This causes ImportError when trying to import it
    sys.modules["db_operations"] = None

    # Remove cached db module so it gets reimported
    if "codeflow_py_lib.db" in sys.modules:
        del sys.modules["codeflow_py_lib.db"]

    # Re-import the db module - this will trigger the fallback
    import codeflow_py_lib.db as stub_db_module

    # Reload to ensure fresh import with blocked db_operations
    importlib.reload(stub_db_module)

    yield stub_db_module

    # Restore original state
    del sys.modules["db_operations"]

    for mod, module in original_modules.items():
        sys.modules[mod] = module

    # Re-import original module
    if "codeflow_py_lib.db" in sys.modules:
        del sys.modules["codeflow_py_lib.db"]
    importlib.reload(importlib.import_module("codeflow_py_lib.db"))


class TestDatabaseConfig:
    """Tests for DatabaseConfig dataclass."""

    def test_default_timeout(self):
        """Should have default timeout."""
        config = DatabaseConfig()
        assert config.timeout == 5.0

    def test_default_retries(self):
        """Should have default retries."""
        config = DatabaseConfig()
        assert config.retries == 3

    def test_default_retry_delay(self):
        """Should have default retry delay."""
        config = DatabaseConfig()
        assert config.retry_delay == 0.5

    def test_custom_values(self, temp_db):
        """Should accept custom values."""
        config = DatabaseConfig(
            db_path=str(temp_db),
            timeout=10.0,
            retries=5,
            retry_delay=1.0,
        )
        assert config.db_path == str(temp_db)
        assert config.timeout == 10.0
        assert config.retries == 5
        assert config.retry_delay == 1.0


class TestDatabaseOperations:
    """Tests for DatabaseOperations class."""

    def test_init_with_default_config(self):
        """Should initialize with default config."""
        ops = DatabaseOperations()
        assert ops.config is not None
        assert isinstance(ops.config, DatabaseConfig)

    def test_init_with_custom_config(self, temp_db):
        """Should accept custom config."""
        config = DatabaseConfig(db_path=str(temp_db))
        ops = DatabaseOperations(config)
        assert ops.config.db_path == str(temp_db)


class TestDatabaseConnection:
    """Tests for database connection context manager."""

    def test_connection_returns_connection(self, temp_db):
        """Connection context should yield sqlite3 connection."""
        config = DatabaseConfig(db_path=str(temp_db))
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            assert conn is not None
            # Should be able to execute queries
            conn.execute("SELECT 1")

    def test_connection_uses_row_factory(self, db_with_table):
        """Connection should use Row factory."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            cursor = conn.execute("SELECT * FROM test_table LIMIT 1")
            row = cursor.fetchone()
            # Row factory allows dict-like access
            assert row["name"] == "item1"

    def test_connection_closes_on_exit(self, temp_db):
        """Connection should close on context exit."""
        config = DatabaseConfig(db_path=str(temp_db))
        ops = DatabaseOperations(config)

        conn_ref = None
        with ops.connection() as conn:
            conn_ref = conn
            conn.execute("SELECT 1")

        # Connection should be closed
        with pytest.raises(sqlite3.ProgrammingError):
            conn_ref.execute("SELECT 1")


class TestDatabaseTransaction:
    """Tests for database transaction context manager."""

    def test_transaction_commits_on_success(self, db_with_table):
        """Transaction should commit on successful completion."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        with ops.transaction() as conn:
            conn.execute("INSERT INTO test_table (name, value) VALUES ('item4', 40)")

        # Verify data was committed
        with ops.connection() as conn:
            cursor = conn.execute("SELECT COUNT(*) FROM test_table")
            count = cursor.fetchone()[0]
            assert count == 4

    def test_transaction_rollbacks_on_error(self, db_with_table):
        """Transaction should rollback on exception."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        try:
            with ops.transaction() as conn:
                conn.execute(
                    "INSERT INTO test_table (name, value) VALUES ('item4', 40)"
                )
                raise ValueError("Simulated error")
        except ValueError:
            pass

        # Verify data was NOT committed
        with ops.connection() as conn:
            cursor = conn.execute("SELECT COUNT(*) FROM test_table")
            count = cursor.fetchone()[0]
            assert count == 3  # Original 3 items


class TestExecuteQuery:
    """Tests for execute_query method."""

    def test_returns_list_of_dicts(self, db_with_table):
        """Should return list of dictionaries."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        results = ops.execute_query("SELECT * FROM test_table ORDER BY id")
        assert isinstance(results, list)
        assert len(results) == 3
        assert results[0]["name"] == "item1"
        assert results[1]["name"] == "item2"

    def test_empty_result(self, db_with_table):
        """Should return empty list for no matches."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        results = ops.execute_query("SELECT * FROM test_table WHERE value > 100")
        assert results == []

    def test_with_parameters(self, db_with_table):
        """Should support parameterized queries."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        results = ops.execute_query(
            "SELECT * FROM test_table WHERE value > :min_value", {"min_value": 15}
        )
        assert len(results) == 2
        assert all(r["value"] > 15 for r in results)


class TestExecuteWrite:
    """Tests for execute_write method."""

    def test_insert_returns_rowcount(self, db_with_table):
        """INSERT should return 1 for single row."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        count = ops.execute_write(
            "INSERT INTO test_table (name, value) VALUES ('item4', 40)"
        )
        assert count == 1

    def test_update_returns_affected_rows(self, db_with_table):
        """UPDATE should return number of affected rows."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        count = ops.execute_write(
            "UPDATE test_table SET value = value + 1 WHERE value < 25"
        )
        assert count == 2  # item1 and item2

    def test_delete_returns_affected_rows(self, db_with_table):
        """DELETE should return number of deleted rows."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        count = ops.execute_write("DELETE FROM test_table WHERE value < 25")
        assert count == 2

    def test_with_parameters(self, db_with_table):
        """Should support parameterized statements."""
        config = DatabaseConfig(db_path=str(db_with_table))
        ops = DatabaseOperations(config)

        count = ops.execute_write(
            "INSERT INTO test_table (name, value) VALUES (:name, :value)",
            {"name": "item4", "value": 40},
        )
        assert count == 1


class TestGetDb:
    """Tests for get_db singleton function."""

    def test_returns_database_operations(self, reset_db_singleton):
        """Should return DatabaseOperations instance."""
        ops = get_db()
        assert isinstance(ops, DatabaseOperations)

    def test_singleton_behavior(self, reset_db_singleton):
        """Should return same instance on repeated calls."""
        ops1 = get_db()
        ops2 = get_db()
        assert ops1 is ops2

    def test_new_config_creates_new_instance(self, reset_db_singleton, temp_db):
        """Should create new instance when config is provided."""
        ops1 = get_db()
        config = DatabaseConfig(db_path=str(temp_db))
        ops2 = get_db(config)
        assert ops1 is not ops2
        assert ops2.config.db_path == str(temp_db)


class TestDictFactory:
    """Tests for dict_factory helper function."""

    def test_converts_row_to_dict(self):
        """Should convert sqlite3 row to dictionary."""

        # Create a mock cursor description
        class MockCursor:
            description = [("id",), ("name",), ("value",)]

        row = (1, "test", 42)
        result = dict_factory(MockCursor(), row)

        assert result == {"id": 1, "name": "test", "value": 42}

    def test_handles_empty_row(self):
        """Should handle empty row."""

        class MockCursor:
            description = []

        row = ()
        result = dict_factory(MockCursor(), row)

        assert result == {}


class TestStubImplementation:
    """
    Tests for the stub implementation fallback in db.py.

    These tests verify that the stub code (except ImportError block) works correctly
    when db_operations.py is unavailable.
    """

    def test_stub_database_config_defaults(self, force_stub_implementation):
        """Stub DatabaseConfig should have correct defaults."""
        StubConfig = force_stub_implementation.DatabaseConfig
        config = StubConfig()

        assert config.timeout == 5.0
        assert config.retries == 3
        assert config.retry_delay == 0.5
        # db_path is computed from get_repo_root
        assert "codeflow.db" in config.db_path

    def test_stub_database_config_custom_values(self, force_stub_implementation, tmp_path):
        """Stub DatabaseConfig should accept custom values."""
        StubConfig = force_stub_implementation.DatabaseConfig
        db_path = tmp_path / "custom.db"

        config = StubConfig(
            db_path=str(db_path),
            timeout=10.0,
            retries=5,
            retry_delay=1.0,
        )

        assert config.db_path == str(db_path)
        assert config.timeout == 10.0
        assert config.retries == 5
        assert config.retry_delay == 1.0

    def test_stub_database_operations_init(self, force_stub_implementation):
        """Stub DatabaseOperations should initialize with config."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations

        ops = StubOps()
        assert ops.config is not None
        assert isinstance(ops.config, StubConfig)

    def test_stub_database_operations_custom_config(
        self, force_stub_implementation, tmp_path
    ):
        """Stub DatabaseOperations should accept custom config."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "custom.db"

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        assert ops.config.db_path == str(db_path)

    def test_stub_connection_context_manager(self, force_stub_implementation, tmp_path):
        """Stub connection() should work as context manager."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        with ops.connection() as conn:
            assert conn is not None
            conn.execute("SELECT 1")

    def test_stub_connection_row_factory(self, force_stub_implementation, tmp_path):
        """Stub connection should use Row factory for dict-like access."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.execute("INSERT INTO t VALUES (1, 'test')")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        with ops.connection() as conn:
            cursor = conn.execute("SELECT * FROM t")
            row = cursor.fetchone()
            assert row["id"] == 1
            assert row["name"] == "test"

    def test_stub_connection_closes(self, force_stub_implementation, tmp_path):
        """Stub connection should close on context exit."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        conn_ref = None
        with ops.connection() as conn:
            conn_ref = conn
            conn.execute("SELECT 1")

        # Connection should be closed
        with pytest.raises(sqlite3.ProgrammingError):
            conn_ref.execute("SELECT 1")

    def test_stub_transaction_commits(self, force_stub_implementation, tmp_path):
        """Stub transaction() should commit on success."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        with ops.transaction() as conn:
            conn.execute("INSERT INTO t VALUES (1, 'test')")

        # Verify committed
        with ops.connection() as conn:
            cursor = conn.execute("SELECT COUNT(*) FROM t")
            assert cursor.fetchone()[0] == 1

    def test_stub_transaction_rollbacks_on_error(
        self, force_stub_implementation, tmp_path
    ):
        """Stub transaction() should rollback on exception."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        try:
            with ops.transaction() as conn:
                conn.execute("INSERT INTO t VALUES (1, 'test')")
                raise ValueError("Simulated error")
        except ValueError:
            pass

        # Verify NOT committed (rollback)
        with ops.connection() as conn:
            cursor = conn.execute("SELECT COUNT(*) FROM t")
            assert cursor.fetchone()[0] == 0

    def test_stub_execute_query(self, force_stub_implementation, tmp_path):
        """Stub execute_query() should return list of dicts."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table with data
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.execute("INSERT INTO t VALUES (1, 'one')")
        conn.execute("INSERT INTO t VALUES (2, 'two')")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        results = ops.execute_query("SELECT * FROM t ORDER BY id")
        assert isinstance(results, list)
        assert len(results) == 2
        assert results[0] == {"id": 1, "name": "one"}
        assert results[1] == {"id": 2, "name": "two"}

    def test_stub_execute_query_with_params(self, force_stub_implementation, tmp_path):
        """Stub execute_query() should support parameters."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table with data
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.execute("INSERT INTO t VALUES (1, 'one')")
        conn.execute("INSERT INTO t VALUES (2, 'two')")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        results = ops.execute_query("SELECT * FROM t WHERE id > :min_id", {"min_id": 1})
        assert len(results) == 1
        assert results[0]["id"] == 2

    def test_stub_execute_write(self, force_stub_implementation, tmp_path):
        """Stub execute_write() should return affected row count."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        count = ops.execute_write("INSERT INTO t VALUES (1, 'test')")
        assert count == 1

    def test_stub_execute_write_with_params(self, force_stub_implementation, tmp_path):
        """Stub execute_write() should support parameters."""
        StubConfig = force_stub_implementation.DatabaseConfig
        StubOps = force_stub_implementation.DatabaseOperations
        db_path = tmp_path / "test.db"

        # Create table
        conn = sqlite3.connect(str(db_path))
        conn.execute("CREATE TABLE t (id INTEGER, name TEXT)")
        conn.commit()
        conn.close()

        config = StubConfig(db_path=str(db_path))
        ops = StubOps(config)

        count = ops.execute_write(
            "INSERT INTO t VALUES (:id, :name)", {"id": 1, "name": "test"}
        )
        assert count == 1

    def test_stub_get_db_singleton(self, force_stub_implementation):
        """Stub get_db() should return singleton instance."""
        stub_get_db = force_stub_implementation.get_db
        StubOps = force_stub_implementation.DatabaseOperations

        # Reset singleton
        force_stub_implementation._db_ops = None

        ops1 = stub_get_db()
        ops2 = stub_get_db()

        assert isinstance(ops1, StubOps)
        assert ops1 is ops2

    def test_stub_get_db_with_config(self, force_stub_implementation, tmp_path):
        """Stub get_db() with config should create new instance."""
        stub_get_db = force_stub_implementation.get_db
        StubConfig = force_stub_implementation.DatabaseConfig
        db_path = tmp_path / "custom.db"

        # Reset singleton
        force_stub_implementation._db_ops = None

        ops1 = stub_get_db()
        config = StubConfig(db_path=str(db_path))
        ops2 = stub_get_db(config)

        assert ops1 is not ops2
        assert ops2.config.db_path == str(db_path)
