"""
db_operations.py - Unified database operations library for Python scripts.

Provides:
    - DatabaseConfig: Configuration dataclass
    - DatabaseOperations: Main operations class with retry logic
    - get_db_operations(): Singleton accessor

Example usage:
    from db_operations import get_db_operations

    db = get_db_operations()
    results = db.execute_query("SELECT * FROM tasks WHERE status = :status", {"status": "active"})
"""

import json
import logging
import os
import sqlite3
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import date, datetime
from pathlib import Path
from typing import Any, Dict, Generator, List, Optional, Tuple, Union

logger = logging.getLogger(__name__)


def _find_repo_root() -> Path:
    """Find repository root by looking for .codeflow directory."""
    current = Path.cwd()
    while current != current.parent:
        if (current / ".codeflow").exists():
            return current
        current = current.parent
    return Path.cwd()


@dataclass
class DatabaseConfig:
    """Database configuration with defaults."""

    db_path: str = field(
        default_factory=lambda: str(_find_repo_root() / ".state/db/codeflow.db")
    )
    timeout: float = 5.0
    retries: int = 3
    retry_delay: float = 0.5
    op_log_path: str = field(
        default_factory=lambda: str(
            _find_repo_root() / f".state/logs/db/operations-{date.today()}.jsonl"
        )
    )
    enable_wal: bool = True
    enable_foreign_keys: bool = True

    def __post_init__(self):
        """Ensure log directory exists."""
        Path(self.op_log_path).parent.mkdir(parents=True, exist_ok=True)


