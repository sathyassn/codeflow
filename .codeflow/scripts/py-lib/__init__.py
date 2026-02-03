"""
codeflow_py_lib - Shared Python library for CodeFlow template scripts.

Usage:
    from codeflow_py_lib import get_logger, CodeFlowError, generate_ulid
"""

from .paths import get_repo_root, get_state_dir, get_config_dir
from .errors import (
    CodeFlowError,
    ConfigError,
    ValidationError,
    DatabaseError,
    JSONLError,
)
from .ulid import generate_ulid, parse_ulid
from .logging import get_logger, setup_logging
from .config import get_config, Config
from .validation import validate_input, validate_schema
from .jsonl import read_jsonl, write_jsonl, append_jsonl

__version__ = "1.0.0"
__all__ = [
    # Paths
    "get_repo_root",
    "get_state_dir",
    "get_config_dir",
    # Errors
    "CodeFlowError",
    "ConfigError",
    "ValidationError",
    "DatabaseError",
    "JSONLError",
    # ULID
    "generate_ulid",
    "parse_ulid",
    # Logging
    "get_logger",
    "setup_logging",
    # Configuration
    "get_config",
    "Config",
    # Validation
    "validate_input",
    "validate_schema",
    # JSONL
    "read_jsonl",
    "write_jsonl",
    "append_jsonl",
]
