"""
test_config.py - Tests for configuration module.

Tests Config dataclass and from_file, from_env, get_config functions.
"""

import json
import os

import pytest
from codeflow_py_lib import ConfigError
from codeflow_py_lib.config import Config, get_config


@pytest.fixture
def temp_config_dir(tmp_path, monkeypatch):
    """Create temporary config directory structure."""
    # Create repo structure
    (tmp_path / ".codeflow" / "config").mkdir(parents=True)
    (tmp_path / ".state" / "db").mkdir(parents=True)
    monkeypatch.setenv("CODEFLOW_REPO_ROOT", str(tmp_path))

    # Clear paths cache
    import codeflow_py_lib.paths as paths_module

    paths_module.get_repo_root.cache_clear()

    yield tmp_path

    paths_module.get_repo_root.cache_clear()


@pytest.fixture
def reset_config():
    """Reset config singleton between tests."""
    import codeflow_py_lib.config as config_module

    config_module._config = None
    yield
    config_module._config = None


class TestConfigDefaults:
    """Tests for Config default values."""

    def test_default_db_timeout(self):
        """Should have default db timeout."""
        cfg = Config()
        assert cfg.db_timeout == 5.0

    def test_default_db_retries(self):
        """Should have default db retries."""
        cfg = Config()
        assert cfg.db_retries == 3

    def test_default_log_level(self):
        """Should have default log level."""
        cfg = Config()
        assert cfg.log_level == "INFO"

    def test_default_log_format(self):
        """Should have default log format."""
        cfg = Config()
        assert cfg.log_format == "json"

    def test_default_session_retention(self):
        """Should have default session retention."""
        cfg = Config()
        assert cfg.session_retention == 30

    def test_default_ulid_random_bits(self):
        """Should have default ULID random bits."""
        cfg = Config()
        assert cfg.ulid_random_bits == 80


class TestConfigFromFile:
    """Tests for Config.from_file method."""

    def test_load_from_json(self, temp_config_dir):
        """Should load config from JSON file."""
        config_file = temp_config_dir / ".codeflow" / "config" / "test.json"
        config_data = {
            "db_timeout": 10.0,
            "db_retries": 5,
            "log_level": "DEBUG",
        }
        config_file.write_text(json.dumps(config_data))

        cfg = Config.from_file(config_file)
        assert cfg.db_timeout == 10.0
        assert cfg.db_retries == 5
        assert cfg.log_level == "DEBUG"

    def test_load_from_yaml(self, temp_config_dir):
        """Should load config from YAML file if PyYAML is available."""
        pytest.importorskip("yaml")

        import yaml

        config_file = temp_config_dir / ".codeflow" / "config" / "test.yaml"
        config_data = {
            "db_timeout": 15.0,
            "log_level": "WARNING",
        }
        config_file.write_text(yaml.dump(config_data))

        cfg = Config.from_file(config_file)
        assert cfg.db_timeout == 15.0
        assert cfg.log_level == "WARNING"

    def test_file_not_found(self, temp_config_dir):
        """Should raise ConfigError for missing file."""
        missing_file = temp_config_dir / "nonexistent.json"
        with pytest.raises(ConfigError) as exc_info:
            Config.from_file(missing_file)
        assert "not found" in str(exc_info.value)

    def test_unsupported_format(self, temp_config_dir):
        """Should raise ConfigError for unsupported format."""
        bad_file = temp_config_dir / "config.txt"
        bad_file.write_text("key=value")
        with pytest.raises(ConfigError) as exc_info:
            Config.from_file(bad_file)
        assert "Unsupported config format" in str(exc_info.value)


class TestConfigFromEnv:
    """Tests for Config.from_env method."""

    def test_loads_db_path(self, monkeypatch):
        """Should load DB path from environment."""
        monkeypatch.setenv("CODEFLOW_DB_PATH", "/custom/path/db.sqlite")
        cfg = Config.from_env()
        assert cfg.db_path == "/custom/path/db.sqlite"

    def test_loads_db_timeout(self, monkeypatch):
        """Should load DB timeout from environment."""
        monkeypatch.setenv("CODEFLOW_DB_TIMEOUT", "15.5")
        cfg = Config.from_env()
        assert cfg.db_timeout == 15.5

    def test_loads_db_retries(self, monkeypatch):
        """Should load DB retries from environment."""
        monkeypatch.setenv("CODEFLOW_DB_RETRIES", "10")
        cfg = Config.from_env()
        assert cfg.db_retries == 10

    def test_loads_log_level(self, monkeypatch):
        """Should load log level from environment."""
        monkeypatch.setenv("CODEFLOW_LOG_LEVEL", "DEBUG")
        cfg = Config.from_env()
        assert cfg.log_level == "DEBUG"

    def test_loads_log_format(self, monkeypatch):
        """Should load log format from environment."""
        monkeypatch.setenv("CODEFLOW_LOG_FORMAT", "text")
        cfg = Config.from_env()
        assert cfg.log_format == "text"

    def test_missing_env_uses_defaults(self, monkeypatch):
        """Should use defaults for missing env vars."""
        # Clear any existing CODEFLOW_ env vars
        for key in list(os.environ.keys()):
            if key.startswith("CODEFLOW_"):
                monkeypatch.delenv(key, raising=False)

        cfg = Config.from_env()
        assert cfg.db_timeout == 5.0
        assert cfg.db_retries == 3


class TestGetConfig:
    """Tests for get_config function."""

    def test_returns_config_instance(self, reset_config, temp_config_dir):
        """Should return Config instance."""
        cfg = get_config()
        assert isinstance(cfg, Config)

    def test_singleton_behavior(self, reset_config, temp_config_dir):
        """Should return same instance on repeated calls."""
        cfg1 = get_config()
        cfg2 = get_config()
        assert cfg1 is cfg2

    def test_reload_creates_new_instance(self, reset_config, temp_config_dir):
        """Should create new instance when reload=True."""
        cfg1 = get_config()
        cfg2 = get_config(reload=True)
        # New instance created
        assert cfg1 is not cfg2

    def test_loads_from_yaml_file(self, reset_config, temp_config_dir):
        """Should load from codeflow.yaml if exists."""
        pytest.importorskip("yaml")

        import yaml

        config_file = temp_config_dir / ".codeflow" / "config" / "codeflow.yaml"
        config_data = {"log_level": "ERROR"}
        config_file.write_text(yaml.dump(config_data))

        cfg = get_config()
        assert cfg.log_level == "ERROR"

    def test_loads_from_json_file(self, reset_config, temp_config_dir):
        """Should load from codeflow.json if yaml doesn't exist."""
        config_file = temp_config_dir / ".codeflow" / "config" / "codeflow.json"
        config_data = {"log_level": "WARNING"}
        config_file.write_text(json.dumps(config_data))

        cfg = get_config()
        assert cfg.log_level == "WARNING"

    def test_falls_back_to_env(self, reset_config, temp_config_dir, monkeypatch):
        """Should fall back to environment if no config file."""
        monkeypatch.setenv("CODEFLOW_LOG_LEVEL", "DEBUG")
        cfg = get_config()
        assert cfg.log_level == "DEBUG"
