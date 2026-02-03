"""
conftest.py - pytest fixtures for coordination script tests.

Provides common test fixtures for coordination CRDT and claim testing.
"""

import json
import shutil
import sqlite3
import sys
import tempfile
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Dict, Generator

import pytest

# Add paths for imports
SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
sys.path.insert(0, str(SCRIPTS_PATH))
sys.path.insert(0, str(SCRIPTS_PATH / "codeflow_py_lib"))


# Schema for coordination tests
COORDINATION_SCHEMA = """
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS work_claims (
    id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL,
    pattern TEXT NOT NULL,
    mode TEXT DEFAULT 'exclusive',
    owner_id TEXT NOT NULL,
    fencing_token INTEGER DEFAULT 0,
    expires_at TEXT,
    status TEXT DEFAULT 'active',
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_work_claims_pattern ON work_claims(pattern);
CREATE INDEX IF NOT EXISTS idx_work_claims_owner ON work_claims(owner_id);
CREATE INDEX IF NOT EXISTS idx_work_claims_status ON work_claims(status);

INSERT INTO schema_version (version) VALUES (1);
"""


@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    """Create a temporary directory for tests.

    Yields:
        Path to temporary directory that is cleaned up after test.
    """
    temp = tempfile.mkdtemp(prefix="codeflow_coord_test_")
    yield Path(temp)
    shutil.rmtree(temp, ignore_errors=True)


@pytest.fixture
def mock_state_dir(temp_dir: Path) -> Path:
    """Create mock state directory structure."""
    state_dir = temp_dir / ".state"
    (state_dir / "db").mkdir(parents=True)
    (state_dir / "ledger").mkdir(parents=True)
    (state_dir / "coordination").mkdir(parents=True)
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
    conn.executescript(COORDINATION_SCHEMA)
    conn.close()
    return db_path


@pytest.fixture
def mock_db_ops(temp_dir: Path) -> Any:
    """Mock DatabaseOperations for testing scripts.

    Args:
        temp_dir: Temporary directory fixture.

    Returns:
        Configured DatabaseOperations instance.
    """
    from db_operations import DatabaseConfig, DatabaseOperations

    db_path = str(temp_dir / "test.db")
    log_path = str(temp_dir / "ops.jsonl")
    config = DatabaseConfig(db_path=db_path, op_log_path=log_path)
    ops = DatabaseOperations(config)

    # Initialize schema
    with ops.connection() as conn:
        conn.executescript(COORDINATION_SCHEMA)

    return ops


@pytest.fixture
def sample_claim() -> Dict[str, Any]:
    """Sample claim data.

    Returns:
        Dictionary containing sample claim fields.
    """
    now = datetime.utcnow()
    expires = now + timedelta(seconds=600)
    return {
        "id": "claim-test-001",
        "work_id": "TSK-001",
        "pattern": "file:src/*.ts",
        "mode": "exclusive",
        "owner_id": "agent-001",
        "fencing_token": 1,
        "expires_at": expires.isoformat() + "Z",
        "status": "active",
        "created_at": now.isoformat() + "Z",
    }


@pytest.fixture
def mock_coordination_doc(temp_dir: Path) -> Any:
    """Create a mock CoordinationDoc with sample data.

    Args:
        temp_dir: Temporary directory fixture.

    Returns:
        CoordinationDoc instance with sample claims.
    """
    from codeflow_py_lib.crdt import CoordinationDoc

    doc = CoordinationDoc()
    now = datetime.utcnow()
    expires = now + timedelta(seconds=600)

    # Add some sample claims
    doc._claims = {
        "claim-001": {
            "id": "claim-001",
            "work_id": "TSK-001",
            "pattern": "file:src/*.ts",
            "mode": "exclusive",
            "owner_id": "agent-001",
            "fencing_token": 1,
            "expires_at": expires.isoformat() + "Z",
            "status": "active",
            "created_at": now.isoformat() + "Z",
        },
        "claim-002": {
            "id": "claim-002",
            "work_id": "TSK-002",
            "pattern": "dir:src/auth",
            "mode": "shared",
            "owner_id": "agent-002",
            "fencing_token": 2,
            "expires_at": expires.isoformat() + "Z",
            "status": "active",
            "created_at": now.isoformat() + "Z",
        },
    }
    doc._token_counter = 2

    return doc


@pytest.fixture
def mock_jsonl_ledger(mock_state_dir: Path) -> Path:
    """Create a mock JSONL ledger with claim events.

    Args:
        mock_state_dir: Mock state directory fixture.

    Returns:
        Path to the created JSONL ledger file.
    """
    ledger_path = mock_state_dir / "ledger" / "sessions.jsonl"
    now = datetime.utcnow()
    expires = now + timedelta(seconds=600)

    events = [
        {
            "type": "claim_created",
            "id": "claim-001",
            "work_id": "TSK-001",
            "pattern": "file:src/*.ts",
            "mode": "exclusive",
            "owner_id": "agent-001",
            "fencing_token": 1,
            "expires_at": expires.isoformat() + "Z",
            "created_at": now.isoformat() + "Z",
        },
        {
            "type": "claim_created",
            "id": "claim-002",
            "work_id": "TSK-002",
            "pattern": "dir:src/auth",
            "mode": "shared",
            "owner_id": "agent-002",
            "fencing_token": 2,
            "expires_at": expires.isoformat() + "Z",
            "created_at": now.isoformat() + "Z",
        },
        {
            "type": "claim_released",
            "claim_id": "claim-001",
            "released_at": now.isoformat() + "Z",
        },
    ]

    with open(ledger_path, "w") as f:
        for event in events:
            f.write(json.dumps(event) + "\n")

    return ledger_path
