"""
conftest.py - pytest fixtures for codeflow_py_lib tests.
"""

import json
import os
import shutil
import tempfile
from pathlib import Path

import pytest

# Note: pythonpath is configured in pytest.ini to include ../scripts
# which makes codeflow_py_lib available as a package


@pytest.fixture
def temp_dir():
    """Create a temporary directory for tests."""
    temp = tempfile.mkdtemp(prefix="codeflow_test_")
    yield Path(temp)
    shutil.rmtree(temp, ignore_errors=True)


@pytest.fixture
def mock_repo_root(temp_dir):
    """Create a mock repository structure."""
    # Create .codeflow directory to mark as repo root
    (temp_dir / ".codeflow").mkdir()
    (temp_dir / ".state").mkdir()
    (temp_dir / ".state" / "db").mkdir()
    (temp_dir / ".state" / "ledger").mkdir()
    (temp_dir / ".state" / "logs").mkdir()

    # Set environment variable for tests
    original_cwd = os.getcwd()
    os.chdir(str(temp_dir))

    yield temp_dir

    os.chdir(original_cwd)


@pytest.fixture
def mock_db_path(temp_dir):
    """Create a temporary database path."""
    db_path = temp_dir / "test.db"
    yield db_path
    if db_path.exists():
        db_path.unlink()


@pytest.fixture
def sample_memory_event():
    """Sample memory event data."""
    return {
        "event_type": "progress",
        "domain": "development",
        "content": "Implemented user authentication",
        "work_id": "TSK-01HQXYZ123456789ABCDEFGH",
    }


@pytest.fixture
def sample_jsonl_data():
    """Sample JSONL data for testing."""
    return [
        {"ts": "2026-01-25T10:00:00Z", "e": "session_start", "sid": "ses-abc123"},
        {
            "ts": "2026-01-25T10:01:00Z",
            "e": "work_claimed",
            "sid": "ses-abc123",
            "wid": "TSK-001",
        },
        {
            "ts": "2026-01-25T10:30:00Z",
            "e": "progress",
            "sid": "ses-abc123",
            "d": "Implemented auth",
        },
    ]


@pytest.fixture
def mock_jsonl_file(temp_dir, sample_jsonl_data):
    """Create a mock JSONL file."""
    jsonl_path = temp_dir / "test.jsonl"
    with open(jsonl_path, "w") as f:
        for event in sample_jsonl_data:
            f.write(json.dumps(event) + "\n")
    yield jsonl_path


@pytest.fixture
def mock_config(temp_dir):
    """Create mock configuration files."""
    config_dir = temp_dir / ".codeflow" / "config"
    config_dir.mkdir(parents=True)

    # Create enforcement policy
    policy = {
        "enforcement_level": "warn",
        "protected_patterns": [".env*", "*.key"],
    }
    (config_dir / "enforcement-policy.json").write_text(json.dumps(policy))

    yield config_dir
