"""
test_logging.py - Tests for structured logging module.

Tests JSONFormatter, CodeFlowLogger, setup_logging, and get_logger functions.
"""

import importlib
import json
import logging as stdlib_logging
import sys
from io import StringIO
from pathlib import Path

import pytest

# Import through the package so coverage tracks the right module object.
# codeflow_py_lib.logging is already loaded by __init__.py; grab a reference.
import codeflow_py_lib.logging as codeflow_logging  # noqa: E402

JSONFormatter = codeflow_logging.JSONFormatter
CodeFlowLogger = codeflow_logging.CodeFlowLogger
setup_logging = codeflow_logging.setup_logging
get_logger = codeflow_logging.get_logger


@pytest.fixture
def reset_logger():
    """Reset logger state between tests."""
    codeflow_logging._logger = None
    yield
    codeflow_logging._logger = None


@pytest.fixture
def capture_stderr(monkeypatch):
    """Capture stderr for testing log output."""
    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    return captured


class TestJSONFormatter:
    """Tests for JSONFormatter class."""

    def test_format_returns_json(self):
        """Format should return valid JSON."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.INFO,
            pathname="test.py",
            lineno=1,
            msg="Test message",
            args=(),
            exc_info=None,
        )
        output = formatter.format(record)
        parsed = json.loads(output)
        assert "timestamp" in parsed
        assert "level" in parsed
        assert "logger" in parsed
        assert "message" in parsed

    def test_format_includes_level(self):
        """Should include log level."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.WARNING,
            pathname="test.py",
            lineno=1,
            msg="Warning message",
            args=(),
            exc_info=None,
        )
        output = formatter.format(record)
        parsed = json.loads(output)
        assert parsed["level"] == "WARNING"

    def test_format_includes_logger_name(self):
        """Should include logger name."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="my.module.name",
            level=stdlib_logging.INFO,
            pathname="test.py",
            lineno=1,
            msg="Test",
            args=(),
            exc_info=None,
        )
        output = formatter.format(record)
        parsed = json.loads(output)
        assert parsed["logger"] == "my.module.name"

    def test_format_includes_message(self):
        """Should include formatted message."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.INFO,
            pathname="test.py",
            lineno=1,
            msg="Hello %s",
            args=("World",),
            exc_info=None,
        )
        output = formatter.format(record)
        parsed = json.loads(output)
        assert parsed["message"] == "Hello World"

    def test_format_includes_extra_fields(self):
        """Should include extra fields."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.INFO,
            pathname="test.py",
            lineno=1,
            msg="Test",
            args=(),
            exc_info=None,
        )
        record.extra = {"task_id": "TSK-001", "user": "alice"}
        output = formatter.format(record)
        parsed = json.loads(output)
        assert parsed["task_id"] == "TSK-001"
        assert parsed["user"] == "alice"

    def test_format_includes_exception(self):
        """Should include exception info."""
        formatter = JSONFormatter()
        try:
            raise ValueError("Test error")
        except ValueError:
            exc_info = sys.exc_info()

        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.ERROR,
            pathname="test.py",
            lineno=1,
            msg="An error occurred",
            args=(),
            exc_info=exc_info,
        )
        output = formatter.format(record)
        parsed = json.loads(output)
        assert "exception" in parsed
        assert "ValueError" in parsed["exception"]


class TestCodeFlowLogger:
    """Tests for CodeFlowLogger class."""

    def test_is_logger_subclass(self):
        """Should be subclass of logging.Logger."""
        assert issubclass(CodeFlowLogger, stdlib_logging.Logger)

    def test_with_context_returns_self(self):
        """with_context should return the logger instance."""
        stdlib_logging.setLoggerClass(CodeFlowLogger)
        logger = stdlib_logging.getLogger("test.context")
        result = logger.with_context(task_id="TSK-001")
        assert result is logger

    def test_with_context_adds_fields(self):
        """with_context should add context fields."""
        stdlib_logging.setLoggerClass(CodeFlowLogger)
        logger = stdlib_logging.getLogger("test.context2")
        logger.with_context(session_id="ses-001")
        assert logger._extra["session_id"] == "ses-001"

    def test_context_persists_across_calls(self):
        """Context should persist across with_context calls."""
        stdlib_logging.setLoggerClass(CodeFlowLogger)
        logger = stdlib_logging.getLogger("test.context3")
        logger.with_context(task_id="TSK-001")
        logger.with_context(user="alice")
        assert logger._extra["task_id"] == "TSK-001"
        assert logger._extra["user"] == "alice"


class TestSetupLogging:
    """Tests for setup_logging function."""

    def test_returns_codeflow_logger(self):
        """Should return CodeFlowLogger instance."""
        logger = setup_logging(name="test.setup1")
        assert isinstance(logger, CodeFlowLogger)

    def test_sets_log_level(self):
        """Should set specified log level."""
        logger = setup_logging(name="test.setup2", level="DEBUG")
        assert logger.level == stdlib_logging.DEBUG

        logger = setup_logging(name="test.setup3", level="WARNING")
        assert logger.level == stdlib_logging.WARNING

    def test_json_format_uses_json_formatter(self):
        """Should use JSONFormatter for json format."""
        logger = setup_logging(name="test.setup4", log_format="json")
        handler = logger.handlers[0]
        assert isinstance(handler.formatter, JSONFormatter)

    def test_text_format_uses_standard_formatter(self):
        """Should use standard Formatter for text format."""
        logger = setup_logging(name="test.setup5", log_format="text")
        handler = logger.handlers[0]
        assert isinstance(handler.formatter, stdlib_logging.Formatter)
        assert not isinstance(handler.formatter, JSONFormatter)

    def test_clears_existing_handlers(self):
        """Should clear existing handlers."""
        logger = setup_logging(name="test.setup6")
        initial_handlers = len(logger.handlers)

        # Call setup again
        logger = setup_logging(name="test.setup6")
        assert len(logger.handlers) == initial_handlers

    def test_adds_file_handler(self, tmp_path):
        """Should add file handler when log_file specified."""
        log_file = tmp_path / "test.log"
        logger = setup_logging(name="test.setup7", log_file=log_file)

        # Should have 2 handlers: console + file
        assert len(logger.handlers) == 2
        assert log_file.parent.exists()

    def test_creates_log_directory(self, tmp_path):
        """Should create log directory if needed."""
        log_file = tmp_path / "logs" / "deep" / "test.log"
        setup_logging(name="test.setup8", log_file=log_file)
        assert log_file.parent.exists()


class TestGetLogger:
    """Tests for get_logger function."""

    def test_returns_logger(self, reset_logger):
        """Should return a logger."""
        logger = get_logger()
        assert logger is not None

    def test_singleton_behavior(self, reset_logger):
        """Should return same logger on repeated calls."""
        logger1 = get_logger()
        logger2 = get_logger()
        assert logger1 is logger2

    def test_default_name(self, reset_logger):
        """Should use codeflow as default name."""
        logger = get_logger()
        assert logger.name == "codeflow"

    def test_custom_name(self, reset_logger):
        """Should accept custom logger name."""
        logger = get_logger(name="custom.logger")
        # First call sets the singleton
        assert logger.name == "custom.logger"

    def test_different_name_replaces_singleton(self, reset_logger):
        """Getting logger with different name replaces the singleton."""
        logger1 = get_logger(name="first")
        logger2 = get_logger(name="second")
        assert logger2.name == "second"
        assert logger1 is not logger2


class TestJSONFormatterExtraFields:
    """Tests for JSONFormatter with extra fields and exceptions."""

    def test_no_extra_field_on_record(self):
        """Should not include extra when record has no extra attribute."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.INFO,
            pathname="test.py",
            lineno=1,
            msg="No extras",
            args=(),
            exc_info=None,
        )
        # Standard LogRecord has no 'extra' attribute
        output = formatter.format(record)
        parsed = json.loads(output)
        assert "task_id" not in parsed

    def test_format_with_no_exception(self):
        """Should not include exception key when exc_info is None."""
        formatter = JSONFormatter()
        record = stdlib_logging.LogRecord(
            name="test",
            level=stdlib_logging.INFO,
            pathname="test.py",
            lineno=1,
            msg="Normal message",
            args=(),
            exc_info=None,
        )
        output = formatter.format(record)
        parsed = json.loads(output)
        assert "exception" not in parsed


