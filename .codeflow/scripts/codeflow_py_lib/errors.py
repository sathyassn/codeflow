"""
Custom exceptions for CodeFlow scripts.

Provides a consistent exception hierarchy for error handling.
"""

from typing import Any, Dict, Optional


class CodeFlowError(Exception):
    """Base exception for all CodeFlow errors."""

    def __init__(
        self,
        message: str,
        code: Optional[str] = None,
        details: Optional[Dict[str, Any]] = None,
    ):
        super().__init__(message)
        self.message = message
        self.code = code or "CODEFLOW_ERROR"
        self.details = details or {}

    def to_dict(self) -> Dict[str, Any]:
        """Convert exception to dictionary for logging/serialization."""
        return {
            "error": self.code,
            "message": self.message,
            "details": self.details,
        }


class ConfigError(CodeFlowError):
    """Configuration-related errors."""

    def __init__(self, message: str, details: Optional[Dict[str, Any]] = None):
        super().__init__(message, "CONFIG_ERROR", details)


class ValidationError(CodeFlowError):
    """Input validation errors."""

    def __init__(
        self,
        message: str,
        field: Optional[str] = None,
        value: Any = None,
        details: Optional[Dict[str, Any]] = None,
    ):
        details = details or {}
        if field:
            details["field"] = field
        if value is not None:
            details["value"] = repr(value)
        super().__init__(message, "VALIDATION_ERROR", details)


class DatabaseError(CodeFlowError):
    """Database operation errors."""

    def __init__(
        self,
        message: str,
        operation: Optional[str] = None,
        details: Optional[Dict[str, Any]] = None,
    ):
        details = details or {}
        if operation:
            details["operation"] = operation
        super().__init__(message, "DATABASE_ERROR", details)


class JSONLError(CodeFlowError):
    """JSONL parsing/writing errors."""

    def __init__(
        self,
        message: str,
        line_number: Optional[int] = None,
        details: Optional[Dict[str, Any]] = None,
    ):
        details = details or {}
        if line_number is not None:
            details["line_number"] = line_number
        super().__init__(message, "JSONL_ERROR", details)


class ScriptError(CodeFlowError):
    """Memory/coordination script errors."""

    def __init__(
        self,
        message: str,
        script: Optional[str] = None,
        details: Optional[Dict[str, Any]] = None,
    ):
        details = details or {}
        if script:
            details["script"] = script
        super().__init__(message, "SCRIPT_ERROR", details)


class CRDTError(CodeFlowError):
    """CRDT operation errors."""

    def __init__(
        self,
        message: str,
        operation: Optional[str] = None,
        details: Optional[Dict[str, Any]] = None,
    ):
        details = details or {}
        if operation:
            details["operation"] = operation
        super().__init__(message, "CRDT_ERROR", details)
