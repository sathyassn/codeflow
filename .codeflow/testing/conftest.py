"""
conftest.py - Root pytest configuration and fixtures for CodeFlow testing.

Provides shared fixtures for all test modules including:
- Subprocess runner with proper PYTHONPATH
- Temporary directories
- Database fixtures
"""

import os
import subprocess
import sys
import tempfile
import shutil
from pathlib import Path
from typing import Any, Callable, Dict, Generator, List, Optional

import pytest


# =============================================================================
# PATH CONFIGURATION
# =============================================================================

# Get paths relative to this conftest.py
TESTING_DIR = Path(__file__).parent
CODEFLOW_DIR = TESTING_DIR.parent
SCRIPTS_DIR = CODEFLOW_DIR / "scripts"
REPO_ROOT = CODEFLOW_DIR.parent

# Paths that need to be in PYTHONPATH for subprocess calls
# NOTE: Only include parent directories, NOT codeflow_py_lib directly
# Adding codeflow_py_lib to PYTHONPATH causes its logging.py to shadow stdlib logging
PYTHON_PATHS = [
    str(SCRIPTS_DIR),
    str(SCRIPTS_DIR / "db" / "lib"),
]

# Add paths for imports in test files
# Only add parent directories to avoid shadowing stdlib modules
for path in PYTHON_PATHS:
    if path not in sys.path:
        sys.path.insert(0, path)


# =============================================================================
# SUBPROCESS FIXTURES
# =============================================================================

@pytest.fixture
def script_env() -> Dict[str, str]:
    """Get environment variables with proper PYTHONPATH for subprocess calls.

    Returns:
        Dictionary of environment variables with PYTHONPATH set correctly.
    """
    env = os.environ.copy()
    existing_path = env.get("PYTHONPATH", "")
    new_paths = ":".join(PYTHON_PATHS)
    if existing_path:
        env["PYTHONPATH"] = f"{new_paths}:{existing_path}"
    else:
        env["PYTHONPATH"] = new_paths
    return env


@pytest.fixture
def run_script(script_env: Dict[str, str]) -> Callable[..., subprocess.CompletedProcess]:
    """Fixture providing a function to run Python scripts with proper PYTHONPATH.

    This fixture solves the module import issue when running scripts as subprocesses.

    Args:
        script_env: Environment with PYTHONPATH configured.

    Returns:
        Callable that runs a script and returns CompletedProcess.

    Example:
        def test_script(run_script):
            result = run_script("path/to/script.py", "--arg", "value")
            assert result.returncode == 0
    """
    def _run_script(
        script_path: str,
        *args: str,
        input: Optional[str] = None,
        timeout: Optional[float] = 30.0,
        check: bool = False,
    ) -> subprocess.CompletedProcess:
        """Run a Python script with proper environment.

        Args:
            script_path: Path to the Python script.
            *args: Arguments to pass to the script.
            input: Input to send to stdin.
            timeout: Timeout in seconds.
            check: If True, raise on non-zero exit.

        Returns:
            CompletedProcess instance with stdout, stderr, and returncode.
        """
        cmd = [sys.executable, str(script_path)] + list(args)
        return subprocess.run(
            cmd,
            env=script_env,
            capture_output=True,
            text=True,
            input=input,
            timeout=timeout,
            check=check,
        )

    return _run_script


@pytest.fixture
def run_bash_script(script_env: Dict[str, str]) -> Callable[..., subprocess.CompletedProcess]:
    """Fixture providing a function to run bash scripts with proper environment.

    Args:
        script_env: Environment with PYTHONPATH configured.

    Returns:
        Callable that runs a bash script and returns CompletedProcess.
    """
    def _run_bash(
        script_path: str,
        *args: str,
        input: Optional[str] = None,
        timeout: Optional[float] = 30.0,
        check: bool = False,
    ) -> subprocess.CompletedProcess:
        """Run a bash script with proper environment.

        Args:
            script_path: Path to the bash script.
            *args: Arguments to pass to the script.
            input: Input to send to stdin.
            timeout: Timeout in seconds.
            check: If True, raise on non-zero exit.

        Returns:
            CompletedProcess instance with stdout, stderr, and returncode.
        """
        cmd = ["bash", str(script_path)] + list(args)
        return subprocess.run(
            cmd,
            env=script_env,
            capture_output=True,
            text=True,
            input=input,
            timeout=timeout,
            check=check,
        )

    return _run_bash


# =============================================================================
# TEMPORARY DIRECTORY FIXTURES
# =============================================================================

@pytest.fixture
def temp_dir() -> Generator[Path, None, None]:
    """Create a temporary directory for tests.

    Yields:
        Path to temporary directory that is cleaned up after test.
    """
    temp = tempfile.mkdtemp(prefix="codeflow_test_")
    yield Path(temp)
    shutil.rmtree(temp, ignore_errors=True)


@pytest.fixture
def mock_repo_root(temp_dir: Path) -> Path:
    """Create a mock repository root with standard structure.

    Args:
        temp_dir: Temporary directory fixture.

    Returns:
        Path to mock repo root with .codeflow, .state directories.
    """
    # Create standard directory structure
    (temp_dir / ".codeflow" / "scripts").mkdir(parents=True)
    (temp_dir / ".codeflow" / "config").mkdir(parents=True)
    (temp_dir / ".state" / "db").mkdir(parents=True)
    (temp_dir / ".state" / "ledger").mkdir(parents=True)
    (temp_dir / ".state" / "logs").mkdir(parents=True)
    (temp_dir / ".claude" / "memory").mkdir(parents=True)
    return temp_dir


# =============================================================================
# PATH FIXTURES
# =============================================================================

@pytest.fixture
def scripts_path() -> Path:
    """Get the path to the scripts directory.

    Returns:
        Path to .codeflow/scripts
    """
    return SCRIPTS_DIR


@pytest.fixture
def testing_path() -> Path:
    """Get the path to the testing directory.

    Returns:
        Path to .codeflow/testing
    """
    return TESTING_DIR


@pytest.fixture
def repo_root() -> Path:
    """Get the path to the repository root.

    Returns:
        Path to repository root
    """
    return REPO_ROOT
