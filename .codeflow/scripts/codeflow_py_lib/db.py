"""
Database operations re-export.

This module re-exports the DatabaseOperations class from db/lib/db_operations.py
for convenient access from codeflow_py_lib.
"""

import sys
from pathlib import Path

# Add db/lib to path for import
_db_lib_path = Path(__file__).parent.parent / "db" / "lib"
if str(_db_lib_path) not in sys.path:
    sys.path.insert(0, str(_db_lib_path))

# Try to import from db_operations, provide stub if not yet created
try:
    from db_operations import (
        DatabaseConfig,
        DatabaseOperations,
    )
    from db_operations import (
        get_db_operations as get_db,
    )
except ImportError:
    # Stub implementations until db_operations.py is created
    import sqlite3
    from contextlib import contextmanager
    from dataclasses import dataclass, field
    from typing import Any, Dict, List, Optional

    from .paths import get_repo_root

    @dataclass
    class DatabaseConfig:
        """Database configuration with defaults."""

        db_path: str = field(
            default_factory=lambda: str(get_repo_root() / ".state/db/codeflow.db")
        )
        timeout: float = 5.0
        retries: int = 3
        retry_delay: float = 0.5

    class DatabaseOperations:
        """Stub database operations class."""

        def __init__(self, config: Optional[DatabaseConfig] = None):
            self.config = config or DatabaseConfig()

        @contextmanager
        def connection(self):
            """Context manager for database connection."""
            conn = sqlite3.connect(
                self.config.db_path,
                timeout=self.config.timeout,
            )
            conn.row_factory = sqlite3.Row
            try:
                yield conn
            finally:
                conn.close()

        @contextmanager
        def transaction(self):
            """Context manager for transaction."""
            with self.connection() as conn:
                try:
                    conn.execute("BEGIN TRANSACTION")
                    yield conn
                    conn.execute("COMMIT")
                except Exception:
                    conn.execute("ROLLBACK")
                    raise

        def execute_query(
            self, query: str, params: Optional[Dict[str, Any]] = None
        ) -> List[Dict[str, Any]]:
            """Execute SELECT query and return results."""
            with self.connection() as conn:
                cursor = conn.execute(query, params or {})
                return [dict(row) for row in cursor.fetchall()]

        def execute_write(
            self, statement: str, params: Optional[Dict[str, Any]] = None
        ) -> int:
            """Execute INSERT/UPDATE/DELETE and return rows affected."""
            with self.connection() as conn:
                cursor = conn.execute(statement, params or {})
                conn.commit()
                return cursor.rowcount

        def get_value(
            self, query: str, params: Optional[Dict[str, Any]] = None,
            default: Any = None
        ) -> Any:
            """Execute query and return single value."""
            results = self.execute_query(query, params)
            if results and results[0]:
                first_key = list(results[0].keys())[0]
                return results[0][first_key]
            return default

        def table_exists(self, table: str) -> bool:
            """Check if table exists in database."""
            count = self.get_value(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=:name",
                {"name": table}, 0,
            )
            return count > 0

        def row_exists(self, table: str, conditions: Dict[str, Any]) -> bool:
            """Check if a row exists matching conditions."""
            where_clauses = [f"{col} = :{col}" for col in conditions.keys()]
            query = f"SELECT 1 FROM {table} WHERE {' AND '.join(where_clauses)} LIMIT 1"
            results = self.execute_query(query, conditions)
            return len(results) > 0

    _db_ops: Optional[DatabaseOperations] = None

    def get_db(config: Optional[DatabaseConfig] = None) -> DatabaseOperations:
        """Get or create singleton DatabaseOperations instance."""
        global _db_ops
        if _db_ops is None or config is not None:
            _db_ops = DatabaseOperations(config)
        return _db_ops


# Helper function for dict_factory
def dict_factory(cursor, row):
    """Convert sqlite3 row to dictionary."""
    return {col[0]: row[idx] for idx, col in enumerate(cursor.description)}


__all__ = ["DatabaseOperations", "DatabaseConfig", "get_db", "dict_factory"]
