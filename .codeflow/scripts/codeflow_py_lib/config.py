"""
Configuration loading and management for CodeFlow scripts.

Handles loading from:
- .codeflow/config/*.yaml
- .codeflow/config/*.json
- Environment variables (CODEFLOW_* prefix)
"""

import json
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional, Tuple, Union

from .errors import ConfigError
from .paths import get_config_dir, get_repo_root


@dataclass
class Config:
    """Configuration container with defaults."""

    # Database
    db_path: str = field(
        default_factory=lambda: str(get_repo_root() / ".state/db/codeflow.db")
    )
    db_timeout: float = 5.0
    db_retries: int = 3

    # Logging
    log_level: str = "INFO"
    log_format: str = "json"

    # Retention (days)
    session_retention: int = 30
    security_retention: int = 30
    network_retention: int = 30
    db_ops_retention: int = 30

    # ULID
    ulid_random_bits: int = 80

    # Paths (computed)
    repo_root: Path = field(default_factory=get_repo_root)
    state_dir: Path = field(default_factory=lambda: get_repo_root() / ".state")
    config_dir: Path = field(default_factory=get_config_dir)

    @classmethod
    def from_file(cls, path: Path) -> "Config":
        """Load configuration from file."""
        if not path.exists():
            raise ConfigError(f"Configuration file not found: {path}")

        if path.suffix in (".yaml", ".yml"):
            try:
                import yaml

                with open(path) as f:
                    data = yaml.safe_load(f)
            except ImportError:
                raise ConfigError("PyYAML not installed, cannot load YAML config")
        elif path.suffix == ".json":
            with open(path) as f:
                data = json.load(f)
        else:
            raise ConfigError(f"Unsupported config format: {path.suffix}")

        return cls(**data) if data else cls()

    @classmethod
    def from_env(cls, prefix: str = "CODEFLOW_") -> "Config":
        """Load configuration from environment variables."""
        env_map: Dict[str, Union[str, Tuple[str, type]]] = {
            f"{prefix}DB_PATH": "db_path",
            f"{prefix}DB_TIMEOUT": ("db_timeout", float),
            f"{prefix}DB_RETRIES": ("db_retries", int),
            f"{prefix}LOG_LEVEL": "log_level",
            f"{prefix}LOG_FORMAT": "log_format",
        }

        kwargs: Dict[str, Any] = {}
        for env_key, config_key in env_map.items():
            value = os.environ.get(env_key)
            if value:
                if isinstance(config_key, tuple):
                    key, converter = config_key
                    kwargs[key] = converter(value)
                else:
                    kwargs[config_key] = value

        return cls(**kwargs)


# Singleton instance
_config: Optional[Config] = None


def get_config(reload: bool = False) -> Config:
    """Get or create singleton Config instance."""
    global _config
    if _config is None or reload:
        # Try loading from file, fall back to env, then defaults
        config_file = get_config_dir() / "codeflow.yaml"
        if config_file.exists():
            _config = Config.from_file(config_file)
        else:
            config_file = get_config_dir() / "codeflow.json"
            if config_file.exists():
                _config = Config.from_file(config_file)
            else:
                _config = Config.from_env()
    return _config
