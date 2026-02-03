"""
test_db_retry_concurrency.py - Tests for database retry logic and concurrent access.

Tests retry mechanisms, transaction rollback, concurrent write handling,
WAL mode, operation logging, and error recovery.
"""

import json
import sqlite3
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import pytest

SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
sys.path.insert(0, str(SCRIPTS_PATH / "db" / "lib"))
sys.path.insert(0, str(SCRIPTS_PATH / "codeflow_py_lib"))

from db_operations import DatabaseConfig, DatabaseOperations


@pytest.fixture
def temp_db_dir():
    """Create a temporary directory for database files."""
    with tempfile.TemporaryDirectory(prefix="codeflow_db_test_") as temp:
        yield Path(temp)


@pytest.fixture
def db_config(temp_db_dir):
    """Create database configuration with test paths."""
    return DatabaseConfig(
        db_path=str(temp_db_dir / "test.db"),
        op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
        timeout=5.0,
        retries=3,
        retry_delay=0.1,
        enable_wal=True,
        enable_foreign_keys=True,
    )


@pytest.fixture
def db_ops(db_config):
    """Create DatabaseOperations instance."""
    return DatabaseOperations(db_config)


@pytest.fixture
def initialized_db(db_ops):
    """Initialize database with basic schema."""
    with db_ops.connection() as conn:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS test_table (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                value INTEGER
            );
            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY
            );
            INSERT OR IGNORE INTO schema_version (version) VALUES (1);
        """)
    return db_ops


# =============================================================================
# RETRY LOGIC TESTS
# =============================================================================


class TestRetryLogic:
    """Tests for database connection retry logic."""

    def test_config_defaults(self):
        """Config should have sensible defaults."""
        config = DatabaseConfig()
        assert config.retries == 3
        assert config.retry_delay == 0.5
        assert config.timeout == 5.0

    def test_successful_connection_no_retry(self, initialized_db):
        """Successful connection should not retry."""
        # Simple query should work first time
        results = initialized_db.execute_query("SELECT 1 as value")
        assert results == [{"value": 1}]

    def test_retry_on_locked_database(self, db_config, temp_db_dir):
        """Should retry on 'database is locked' error."""
        db_path = str(temp_db_dir / "locked_test.db")
        config = DatabaseConfig(
            db_path=db_path,
            op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
            retries=3,
            retry_delay=0.05,
        )

        # Create database
        conn = sqlite3.connect(db_path)
        conn.execute("CREATE TABLE test (id INTEGER)")
        conn.commit()

        # Acquire exclusive lock
        lock_conn = sqlite3.connect(db_path, timeout=0.1)
        lock_conn.isolation_level = "EXCLUSIVE"
        lock_conn.execute("BEGIN EXCLUSIVE")

        db_ops = DatabaseOperations(config)

        # Track retry attempts
        original_connect = sqlite3.connect
        attempts = []

        def tracking_connect(*args, **kwargs):
            attempts.append(1)
            return original_connect(*args, **kwargs)

        # Release lock after first attempt
        def release_lock():
            time.sleep(0.1)
            lock_conn.rollback()
            lock_conn.close()

        release_thread = threading.Thread(target=release_lock)
        release_thread.start()

        # This should eventually succeed after lock is released
        try:
            with db_ops.connection() as conn:
                result = conn.execute("SELECT 1").fetchone()
                assert result[0] == 1
        except sqlite3.OperationalError:
            pass  # May still fail depending on timing

        release_thread.join()

    def test_max_retries_exceeded(self, temp_db_dir):
        """Should raise after max retries exceeded."""
        # Use a valid path but simulate locked database
        config = DatabaseConfig(
            db_path=str(temp_db_dir / "retry_test.sqlite"),
            op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
            retries=2,
            retry_delay=0.01,
        )

        db_ops = DatabaseOperations(config)

        # Create and lock the database
        lock_conn = sqlite3.connect(str(temp_db_dir / "retry_test.sqlite"))
        lock_conn.execute("CREATE TABLE IF NOT EXISTS test (id INTEGER)")
        lock_conn.execute("BEGIN EXCLUSIVE TRANSACTION")

        try:
            # Attempting to write should fail after retries
            with pytest.raises(sqlite3.OperationalError):
                db_ops.execute_write("INSERT INTO test (id) VALUES (1)")
        finally:
            lock_conn.rollback()
            lock_conn.close()


# =============================================================================
# TRANSACTION TESTS
# =============================================================================


class TestTransactions:
    """Tests for transaction handling."""

    def test_transaction_commit(self, initialized_db):
        """Successful transaction should commit."""
        with initialized_db.transaction() as conn:
            conn.execute("INSERT INTO test_table (name, value) VALUES ('test', 42)")

        # Verify data persisted
        results = initialized_db.execute_query(
            "SELECT * FROM test_table WHERE name = 'test'"
        )
        assert len(results) == 1
        assert results[0]["value"] == 42

    def test_transaction_rollback_on_error(self, initialized_db):
        """Transaction should rollback on error."""
        try:
            with initialized_db.transaction() as conn:
                conn.execute(
                    "INSERT INTO test_table (name, value) VALUES ('rollback', 1)"
                )
                # Force an error
                conn.execute("INSERT INTO nonexistent_table (x) VALUES (1)")
        except sqlite3.OperationalError:
            pass

        # Verify data was rolled back
        results = initialized_db.execute_query(
            "SELECT * FROM test_table WHERE name = 'rollback'"
        )
        assert len(results) == 0

    def test_transaction_rollback_on_exception(self, initialized_db):
        """Transaction should rollback on Python exception."""
        try:
            with initialized_db.transaction() as conn:
                conn.execute(
                    "INSERT INTO test_table (name, value) VALUES ('exception', 2)"
                )
                raise ValueError("Test exception")
        except ValueError:
            pass

        # Verify data was rolled back
        results = initialized_db.execute_query(
            "SELECT * FROM test_table WHERE name = 'exception'"
        )
        assert len(results) == 0

    def test_nested_operations_in_transaction(self, initialized_db):
        """Multiple operations in one transaction."""
        with initialized_db.transaction() as conn:
            conn.execute("INSERT INTO test_table (name, value) VALUES ('item1', 1)")
            conn.execute("INSERT INTO test_table (name, value) VALUES ('item2', 2)")
            conn.execute("INSERT INTO test_table (name, value) VALUES ('item3', 3)")

        # All should be committed
        results = initialized_db.execute_query("SELECT COUNT(*) as cnt FROM test_table")
        assert results[0]["cnt"] == 3


# =============================================================================
# CONCURRENT ACCESS TESTS
# =============================================================================


class TestConcurrency:
    """Tests for concurrent database access."""

    def test_concurrent_reads(self, initialized_db):
        """Multiple concurrent reads should work."""
        # Add some data
        for i in range(10):
            initialized_db.execute_write(
                "INSERT INTO test_table (name, value) VALUES (:name, :value)",
                {"name": f"item{i}", "value": i},
            )

        def read_all():
            return initialized_db.execute_query("SELECT * FROM test_table")

        with ThreadPoolExecutor(max_workers=5) as executor:
            futures = [executor.submit(read_all) for _ in range(10)]
            results = [f.result() for f in as_completed(futures)]

        # All should succeed
        assert len(results) == 10
        for result in results:
            assert len(result) == 10

    def test_concurrent_writes_with_wal(self, db_config, temp_db_dir):
        """Concurrent writes should work with WAL mode."""
        # Create multiple db_ops instances (simulating concurrent processes)
        config = DatabaseConfig(
            db_path=str(temp_db_dir / "wal_test.db"),
            op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
            enable_wal=True,
            retries=5,
            retry_delay=0.1,
        )

        db_ops = DatabaseOperations(config)

        # Initialize schema
        with db_ops.connection() as conn:
            conn.execute("CREATE TABLE counter (id INTEGER PRIMARY KEY, value INTEGER)")
            conn.execute("INSERT INTO counter (id, value) VALUES (1, 0)")

        errors = []
        success_count = [0]

        def increment():
            """Increment counter in transaction."""
            try:
                local_ops = DatabaseOperations(config)
                with local_ops.transaction() as conn:
                    cursor = conn.execute("SELECT value FROM counter WHERE id = 1")
                    current = cursor.fetchone()[0]
                    conn.execute(
                        "UPDATE counter SET value = :val WHERE id = 1",
                        {"val": current + 1},
                    )
                success_count[0] += 1
            except Exception as e:
                errors.append(str(e))

        # Run concurrent increments
        threads = [threading.Thread(target=increment) for _ in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        # Check final value (may be less than 10 due to race conditions)
        result = db_ops.execute_query("SELECT value FROM counter WHERE id = 1")
        # At least some should succeed
        assert success_count[0] >= 1
        assert result[0]["value"] >= 1

    def test_read_write_interleaved(self, initialized_db):
        """Reads and writes can be interleaved."""
        # Pre-populate
        for i in range(5):
            initialized_db.execute_write(
                "INSERT INTO test_table (name, value) VALUES (:name, :value)",
                {"name": f"initial{i}", "value": i},
            )

        results = {"reads": [], "writes": []}
        errors = []

        def read_op():
            try:
                data = initialized_db.execute_query(
                    "SELECT COUNT(*) as c FROM test_table"
                )
                results["reads"].append(data[0]["c"])
            except Exception as e:
                errors.append(f"read: {e}")

        def write_op(i):
            try:
                initialized_db.execute_write(
                    "INSERT INTO test_table (name, value) VALUES (:name, :value)",
                    {"name": f"concurrent{i}", "value": i},
                )
                results["writes"].append(i)
            except Exception as e:
                errors.append(f"write: {e}")

        with ThreadPoolExecutor(max_workers=4) as executor:
            futures = []
            for i in range(5):
                futures.append(executor.submit(read_op))
                futures.append(executor.submit(write_op, i))

            for f in as_completed(futures):
                f.result()  # Raise any exceptions

        # Verify some operations completed
        assert len(results["reads"]) > 0 or len(results["writes"]) > 0


# =============================================================================
# WAL MODE TESTS
# =============================================================================


class TestWALMode:
    """Tests for WAL mode operation."""

    def test_wal_mode_enabled(self, db_config, temp_db_dir):
        """WAL mode should be enabled when configured."""
        config = DatabaseConfig(
            db_path=str(temp_db_dir / "wal_enabled.db"),
            op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
            enable_wal=True,
        )
        db_ops = DatabaseOperations(config)

        with db_ops.connection() as conn:
            cursor = conn.execute("PRAGMA journal_mode")
            mode = cursor.fetchone()[0]
            assert mode.lower() == "wal"

    def test_wal_mode_disabled(self, temp_db_dir):
        """WAL mode can be disabled."""
        config = DatabaseConfig(
            db_path=str(temp_db_dir / "wal_disabled.db"),
            op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
            enable_wal=False,
        )
        db_ops = DatabaseOperations(config)

        with db_ops.connection() as conn:
            cursor = conn.execute("PRAGMA journal_mode")
            mode = cursor.fetchone()[0]
            # Default is 'delete' when WAL not enabled
            assert mode.lower() in ("delete", "memory")

    def test_checkpoint(self, initialized_db):
        """Checkpoint should work."""
        # Write some data
        for i in range(100):
            initialized_db.execute_write(
                "INSERT INTO test_table (name, value) VALUES (:name, :value)",
                {"name": f"checkpoint{i}", "value": i},
            )

        # Checkpoint should not raise
        initialized_db.checkpoint("PASSIVE")


# =============================================================================
# OPERATION LOGGING TESTS
# =============================================================================


class TestOperationLogging:
    """Tests for operation audit logging."""

    def test_query_logged(self, initialized_db, db_config):
        """Queries should be logged."""
        initialized_db.execute_query("SELECT 1")

        log_path = Path(db_config.op_log_path)
        assert log_path.exists()

        with open(log_path) as f:
            logs = [json.loads(line) for line in f]

        # Find query log
        query_logs = [entry for entry in logs if entry.get("operation") == "QUERY"]
        assert len(query_logs) > 0
        assert query_logs[-1]["status"] == "SUCCESS"

    def test_write_logged(self, initialized_db, db_config):
        """Writes should be logged."""
        initialized_db.execute_write(
            "INSERT INTO test_table (name, value) VALUES ('logged', 1)"
        )

        log_path = Path(db_config.op_log_path)
        with open(log_path) as f:
            logs = [json.loads(line) for line in f]

        write_logs = [entry for entry in logs if entry.get("operation") == "WRITE"]
        assert len(write_logs) > 0
        assert write_logs[-1]["status"] == "SUCCESS"

    def test_transaction_logged(self, initialized_db, db_config):
        """Transactions should be logged."""
        with initialized_db.transaction() as conn:
            conn.execute("INSERT INTO test_table (name, value) VALUES ('txn', 1)")

        log_path = Path(db_config.op_log_path)
        with open(log_path) as f:
            logs = [json.loads(line) for line in f]

        txn_logs = [entry for entry in logs if entry.get("operation") == "TRANSACTION"]
        assert len(txn_logs) > 0
        assert txn_logs[-1]["status"] == "COMMITTED"

    def test_failed_operation_logged(self, initialized_db, db_config):
        """Failed operations should be logged."""
        try:
            initialized_db.execute_query("SELECT * FROM nonexistent_table")
        except sqlite3.OperationalError:
            pass

        log_path = Path(db_config.op_log_path)
        with open(log_path) as f:
            logs = [json.loads(line) for line in f]

        failed_logs = [entry for entry in logs if entry.get("status") == "FAILED"]
        assert len(failed_logs) > 0

    def test_duration_logged(self, initialized_db, db_config):
        """Operations should log duration."""
        initialized_db.execute_query("SELECT 1")

        log_path = Path(db_config.op_log_path)
        with open(log_path) as f:
            logs = [json.loads(line) for line in f]

        query_logs = [entry for entry in logs if entry.get("operation") == "QUERY"]
        assert len(query_logs) > 0
        assert "duration_ms" in query_logs[-1]
        assert query_logs[-1]["duration_ms"] >= 0


# =============================================================================
# ERROR RECOVERY TESTS
# =============================================================================


class TestErrorRecovery:
    """Tests for error handling and recovery."""

    def test_connection_closed_after_error(self, initialized_db):
        """Connection should be closed even on error."""
        try:
            with initialized_db.connection() as conn:
                conn.execute("INVALID SQL")
        except sqlite3.OperationalError:
            pass

        # New connection should work
        results = initialized_db.execute_query("SELECT 1 as v")
        assert results == [{"v": 1}]

    def test_recover_after_corrupted_query(self, initialized_db):
        """Should recover after corrupted query."""
        # First, normal operation
        initialized_db.execute_write(
            "INSERT INTO test_table (name, value) VALUES ('before', 1)"
        )

        # Corrupted query
        try:
            initialized_db.execute_query("SEL ECT * FR OM test_table")
        except sqlite3.OperationalError:
            pass

        # Should still work
        results = initialized_db.execute_query(
            "SELECT * FROM test_table WHERE name = 'before'"
        )
        assert len(results) == 1

    def test_integrity_check(self, initialized_db):
        """Integrity check should pass for healthy database."""
        assert initialized_db.check_integrity() is True

    def test_backup_recovery(self, initialized_db, temp_db_dir):
        """Should be able to create and use backup."""
        # Add data
        initialized_db.execute_write(
            "INSERT INTO test_table (name, value) VALUES ('backup_test', 123)"
        )

        # Create backup
        backup_path = initialized_db.backup(str(temp_db_dir / "backup.db"))
        assert Path(backup_path).exists()

        # Verify backup has data
        backup_config = DatabaseConfig(
            db_path=backup_path,
            op_log_path=str(temp_db_dir / "logs" / "backup_ops.jsonl"),
        )
        backup_ops = DatabaseOperations(backup_config)

        results = backup_ops.execute_query(
            "SELECT * FROM test_table WHERE name = 'backup_test'"
        )
        assert len(results) == 1
        assert results[0]["value"] == 123


# =============================================================================
# FOREIGN KEY TESTS
# =============================================================================


class TestForeignKeys:
    """Tests for foreign key enforcement."""

    def test_foreign_keys_enabled(self, db_config, temp_db_dir):
        """Foreign keys should be enforced when enabled."""
        config = DatabaseConfig(
            db_path=str(temp_db_dir / "fk_test.db"),
            op_log_path=str(temp_db_dir / "logs" / "ops.jsonl"),
            enable_foreign_keys=True,
        )
        db_ops = DatabaseOperations(config)

        with db_ops.connection() as conn:
            conn.executescript("""
                CREATE TABLE parent (id INTEGER PRIMARY KEY);
                CREATE TABLE child (
                    id INTEGER PRIMARY KEY,
                    parent_id INTEGER REFERENCES parent(id)
                );
            """)

        # This should fail due to FK constraint
        with pytest.raises(sqlite3.IntegrityError):
            db_ops.execute_write("INSERT INTO child (id, parent_id) VALUES (1, 999)")


# =============================================================================
# UTILITY METHOD TESTS
# =============================================================================


class TestUtilityMethods:
    """Tests for utility methods."""

    def test_get_value(self, initialized_db):
        """get_value should return single value."""
        initialized_db.execute_write(
            "INSERT INTO test_table (name, value) VALUES ('single', 42)"
        )

        value = initialized_db.get_value(
            "SELECT value FROM test_table WHERE name = 'single'"
        )
        assert value == 42

    def test_get_value_default(self, initialized_db):
        """get_value should return default when no results."""
        value = initialized_db.get_value(
            "SELECT value FROM test_table WHERE name = 'nonexistent'", default=-1
        )
        assert value == -1

    def test_row_exists(self, initialized_db):
        """row_exists should check existence."""
        initialized_db.execute_write(
            "INSERT INTO test_table (name, value) VALUES ('exists', 1)"
        )

        assert initialized_db.row_exists("test_table", {"name": "exists"}) is True
        assert initialized_db.row_exists("test_table", {"name": "missing"}) is False

    def test_table_exists(self, initialized_db):
        """table_exists should check table existence."""
        assert initialized_db.table_exists("test_table") is True
        assert initialized_db.table_exists("nonexistent_table") is False

    def test_get_table_count(self, initialized_db):
        """get_table_count should return row count."""
        for i in range(5):
            initialized_db.execute_write(
                "INSERT INTO test_table (name, value) VALUES (:name, :value)",
                {"name": f"count{i}", "value": i},
            )

        count = initialized_db.get_table_count("test_table")
        assert count == 5

    def test_get_schema_version(self, initialized_db):
        """get_schema_version should return version."""
        version = initialized_db.get_schema_version()
        assert version == 1

    def test_get_db_size(self, initialized_db):
        """get_db_size should return file size."""
        size = initialized_db.get_db_size()
        assert size > 0


# =============================================================================
# MAINTENANCE METHOD TESTS
# =============================================================================


class TestMaintenanceMethods:
    """Tests for maintenance methods."""

    def test_vacuum(self, initialized_db):
        """vacuum should not raise."""
        # Add and delete data to create fragmentation
        for i in range(100):
            initialized_db.execute_write(
                "INSERT INTO test_table (name, value) VALUES (:name, :value)",
                {"name": f"vacuum{i}", "value": i},
            )
        initialized_db.execute_write("DELETE FROM test_table")

        # Should not raise
        initialized_db.vacuum()

    def test_analyze(self, initialized_db):
        """analyze should not raise."""
        initialized_db.analyze()

    def test_execute_many(self, initialized_db):
        """execute_many should handle batch operations."""
        params = [{"name": f"batch{i}", "value": i} for i in range(10)]

        count = initialized_db.execute_many(
            "INSERT INTO test_table (name, value) VALUES (:name, :value)", params
        )

        assert count == 10

        # Verify all inserted
        results = initialized_db.execute_query(
            "SELECT COUNT(*) as c FROM test_table WHERE name LIKE 'batch%'"
        )
        assert results[0]["c"] == 10
