"""
conftest.py - pytest fixtures for memory script tests.

Provides common test fixtures for memory operations testing including
temporary directories, mock state directories, and sample memory events.
"""

import shutil
import sqlite3
import sys
import tempfile
from pathlib import Path
from typing import Any, Dict, Generator

import pytest

# Add paths for imports
# NOTE: Only add scripts dir, NOT codeflow_py_lib directly (would shadow stdlib logging)
SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
sys.path.insert(0, str(SCRIPTS_PATH))
sys.path.insert(0, str(SCRIPTS_PATH / "db" / "lib"))


# Schema for memory tests
MEMORY_SCHEMA = """
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS memory_events (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    work_id TEXT,
    session_id TEXT,
    data TEXT NOT NULL DEFAULT '{}',
    memory_type TEXT DEFAULT 'episodic',
    created_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    id,
    event_type,
    domain,
    data,
    content='memory_events',
    content_rowid='rowid'
);

-- Triggers for FTS sync
CREATE TRIGGER IF NOT EXISTS memory_events_ai AFTER INSERT ON memory_events BEGIN
    INSERT INTO memory_fts(rowid, id, event_type, domain, data)
    VALUES (new.rowid, new.id, new.event_type, new.domain, new.data);
END;

CREATE TRIGGER IF NOT EXISTS memory_events_ad AFTER DELETE ON memory_events BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, id, event_type, domain, data)
    VALUES ('delete', old.rowid, old.id, old.event_type, old.domain, old.data);
END;

INSERT INTO schema_version (version) VALUES (1);
"""


@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    """Create a temporary directory for tests.

    Yields:
        Path to temporary directory that is cleaned up after test.
    """
    temp = tempfile.mkdtemp(prefix="codeflow_memory_test_")
    yield Path(temp)
    shutil.rmtree(temp, ignore_errors=True)


@pytest.fixture
def mock_state_dir(temp_dir: Path) -> Path:
    """Create mock state directory structure.

    Args:
        temp_dir: Temporary directory fixture.

    Returns:
        Path to the mock .state directory with db, ledger, and logs subdirs.
    """
    state_dir = temp_dir / ".state"
    (state_dir / "db").mkdir(parents=True)
    (state_dir / "ledger").mkdir(parents=True)
    (state_dir / "logs").mkdir(parents=True)
    return state_dir


@pytest.fixture
def test_db(mock_state_dir: Path) -> Path:
    """Create an initialized test database.

    Args:
        mock_state_dir: Mock state directory fixture.

    Returns:
        Path to the initialized test database.
    """
    db_path = mock_state_dir / "db" / "codeflow.db"
    conn = sqlite3.connect(str(db_path))
    conn.executescript(MEMORY_SCHEMA)
    conn.close()
    return db_path


@pytest.fixture
def populated_db(test_db: Path) -> Path:
    """Database with sample memory events.

    Args:
        test_db: Initialized test database fixture.

    Returns:
        Path to database with sample events inserted.
    """
    conn = sqlite3.connect(str(test_db))

    # Insert sample memory events
    events = [
        (
            "memory-001",
            "progress",
            "development",
            "TSK-001",
            '{"content": "Implemented user authentication"}',
            "episodic",
        ),
        (
            "memory-002",
            "decision",
            "planning",
            "TSK-001",
            '{"content": "Decided to use JWT tokens"}',
            "semantic",
        ),
        (
            "memory-003",
            "progress",
            "development",
            "TSK-002",
            '{"content": "Added unit tests"}',
            "episodic",
        ),
        (
            "memory-004",
            "milestone",
            "development",
            "TSK-001",
            '{"content": "Authentication feature complete"}',
            "episodic",
        ),
        (
            "memory-005",
            "blocker",
            "qa",
            "TSK-003",
            '{"content": "Test environment down"}',
            "episodic",
        ),
    ]

    conn.executemany(
        """
        INSERT INTO memory_events (id, event_type, domain, work_id, data, memory_type)
        VALUES (?, ?, ?, ?, ?, ?)
    """,
        events,
    )
    conn.commit()
    conn.close()

    return test_db


@pytest.fixture
def mock_db_ops(temp_dir: Path) -> Any:
    """Mock DatabaseOperations for testing scripts.

    Args:
        temp_dir: Temporary directory fixture.

    Returns:
        Configured DatabaseOperations instance with MEMORY_SCHEMA initialized.
    """
    from db_operations import DatabaseConfig, DatabaseOperations

    db_path = str(temp_dir / "test.db")
    log_path = str(temp_dir / "ops.jsonl")
    config = DatabaseConfig(db_path=db_path, op_log_path=log_path)
    ops = DatabaseOperations(config)

    # Initialize schema
    with ops.connection() as conn:
        conn.executescript(MEMORY_SCHEMA)

    return ops


@pytest.fixture
def sample_memory_event() -> Dict[str, Any]:
    """Sample memory event data.

    Returns:
        Dictionary containing sample memory event fields.
    """
    return {
        "event_type": "progress",
        "domain": "development",
        "content": "Implemented feature X",
        "work_id": "TSK-001",
        "memory_type": "episodic",
    }
