"""
conftest.py - pytest fixtures for database tests.

Provides common test fixtures for database operations testing including
temporary directories, initialized databases, and sample data.
"""

import shutil
import sqlite3
import sys
import tempfile
from pathlib import Path
from typing import Any, Dict, Generator

import pytest

# Add paths for imports - use codeflow_py_lib package name
SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
DB_LIB_PATH = SCRIPTS_PATH / "db" / "lib"
sys.path.insert(0, str(SCRIPTS_PATH))
sys.path.insert(0, str(DB_LIB_PATH))


@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    """Create a temporary directory for tests.

    Yields:
        Path to temporary directory that is cleaned up after test.
    """
    temp = tempfile.mkdtemp(prefix="codeflow_db_test_")
    yield Path(temp)
    shutil.rmtree(temp, ignore_errors=True)


@pytest.fixture
def test_db_path(temp_dir: Path) -> Path:
    """Create a temporary database file path.

    Args:
        temp_dir: Temporary directory fixture.

    Returns:
        Path to the test database file.
    """
    return temp_dir / "test_codeflow.db"


@pytest.fixture
def schema_path() -> Path:
    """Get the path to the schema file.

    Returns:
        Path to the schema.sql file in .state/db directory.
    """
    return (
        Path(__file__).parent.parent.parent.parent.parent
        / ".state"
        / "db"
        / "schema.sql"
    )


# Inline schema for tests (since schema.sql may not exist yet)
TEST_SCHEMA = """
-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

-- Memory events table
CREATE TABLE IF NOT EXISTS memory_events (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL CHECK(event_type IN ('progress', 'decision', 'learning', 'context', 'observation')),
    domain TEXT NOT NULL CHECK(domain IN ('development', 'infrastructure', 'documentation', 'testing', 'operations')),
    work_id TEXT,
    session_id TEXT,
    data TEXT NOT NULL DEFAULT '{}',
    memory_type TEXT DEFAULT 'episodic' CHECK(memory_type IN ('episodic', 'semantic', 'procedural')),
    created_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT
);

-- Work claims table
CREATE TABLE IF NOT EXISTS work_claims (
    id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    claimed_at TEXT DEFAULT (datetime('now')),
    released_at TEXT,
    status TEXT DEFAULT 'active' CHECK(status IN ('active', 'released', 'expired'))
);

-- Sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    started_at TEXT DEFAULT (datetime('now')),
    ended_at TEXT,
    status TEXT DEFAULT 'active' CHECK(status IN ('active', 'ended', 'crashed'))
);

-- Epics table
CREATE TABLE IF NOT EXISTS epics (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'planning' CHECK(status IN ('planning', 'in_progress', 'completed', 'cancelled')),
    created_at TEXT DEFAULT (datetime('now')),
    completed_at TEXT
);

-- Tasks table
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    epic_id TEXT,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'pending' CHECK(status IN ('pending', 'in_progress', 'completed', 'blocked')),
    created_at TEXT DEFAULT (datetime('now')),
    completed_at TEXT,
    FOREIGN KEY (epic_id) REFERENCES epics(id)
);

-- FTS5 for memory search
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    id,
    event_type,
    domain,
    data,
    content='memory_events',
    content_rowid='rowid'
);

-- Insert schema version
INSERT INTO schema_version (version) VALUES (1);
"""


@pytest.fixture
def initialized_db(
    test_db_path: Path, schema_path: Path
) -> Generator[Path, None, None]:
    """Create and initialize a test database.

    Args:
        test_db_path: Path to the test database file.
        schema_path: Path to the schema file.

    Yields:
        Path to the initialized test database.
    """
    conn = sqlite3.connect(str(test_db_path))
    if schema_path.exists():
        schema_sql = schema_path.read_text()
        conn.executescript(schema_sql)
    else:
        # Use inline schema if file doesn't exist
        conn.executescript(TEST_SCHEMA)
    conn.close()
    yield test_db_path


@pytest.fixture
def db_connection(initialized_db: Path) -> Generator[sqlite3.Connection, None, None]:
    """Create a database connection for testing.

    Args:
        initialized_db: Path to the initialized test database.

    Yields:
        SQLite connection with row factory and foreign keys enabled.
    """
    conn = sqlite3.connect(str(initialized_db))
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys=ON")
    yield conn
    conn.close()


@pytest.fixture
def sample_memory_event() -> Dict[str, Any]:
    """Sample memory event for testing.

    Returns:
        Dictionary containing sample memory event fields.
    """
    return {
        "id": "memory-01HQXYZ123456789ABCDEFGH",
        "event_type": "progress",
        "domain": "development",
        "work_id": None,
        "data": '{"content": "Test progress"}',
        "memory_type": "episodic",
    }
