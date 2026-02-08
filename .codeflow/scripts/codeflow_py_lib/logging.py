"""
Structured logging for CodeFlow scripts.

Provides consistent JSON logging format across all Python scripts.
"""

import json
import logging
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Optional


class JSONFormatter(logging.Formatter):
    """Format log records as JSON."""

    def format(self, record: logging.LogRecord) -> str:
        log_entry = {
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "level": record.levelname,
            "logger": record.name,
            "message": record.getMessage(),
        }

        # Add extra fields
        if hasattr(record, "extra"):
            log_entry.update(record.extra)

        # Add exception info
        if record.exc_info:
            log_entry["exception"] = self.formatException(record.exc_info)

        return json.dumps(log_entry)


class CodeFlowLogger(logging.Logger):
    """Extended logger with structured logging support."""

    def __init__(self, name: str, level: int = logging.NOTSET):
        super().__init__(name, level)
        self._extra: Dict[str, Any] = {}

    def with_context(self, **kwargs: Any) -> "CodeFlowLogger":
        """Return logger with additional context fields."""
        self._extra.update(kwargs)
        return self

    def _log(
        self,
        level: int,
        msg: str,
        args: tuple,
        exc_info: Any = None,
        extra: Optional[Dict[str, Any]] = None,
        **kwargs: Any,
    ) -> None:
        if extra is None:
            extra = {}
        extra["extra"] = {**self._extra, **extra.get("extra", {})}
        super()._log(level, msg, args, exc_info, extra, **kwargs)


def setup_logging(
    name: str = "codeflow",
    level: str = "INFO",
    log_format: str = "json",
    log_file: Optional[Path] = None,
) -> CodeFlowLogger:
    """Set up logging with consistent format."""

    # Register custom logger class
    logging.setLoggerClass(CodeFlowLogger)
    logger = logging.getLogger(name)
    logger.setLevel(getattr(logging, level.upper()))

    # Clear existing handlers
    logger.handlers.clear()

    # Create formatter
    if log_format == "json":
        formatter = JSONFormatter()
    else:
        formatter = logging.Formatter(
            "[%(asctime)s] %(levelname)s %(name)s: %(message)s"
        )

    # Console handler
    console_handler = logging.StreamHandler(sys.stderr)
    console_handler.setFormatter(formatter)
    logger.addHandler(console_handler)

    # File handler (optional)
    if log_file:
        log_file.parent.mkdir(parents=True, exist_ok=True)
        file_handler = logging.FileHandler(log_file)
        file_handler.setFormatter(formatter)
        logger.addHandler(file_handler)

    return logger  # type: ignore


# Module-level convenience function
_logger: Optional[CodeFlowLogger] = None


def get_logger(name: str = "codeflow") -> CodeFlowLogger:
    """Get or create logger for the given name."""
    global _logger
    if _logger is None or _logger.name != name:
        _logger = setup_logging(name)
    return _logger
