"""
codeflow_py_lib - Shared Python library for CodeFlow template scripts.

Usage:
    from codeflow_py_lib import get_logger, CodeFlowError, generate_ulid
"""

from .config import Config, get_config
from .crdt import (
    LORO_AVAILABLE,
    CoordinationDoc,
    load_coordination,
    rebuild_from_jsonl,
    save_coordination,
)
from .errors import (
    CodeFlowError,
    ConfigError,
    CRDTError,
    DatabaseError,
    JSONLError,
    ScriptError,
    ValidationError,
)
from .jsonl import append_jsonl, read_jsonl, write_jsonl
from .logging import get_logger, setup_logging
from .paths import get_config_dir, get_pathflow_setting, get_repo_root, get_state_dir, is_pathflow_active
from .ulid import generate_ulid, parse_ulid
from .validation import validate_input, validate_schema

__version__ = "1.0.0"
__all__ = [
    # Paths
    "get_repo_root",
    "get_state_dir",
    "get_config_dir",
    "get_pathflow_setting",
    "is_pathflow_active",
    # Errors
    "CodeFlowError",
    "ConfigError",
    "ValidationError",
    "DatabaseError",
    "JSONLError",
    "CRDTError",
    "ScriptError",
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
    # CRDT
    "CoordinationDoc",
    "load_coordination",
    "save_coordination",
    "rebuild_from_jsonl",
    "LORO_AVAILABLE",
]
