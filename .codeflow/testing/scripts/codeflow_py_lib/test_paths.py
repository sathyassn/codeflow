"""
test_paths.py - Tests for path utilities module.

Tests get_repo_root, get_state_dir, get_config_dir, and other path utilities.
"""

import json
import os
from pathlib import Path
from unittest.mock import patch

import pytest
from codeflow_py_lib.paths import (
    ensure_dir,
    get_config_dir,
    get_db_path,
    get_ledger_dir,
    get_logs_dir,
    get_pathflow_setting,
    get_repo_root,
    get_scripts_dir,
    get_state_dir,
    is_pathflow_active,
    relative_to_repo,
)


@pytest.fixture
def temp_repo(tmp_path):
    """Create a temporary repository structure."""
    # Create .codeflow directory to mark as repo root
    (tmp_path / ".codeflow").mkdir()
    (tmp_path / ".state").mkdir()
    (tmp_path / ".state" / "db").mkdir()
    (tmp_path / ".state" / "ledger").mkdir()
    (tmp_path / ".state" / "logs").mkdir()
    return tmp_path


@pytest.fixture
def reset_repo_root_cache():
    """Clear the lru_cache for get_repo_root between tests."""
    get_repo_root.cache_clear()
    yield
    get_repo_root.cache_clear()


