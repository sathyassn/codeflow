"""
codeflow_py_lib - Shared Python library for CodeFlow template scripts.

Usage:
    from codeflow_py_lib import get_logger, CodeFlowError, generate_ulid
"""

from .chunking import (
    DEFAULT_CHUNK_SIZE,
    DEFAULT_OVERLAP,
    TextChunk,
    chunk_for_embedding,
    chunk_text,
    estimate_tokens,
    split_into_paragraphs,
    split_into_sentences,
)
from .config import Config, get_config
from .crdt import (
    LORO_AVAILABLE,
    CoordinationDoc,
    load_coordination,
    rebuild_from_jsonl,
    save_coordination,
)
from .db import DatabaseConfig, DatabaseOperations, dict_factory, get_db
from .embeddings import (
    DEFAULT_MODEL,
    EMBEDDING_DIM,
    SENTENCE_TRANSFORMERS_AVAILABLE,
    blob_to_embedding,
    cosine_similarity,
    embedding_to_blob,
    find_most_similar,
    generate_embedding,
    generate_embeddings,
    get_model,
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
from .paths import get_config_dir, get_repo_root, get_state_dir
from .ulid import generate_ulid, parse_ulid
from .validation import validate_input, validate_schema

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
    # Database
    "DatabaseOperations",
    "DatabaseConfig",
    "get_db",
    "dict_factory",
    # CRDT
    "CoordinationDoc",
    "load_coordination",
    "save_coordination",
    "rebuild_from_jsonl",
    "LORO_AVAILABLE",
    # Embeddings
    "generate_embedding",
    "generate_embeddings",
    "embedding_to_blob",
    "blob_to_embedding",
    "cosine_similarity",
    "find_most_similar",
    "get_model",
    "DEFAULT_MODEL",
    "EMBEDDING_DIM",
    "SENTENCE_TRANSFORMERS_AVAILABLE",
    # Chunking
    "TextChunk",
    "chunk_text",
    "chunk_for_embedding",
    "split_into_sentences",
    "split_into_paragraphs",
    "estimate_tokens",
    "DEFAULT_CHUNK_SIZE",
    "DEFAULT_OVERLAP",
]