class TestCodeFlowLoggerLog:
    """Tests for CodeFlowLogger._log method and context merging."""

    def test_log_merges_context_into_extra(self, tmp_path):
        """_log should merge _extra context into the extra dict."""
        stdlib_logging.setLoggerClass(CodeFlowLogger)
        logger = stdlib_logging.getLogger("test.merge_context")
        logger.setLevel(stdlib_logging.DEBUG)
        logger.handlers.clear()

        formatter = JSONFormatter()
        handler = stdlib_logging.StreamHandler(StringIO())
        handler.setFormatter(formatter)
        logger.addHandler(handler)

        logger.with_context(session_id="ses-123")
        logger.info("Test message")

        output = handler.stream.getvalue()
        parsed = json.loads(output)
        assert parsed["session_id"] == "ses-123"

    def test_log_with_none_extra(self, tmp_path):
        """_log should handle None extra parameter."""
        stdlib_logging.setLoggerClass(CodeFlowLogger)
        logger = stdlib_logging.getLogger("test.none_extra")
        logger.setLevel(stdlib_logging.DEBUG)
        logger.handlers.clear()

        formatter = JSONFormatter()
        handler = stdlib_logging.StreamHandler(StringIO())
        handler.setFormatter(formatter)
        logger.addHandler(handler)

        # This exercises the `if extra is None: extra = {}` branch
        logger.info("Test with no extra")
        output = handler.stream.getvalue()
        assert output  # Some output was produced


class TestSetupLoggingTextFormat:
    """Tests for text format in setup_logging."""

    def test_text_formatter_format_string(self):
        """Text formatter should use expected format pattern."""
        logger = setup_logging(name="test.text_fmt", log_format="text")
        handler = logger.handlers[0]
        assert isinstance(handler.formatter, stdlib_logging.Formatter)
        assert "%(levelname)s" in handler.formatter._fmt

    def test_file_handler_with_text_format(self, tmp_path):
        """File handler should also use text format."""
        log_file = tmp_path / "text.log"
        logger = setup_logging(
            name="test.file_text", log_format="text", log_file=log_file
        )
        assert len(logger.handlers) == 2
        # Both handlers should use text formatter
        for handler in logger.handlers:
            assert isinstance(handler.formatter, stdlib_logging.Formatter)