class TestGetRepoRoot:
    """Tests for get_repo_root function."""

    def test_returns_path(self, reset_repo_root_cache):
        """Should return a Path object."""
        result = get_repo_root()
        assert isinstance(result, Path)

    def test_respects_env_variable(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should use CODEFLOW_REPO_ROOT env variable if set."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_repo_root()
        assert result == temp_repo

    def test_finds_codeflow_directory(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should find repo root by walking up to .codeflow directory."""
        # Clear env variable
        monkeypatch.delenv("CODEFLOW_REPO_ROOT", raising=False)

        # Create nested directory and change to it
        nested = temp_repo / "src" / "deep" / "nested"
        nested.mkdir(parents=True)

        # Mock git to fail
        with patch("subprocess.run") as mock_run:
            mock_run.return_value.returncode = 1

            # Change to nested directory
            original_cwd = os.getcwd()
            os.chdir(str(nested))
            try:
                result = get_repo_root()
                assert result == temp_repo
            finally:
                os.chdir(original_cwd)

    def test_result_is_cached(self, reset_repo_root_cache):
        """Should cache the result."""
        result1 = get_repo_root()
        result2 = get_repo_root()
        # Same object due to caching
        assert result1 is result2


class TestGetStateDir:
    """Tests for get_state_dir function."""

    def test_returns_state_subdir(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should return .state under repo root."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_state_dir()
        assert result == temp_repo / ".state"

    def test_returns_path(self, reset_repo_root_cache):
        """Should return a Path object."""
        result = get_state_dir()
        assert isinstance(result, Path)


class TestGetConfigDir:
    """Tests for get_config_dir function."""

    def test_returns_config_subdir(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should return .codeflow/config under repo root."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_config_dir()
        assert result == temp_repo / ".codeflow" / "config"


class TestGetScriptsDir:
    """Tests for get_scripts_dir function."""

    def test_returns_scripts_subdir(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should return .codeflow/scripts under repo root."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_scripts_dir()
        assert result == temp_repo / ".codeflow" / "scripts"


class TestGetLedgerDir:
    """Tests for get_ledger_dir function."""

    def test_returns_ledger_subdir(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should return .state/ledger under repo root."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_ledger_dir()
        assert result == temp_repo / ".state" / "ledger"


class TestGetLogsDir:
    """Tests for get_logs_dir function."""

    def test_returns_logs_subdir(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should return .state/logs under repo root."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_logs_dir()
        assert result == temp_repo / ".state" / "logs"


class TestGetDbPath:
    """Tests for get_db_path function."""

    def test_returns_db_path(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should return path to codeflow.db."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_db_path()
        assert result == temp_repo / ".state" / "db" / "codeflow.db"


class TestEnsureDir:
    """Tests for ensure_dir function."""

    def test_creates_directory(self, tmp_path):
        """Should create directory if it doesn't exist."""
        new_dir = tmp_path / "new" / "nested" / "dir"
        result = ensure_dir(new_dir)
        assert new_dir.exists()
        assert new_dir.is_dir()
        assert result == new_dir

    def test_returns_existing_directory(self, tmp_path):
        """Should return existing directory without error."""
        existing = tmp_path / "existing"
        existing.mkdir()
        result = ensure_dir(existing)
        assert result == existing

    def test_creates_parents(self, tmp_path):
        """Should create parent directories."""
        deep = tmp_path / "a" / "b" / "c" / "d"
        ensure_dir(deep)
        assert deep.exists()
        assert (tmp_path / "a" / "b" / "c").exists()


class TestRelativeToRepo:
    """Tests for relative_to_repo function."""

    def test_returns_relative_path(self, temp_repo, reset_repo_root_cache, monkeypatch):
        """Should return path relative to repo root."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        full_path = temp_repo / "src" / "main.py"
        result = relative_to_repo(full_path)
        assert result == "src/main.py"

    def test_handles_path_outside_repo(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should return full path if outside repo."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        outside_path = Path("/some/other/path/file.py")
        result = relative_to_repo(outside_path)
        assert result == str(outside_path)


class TestGetPathflowSetting:
    """Tests for get_pathflow_setting function."""

    def test_returns_empty_when_no_settings(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should return empty string when settings.json doesn't exist."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        result = get_pathflow_setting()
        assert result == ""

    def test_reads_pathflow_mode(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should read _codeflow.pathflow_mode from settings.json."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        settings_dir = temp_repo / ".claude"
        settings_dir.mkdir(exist_ok=True)
        settings_file = settings_dir / "settings.json"
        settings_file.write_text(json.dumps({
            "_codeflow": {"pathflow_mode": "always"}
        }))
        result = get_pathflow_setting()
        assert result == "always"

    def test_returns_empty_when_key_missing(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should return empty string when pathflow_mode key is absent."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        settings_dir = temp_repo / ".claude"
        settings_dir.mkdir(exist_ok=True)
        settings_file = settings_dir / "settings.json"
        settings_file.write_text(json.dumps({"other": "value"}))
        result = get_pathflow_setting()
        assert result == ""

    def test_handles_invalid_json(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should return empty string on malformed JSON."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        settings_dir = temp_repo / ".claude"
        settings_dir.mkdir(exist_ok=True)
        settings_file = settings_dir / "settings.json"
        settings_file.write_text("not valid json")
        result = get_pathflow_setting()
        assert result == ""


class TestIsPathflowActive:
    """Tests for is_pathflow_active function."""

    def test_env_override_always(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Env CODEFLOW_PATHFLOW_OVERRIDE=always should return True."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.setenv("CODEFLOW_PATHFLOW_OVERRIDE", "always")
        assert is_pathflow_active() is True

    def test_env_override_never(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Env CODEFLOW_PATHFLOW_OVERRIDE=never should return False."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.setenv("CODEFLOW_PATHFLOW_OVERRIDE", "never")
        assert is_pathflow_active() is False

    def test_settings_always(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Settings pathflow_mode=always should return True."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.delenv("CODEFLOW_PATHFLOW_OVERRIDE", raising=False)
        settings_dir = temp_repo / ".claude"
        settings_dir.mkdir(exist_ok=True)
        (settings_dir / "settings.json").write_text(json.dumps({
            "_codeflow": {"pathflow_mode": "always"}
        }))
        assert is_pathflow_active() is True

    def test_settings_never(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Settings pathflow_mode=never should return False."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.delenv("CODEFLOW_PATHFLOW_OVERRIDE", raising=False)
        settings_dir = temp_repo / ".claude"
        settings_dir.mkdir(exist_ok=True)
        (settings_dir / "settings.json").write_text(json.dumps({
            "_codeflow": {"pathflow_mode": "never"}
        }))
        assert is_pathflow_active() is False

    def test_falls_back_to_flag_file_present(
        self, temp_repo, reset_repo_root_cache, monkeypatch, tmp_path
    ):
        """Should return True when flag file exists and no override."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.delenv("CODEFLOW_PATHFLOW_OVERRIDE", raising=False)
        monkeypatch.setenv("CODEFLOW_SESSION_ID", "test-session")
        # Create session-scoped flag file
        flag_dir = temp_repo / ".state" / "session" / "test-session" / "pathflow"
        flag_dir.mkdir(parents=True, exist_ok=True)
        (flag_dir / "is-pathflow-active").touch()
        assert is_pathflow_active() is True

    def test_falls_back_to_flag_file_absent(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Should return False when no flag file and no override."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.delenv("CODEFLOW_PATHFLOW_OVERRIDE", raising=False)
        # No settings.json, no flag file
        assert is_pathflow_active() is False

    def test_env_override_takes_priority_over_settings(
        self, temp_repo, reset_repo_root_cache, monkeypatch
    ):
        """Env override should take priority over settings.json."""
        monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(temp_repo))
        monkeypatch.setenv("CODEFLOW_PATHFLOW_OVERRIDE", "never")
        settings_dir = temp_repo / ".claude"
        settings_dir.mkdir(exist_ok=True)
        (settings_dir / "settings.json").write_text(json.dumps({
            "_codeflow": {"pathflow_mode": "always"}
        }))
        assert is_pathflow_active() is False


class TestGetRepoRootFallbacks:
    """Tests for get_repo_root fallback paths."""

    def test_git_subprocess_error_falls_through(
        self, reset_repo_root_cache, monkeypatch, tmp_path
    ):
        """Should fall through when git subprocess raises error."""
        monkeypatch.delenv("CODEFLOW_REPO_ROOT", raising=False)
        (tmp_path / ".codeflow").mkdir()

        original_cwd = os.getcwd()
        os.chdir(str(tmp_path))
        try:
            with patch("subprocess.run", side_effect=FileNotFoundError("git not found")):
                result = get_repo_root()
                assert result == tmp_path
        finally:
            os.chdir(original_cwd)

    def test_falls_back_to_cwd_when_no_codeflow_dir(
        self, reset_repo_root_cache, monkeypatch, tmp_path
    ):
        """Should return cwd when no .codeflow dir found anywhere."""
        monkeypatch.delenv("CODEFLOW_REPO_ROOT", raising=False)
        # tmp_path has no .codeflow directory

        original_cwd = os.getcwd()
        os.chdir(str(tmp_path))
        try:
            with patch("subprocess.run") as mock_run:
                mock_run.return_value.returncode = 1
                result = get_repo_root()
                assert result == tmp_path
        finally:
            os.chdir(original_cwd)
