"""
Path utilities for CodeFlow scripts.

Provides consistent path resolution across all scripts.
"""

import json
import os
import subprocess
from functools import lru_cache
from pathlib import Path


@lru_cache(maxsize=1)
def get_repo_root() -> Path:
    """
    Get repository root directory.

    Tries in order:
    1. CODEFLOW_REPO_ROOT environment variable
    2. git rev-parse --show-toplevel
    3. Walk up looking for .codeflow directory
    4. Current working directory
    """
    # Check environment variable
    env_root = os.environ.get("CODEFLOW_REPO_ROOT")
    if env_root:
        return Path(env_root)

    # Try git
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0:
            return Path(result.stdout.strip())
    except (subprocess.SubprocessError, FileNotFoundError):
        pass

    # Walk up looking for .codeflow
    current = Path.cwd()
    while current != current.parent:
        if (current / ".codeflow").is_dir():
            return current
        current = current.parent

    # Fall back to cwd
    return Path.cwd()


def get_state_dir() -> Path:
    """Get .state directory path."""
    return get_repo_root() / ".state"


def get_config_dir() -> Path:
    """Get .codeflow/config directory path."""
    return get_repo_root() / ".codeflow" / "config"


def get_scripts_dir() -> Path:
    """Get .codeflow/scripts directory path."""
    return get_repo_root() / ".codeflow" / "scripts"


def get_ledger_dir() -> Path:
    """Get .state/ledger directory path."""
    return get_state_dir() / "ledger"


def get_logs_dir() -> Path:
    """Get .state/logs directory path."""
    return get_state_dir() / "logs"


def get_db_path() -> Path:
    """Get SQLite database path."""
    return get_state_dir() / "db" / "codeflow.db"


def ensure_dir(path: Path) -> Path:
    """Ensure directory exists, creating if necessary."""
    path.mkdir(parents=True, exist_ok=True)
    return path


def relative_to_repo(path: Path) -> str:
    """Get path relative to repo root for display."""
    try:
        return str(path.relative_to(get_repo_root()))
    except ValueError:
        return str(path)


def get_pathflow_setting() -> str:
    """Read pathflow_mode setting from settings.json.

    Returns:
        The pathflow_mode value ("auto", "always", "never") or "" if not set.
    """
    settings_file = get_repo_root() / ".claude" / "settings.json"
    if not settings_file.exists():
        return ""
    try:
        data = json.loads(settings_file.read_text())
        return data.get("_codeflow", {}).get("pathflow_mode", "")
    except (json.JSONDecodeError, OSError):
        return ""


def is_pathflow_active() -> bool:
    """Check if PathFlow mode is active using 3-way priority.

    Mirrors the shell function is_pathflow_active() in context-lib.sh.
    Priority: env override > settings.json > flag file.

    Returns:
        True if PathFlow mode is active, False otherwise.
    """
    setting = os.environ.get("CODEFLOW_PATHFLOW_OVERRIDE", "")
    if not setting:
        setting = get_pathflow_setting()
    if setting == "never":
        return False
    elif setting == "always":
        return True
    else:
        session_id = os.environ.get("CODEFLOW_SESSION_ID", "unknown")
        flag = get_state_dir() / "session" / session_id / "is-pathflow-active"
        return flag.exists()
