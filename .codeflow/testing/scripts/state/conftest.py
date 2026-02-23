"""
conftest.py - pytest fixtures for state script tests.

Provides common test fixtures for stage-sync and other state scripts.
"""

import json
import shutil
import sqlite3
import sys
import tempfile
from pathlib import Path
from typing import Any, Dict, Generator

import pytest

# Add paths for imports
SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
DB_LIB_PATH = SCRIPTS_PATH / "db" / "lib"
sys.path.insert(0, str(SCRIPTS_PATH))
sys.path.insert(0, str(DB_LIB_PATH))
sys.path.insert(0, str(SCRIPTS_PATH / "codeflow_py_lib"))


# Schema with stage pipeline columns (V4)
STATE_TEST_SCHEMA = """
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS epics (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'planning',
    created_at TEXT DEFAULT (datetime('now')),
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    epic_id TEXT,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'todo'
        CHECK(status IN ('todo', 'in_progress', 'complete', 'blocked')),
    stage TEXT CHECK(stage IN ('dev', 'work', 'review', 'qa', 'done') OR stage IS NULL),
    stage_status TEXT CHECK(stage_status IN ('pending', 'in_progress', 'complete', 'failed') OR stage_status IS NULL),
    stage_history TEXT DEFAULT '[]',
    created_at TEXT DEFAULT (datetime('now')),
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (epic_id) REFERENCES epics(id)
);

CREATE TABLE IF NOT EXISTS active_work (
    id TEXT PRIMARY KEY,
    task_id TEXT,
    topic TEXT,
    branch TEXT,
    scope TEXT DEFAULT '[]',
    scope_policy TEXT DEFAULT 'soft',
    current_stage TEXT,
    status TEXT DEFAULT 'in_progress'
        CHECK(status IN ('in_progress', 'complete', 'abandoned')),
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
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

INSERT INTO schema_version (version) VALUES (1);
"""


@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    """Create a temporary directory for tests."""
    temp = tempfile.mkdtemp(prefix="codeflow_state_test_")
    yield Path(temp)
    shutil.rmtree(temp, ignore_errors=True)


@pytest.fixture
def mock_repo(temp_dir: Path) -> Path:
    """Create a mock repo structure with .state/ and project-management/epics/."""
    repo = temp_dir / "repo"
    (repo / ".state" / "db").mkdir(parents=True)
    (repo / ".state" / "ledger").mkdir(parents=True)
    (repo / "project-management" / "epics").mkdir(parents=True)
    (repo / ".claude" / "memory" / "development").mkdir(parents=True)
    return repo


@pytest.fixture
def state_db(mock_repo: Path) -> Path:
    """Create an initialized test database with stage columns."""
    db_path = mock_repo / ".state" / "db" / "codeflow.db"
    conn = sqlite3.connect(str(db_path))
    conn.executescript(STATE_TEST_SCHEMA)
    conn.close()
    return db_path


@pytest.fixture
def db_conn(state_db: Path) -> Generator[sqlite3.Connection, None, None]:
    """SQLite connection for direct test queries."""
    conn = sqlite3.connect(str(state_db))
    conn.row_factory = sqlite3.Row
    yield conn
    conn.close()


@pytest.fixture
def sample_task(db_conn: sqlite3.Connection) -> Dict[str, Any]:
    """Insert a sample task and return its data."""
    task = {
        "id": "FRT-TSK-001-001",
        "epic_id": "EPC-TEST-001",
        "title": "Test feature task",
        "status": "in_progress",
        "stage": "dev",
        "stage_status": "in_progress",
        "stage_history": json.dumps([
            {"stage": "dev", "status": "in_progress", "started_at": "2026-01-01T00:00:00+00:00"}
        ]),
    }
    db_conn.execute(
        "INSERT INTO epics (id, title) VALUES (?, ?)",
        ("EPC-TEST-001", "Test Epic"),
    )
    db_conn.execute(
        """INSERT INTO tasks (id, epic_id, title, status, stage, stage_status, stage_history)
           VALUES (:id, :epic_id, :title, :status, :stage, :stage_status, :stage_history)""",
        task,
    )
    db_conn.commit()
    return task


@pytest.fixture
def sample_active_work(db_conn: sqlite3.Connection, sample_task: Dict[str, Any]) -> Dict[str, Any]:
    """Insert a sample active_work record linked to the sample task."""
    work = {
        "id": "AWK-TEST-001",
        "task_id": sample_task["id"],
        "topic": "Test work",
        "branch": "feat/test",
        "current_stage": "dev",
        "status": "in_progress",
    }
    db_conn.execute(
        """INSERT INTO active_work (id, task_id, topic, branch, current_stage, status)
           VALUES (:id, :task_id, :topic, :branch, :current_stage, :status)""",
        work,
    )
    db_conn.commit()
    return work