class DatabaseOperations:
    """Unified database operations with retry logic and logging."""

    def __init__(self, config: Optional[DatabaseConfig] = None):
        """Initialize database operations.

        Args:
            config: Optional configuration, uses defaults if not provided
        """
        self.config = config or DatabaseConfig()
        self._ensure_directories()

    def _ensure_directories(self) -> None:
        """Ensure required directories exist."""
        Path(self.config.op_log_path).parent.mkdir(parents=True, exist_ok=True)
        Path(self.config.db_path).parent.mkdir(parents=True, exist_ok=True)

    def _log_operation(
        self,
        operation: str,
        status: str,
        details: str = "",
        duration_ms: Optional[int] = None,
    ) -> None:
        """Log database operation to JSONL audit trail.

        Args:
            operation: Type of operation (QUERY, EXECUTE, TRANSACTION, etc.)
            status: Status (SUCCESS, FAILED)
            details: Additional details
            duration_ms: Optional duration in milliseconds
        """
        timestamp = datetime.utcnow().isoformat() + "Z"
        session_id = os.environ.get("CLAUDE_SESSION_ID", "unknown")

        log_entry = {
            "ts": timestamp,
            "level": "INFO" if status == "SUCCESS" else "ERROR",
            "session_id": session_id,
            "event": "db_operation",
            "operation": operation,
            "status": status,
        }

        if details:
            log_entry["details"] = details[:200]  # Truncate long details
        if duration_ms is not None:
            log_entry["duration_ms"] = duration_ms

        try:
            with open(self.config.op_log_path, "a") as f:
                f.write(json.dumps(log_entry, separators=(",", ":")) + "\n")
        except Exception as e:
            logger.warning(f"Failed to write operation log: {e}")

    @contextmanager
    def connection(self) -> Generator[sqlite3.Connection, None, None]:
        """Context manager for database connection with retry logic.

        Yields:
            sqlite3.Connection configured with row_factory and pragmas

        Raises:
            sqlite3.OperationalError: If connection fails after retries
        """
        conn = None
        last_error = None

        for attempt in range(self.config.retries):
            try:
                conn = sqlite3.connect(
                    self.config.db_path,
                    timeout=self.config.timeout,
                    isolation_level=None,  # Autocommit mode
                )
                conn.row_factory = sqlite3.Row

                # Configure connection
                if self.config.enable_wal:
                    conn.execute("PRAGMA journal_mode=WAL")
                    conn.execute("PRAGMA synchronous=NORMAL")
                if self.config.enable_foreign_keys:
                    conn.execute("PRAGMA foreign_keys=ON")

                yield conn
                return

            except sqlite3.OperationalError as e:
                last_error = e
                if "database is locked" in str(e) and attempt < self.config.retries - 1:
                    time.sleep(self.config.retry_delay)
                    continue
                raise
            finally:
                if conn:
                    conn.close()

        if last_error:
            self._log_operation("CONNECTION", "FAILED", str(last_error))
            raise last_error

    @contextmanager
    def transaction(self) -> Generator[sqlite3.Connection, None, None]:
        """Context manager for transaction with automatic commit/rollback.

        Yields:
            sqlite3.Connection within a transaction

        Example:
            with db.transaction() as conn:
                conn.execute("INSERT INTO tasks (id, name) VALUES (?, ?)", (task_id, name))
                conn.execute("INSERT INTO task_log (task_id) VALUES (?)", (task_id,))
        """
        start_time = time.time()
        with self.connection() as conn:
            try:
                conn.execute("BEGIN TRANSACTION")
                yield conn
                conn.execute("COMMIT")
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation("TRANSACTION", "COMMITTED", "", duration_ms)
            except Exception as e:
                conn.execute("ROLLBACK")
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation("TRANSACTION", "ROLLED_BACK", str(e), duration_ms)
                raise

    def execute_query(
        self,
        query: str,
        params: Optional[Union[Dict[str, Any], Tuple]] = None,
    ) -> List[Dict[str, Any]]:
        """Execute SELECT query and return results as list of dicts.

        Args:
            query: SQL SELECT query (can use named :param or ? placeholders)
            params: Optional parameters dict or tuple

        Returns:
            List of dictionaries with column names as keys
        """
        start_time = time.time()
        with self.connection() as conn:
            try:
                cursor = conn.execute(query, params or {})
                results = [dict(row) for row in cursor.fetchall()]
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation(
                    "QUERY", "SUCCESS", f"{len(results)} rows", duration_ms
                )
                return results
            except Exception as e:
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation("QUERY", "FAILED", str(e), duration_ms)
                raise

    def execute_write(
        self,
        statement: str,
        params: Optional[Union[Dict[str, Any], Tuple]] = None,
    ) -> int:
        """Execute INSERT/UPDATE/DELETE and return rows affected.

        Args:
            statement: SQL statement
            params: Optional parameters dict or tuple

        Returns:
            Number of rows affected
        """
        start_time = time.time()
        with self.connection() as conn:
            try:
                cursor = conn.execute(statement, params or {})
                conn.commit()
                rows_affected = cursor.rowcount
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation(
                    "WRITE", "SUCCESS", f"{rows_affected} rows", duration_ms
                )
                return rows_affected
            except Exception as e:
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation("WRITE", "FAILED", str(e), duration_ms)
                raise

    def execute_many(
        self,
        statement: str,
        params_list: List[Union[Dict[str, Any], Tuple]],
    ) -> int:
        """Execute statement with multiple parameter sets in a transaction.

        Args:
            statement: SQL statement with placeholders
            params_list: List of parameter dicts or tuples

        Returns:
            Total number of rows affected
        """
        start_time = time.time()
        total_affected = 0

        with self.transaction() as conn:
            for params in params_list:
                cursor = conn.execute(statement, params)
                total_affected += cursor.rowcount

        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation(
            "BATCH_WRITE", "SUCCESS", f"{total_affected} rows", duration_ms
        )
        return total_affected

    def execute_script(self, script: str) -> None:
        """Execute a multi-statement SQL script.

        Args:
            script: SQL script with multiple statements
        """
        start_time = time.time()
        with self.connection() as conn:
            try:
                conn.executescript(script)
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation(
                    "SCRIPT", "SUCCESS", f"{len(script)} chars", duration_ms
                )
            except Exception as e:
                duration_ms = int((time.time() - start_time) * 1000)
                self._log_operation("SCRIPT", "FAILED", str(e), duration_ms)
                raise

    # =========================================================================
    # Utility Methods
    # =========================================================================

    def get_value(
        self,
        query: str,
        params: Optional[Union[Dict[str, Any], Tuple]] = None,
        default: Any = None,
    ) -> Any:
        """Execute query and return single value (first column of first row).

        Args:
            query: SQL query expected to return single value
            params: Optional parameters
            default: Default value if no results

        Returns:
            Single value or default
        """
        results = self.execute_query(query, params)
        if results and results[0]:
            # Get first column value
            first_key = list(results[0].keys())[0]
            return results[0][first_key]
        return default

    def row_exists(
        self,
        table: str,
        conditions: Dict[str, Any],
    ) -> bool:
        """Check if a row exists matching conditions.

        Args:
            table: Table name
            conditions: Dict of column=value conditions (AND)

        Returns:
            True if row exists
        """
        where_clauses = [f"{col} = :{col}" for col in conditions.keys()]
        query = f"SELECT 1 FROM {table} WHERE {' AND '.join(where_clauses)} LIMIT 1"
        results = self.execute_query(query, conditions)
        return len(results) > 0

    def table_exists(self, table: str) -> bool:
        """Check if table exists in database.

        Args:
            table: Table name

        Returns:
            True if table exists
        """
        count = self.get_value(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=:name",
            {"name": table},
            0,
        )
        return count > 0

    def get_table_count(self, table: str) -> int:
        """Get row count for a table.

        Args:
            table: Table name

        Returns:
            Number of rows
        """
        return self.get_value(f"SELECT COUNT(*) FROM {table}", default=0)

    # =========================================================================
    # Health Check Methods
    # =========================================================================

    def check_integrity(self) -> bool:
        """Check database integrity.

        Returns:
            True if integrity check passes
        """
        start_time = time.time()
        with self.connection() as conn:
            cursor = conn.execute("PRAGMA integrity_check")
            result = cursor.fetchone()[0]
            duration_ms = int((time.time() - start_time) * 1000)

            if result == "ok":
                self._log_operation("INTEGRITY_CHECK", "PASSED", "", duration_ms)
                return True
            else:
                self._log_operation("INTEGRITY_CHECK", "FAILED", result, duration_ms)
                return False

    def get_schema_version(self) -> int:
        """Get current schema version.

        Returns:
            Schema version number (0 if not found)
        """
        try:
            return self.get_value(
                "SELECT COALESCE(MAX(version), 0) as version FROM schema_version",
                default=0,
            )
        except sqlite3.OperationalError:
            return 0

    def get_db_size(self) -> int:
        """Get database file size in bytes.

        Returns:
            Size in bytes
        """
        try:
            return Path(self.config.db_path).stat().st_size
        except FileNotFoundError:
            return 0

    def get_wal_size(self) -> int:
        """Get WAL file size in bytes.

        Returns:
            Size in bytes (0 if no WAL file)
        """
        wal_path = Path(self.config.db_path + "-wal")
        try:
            return wal_path.stat().st_size
        except FileNotFoundError:
            return 0

    # =========================================================================
    # Maintenance Methods
    # =========================================================================

    def vacuum(self) -> None:
        """Run VACUUM to reclaim space."""
        start_time = time.time()
        with self.connection() as conn:
            conn.execute("VACUUM")
        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation("VACUUM", "SUCCESS", "", duration_ms)

    def analyze(self) -> None:
        """Run ANALYZE to update query planner statistics."""
        start_time = time.time()
        with self.connection() as conn:
            conn.execute("ANALYZE")
        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation("ANALYZE", "SUCCESS", "", duration_ms)

    def checkpoint(self, mode: str = "PASSIVE") -> None:
        """Run WAL checkpoint.

        Args:
            mode: Checkpoint mode (PASSIVE, FULL, RESTART, TRUNCATE)
        """
        start_time = time.time()
        with self.connection() as conn:
            conn.execute(f"PRAGMA wal_checkpoint({mode})")
        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation("CHECKPOINT", "SUCCESS", f"mode={mode}", duration_ms)

    def rebuild_fts(self, fts_table: str) -> None:
        """Rebuild FTS5 index.

        Args:
            fts_table: Name of FTS5 virtual table
        """
        start_time = time.time()
        with self.connection() as conn:
            conn.execute(f"INSERT INTO {fts_table}({fts_table}) VALUES('rebuild')")
        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation("FTS_REBUILD", "SUCCESS", fts_table, duration_ms)

    def optimize_fts(self, fts_table: str) -> None:
        """Optimize FTS5 index.

        Args:
            fts_table: Name of FTS5 virtual table
        """
        start_time = time.time()
        with self.connection() as conn:
            conn.execute(f"INSERT INTO {fts_table}({fts_table}) VALUES('optimize')")
        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation("FTS_OPTIMIZE", "SUCCESS", fts_table, duration_ms)

    # =========================================================================
    # Backup Methods
    # =========================================================================

    def backup(self, backup_path: Optional[str] = None) -> str:
        """Create database backup.

        Args:
            backup_path: Optional path for backup file

        Returns:
            Path to backup file
        """
        if backup_path is None:
            backup_dir = Path(self.config.db_path).parent / "backups"
            backup_dir.mkdir(parents=True, exist_ok=True)
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            backup_path = str(backup_dir / f"codeflow-{timestamp}.db")

        start_time = time.time()
        with self.connection() as conn:
            backup_conn = sqlite3.connect(backup_path)
            try:
                conn.backup(backup_conn)
            finally:
                backup_conn.close()

        duration_ms = int((time.time() - start_time) * 1000)
        self._log_operation("BACKUP", "SUCCESS", backup_path, duration_ms)
        return backup_path


# =============================================================================
# Singleton Instance
# =============================================================================

_db_ops: Optional[DatabaseOperations] = None


def get_db_operations(config: Optional[DatabaseConfig] = None) -> DatabaseOperations:
    """Get or create singleton DatabaseOperations instance.

    Args:
        config: Optional configuration (creates new instance if provided)

    Returns:
        DatabaseOperations instance
    """
    global _db_ops
    if _db_ops is None or config is not None:
        _db_ops = DatabaseOperations(config)
    return _db_ops


# =============================================================================
# Exports
# =============================================================================

__all__ = [
    "DatabaseConfig",
    "DatabaseOperations",
    "get_db_operations",
]
