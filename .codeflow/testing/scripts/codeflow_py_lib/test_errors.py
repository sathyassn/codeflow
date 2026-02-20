"""
test_errors.py - Tests for custom exception hierarchy.
"""

from codeflow_py_lib import (
    CodeFlowError,
    ConfigError,
    CRDTError,
    ValidationError,
)
from codeflow_py_lib.errors import DatabaseError, JSONLError, ScriptError


class TestCodeFlowError:
    """Tests for base CodeFlowError."""

    def test_basic_creation(self):
        """Should create error with message."""
        error = CodeFlowError("Test error")
        assert str(error) == "Test error"
        assert error.message == "Test error"

    def test_with_code(self):
        """Should accept error code."""
        error = CodeFlowError("Test error", code="TEST_ERROR")
        assert error.code == "TEST_ERROR"

    def test_default_code(self):
        """Should have default code."""
        error = CodeFlowError("Test error")
        assert error.code == "CODEFLOW_ERROR"

    def test_to_dict(self):
        """Should convert to dictionary."""
        error = CodeFlowError("Test error", code="TEST", details={"x": 1})
        result = error.to_dict()
        assert result["error"] == "TEST"
        assert result["message"] == "Test error"
        assert result["details"] == {"x": 1}

    def test_to_dict_empty_details(self):
        """Should convert to dictionary with empty details."""
        error = CodeFlowError("Test error")
        result = error.to_dict()
        assert result["details"] == {}

    def test_inherits_from_exception(self):
        """Should inherit from Exception."""
        error = CodeFlowError("Test error")
        assert isinstance(error, Exception)

    def test_can_be_raised_and_caught(self):
        """Should be raisable and catchable."""
        try:
            raise CodeFlowError("Test error", code="TEST")
        except CodeFlowError as e:
            assert e.message == "Test error"
            assert e.code == "TEST"


class TestConfigError:
    """Tests for ConfigError."""

    def test_is_subclass(self):
        """ConfigError should be subclass of CodeFlowError."""
        assert issubclass(ConfigError, CodeFlowError)

    def test_default_code(self):
        """Should have CONFIG_ERROR code."""
        error = ConfigError("Config issue")
        assert error.code == "CONFIG_ERROR"

    def test_with_details(self):
        """Should accept details parameter."""
        error = ConfigError("Missing config", details={"file": "config.json"})
        assert error.details.get("file") == "config.json"

    def test_to_dict(self):
        """Should convert to dictionary with correct code."""
        error = ConfigError("Bad config", details={"key": "value"})
        result = error.to_dict()
        assert result["error"] == "CONFIG_ERROR"
        assert result["message"] == "Bad config"


class TestValidationError:
    """Tests for ValidationError."""

    def test_is_subclass(self):
        """ValidationError should be subclass of CodeFlowError."""
        assert issubclass(ValidationError, CodeFlowError)

    def test_with_field(self):
        """Should accept field parameter."""
        error = ValidationError("Invalid", field="username")
        assert error.details.get("field") == "username"

    def test_with_value(self):
        """Should accept value parameter and store repr."""
        error = ValidationError("Invalid", field="age", value=-5)
        assert error.details.get("field") == "age"
        assert error.details.get("value") == repr(-5)

    def test_with_string_value(self):
        """Should store string value repr."""
        error = ValidationError("Invalid", value="bad input")
        assert error.details.get("value") == repr("bad input")

    def test_with_none_value(self):
        """Should not include value when None."""
        error = ValidationError("Invalid", field="test", value=None)
        assert "value" not in error.details
        assert error.details.get("field") == "test"

    def test_default_code(self):
        """Should have VALIDATION_ERROR code."""
        error = ValidationError("Invalid input")
        assert error.code == "VALIDATION_ERROR"

    def test_with_additional_details(self):
        """Should merge field with additional details."""
        error = ValidationError("Invalid", field="email", details={"format": "email"})
        assert error.details.get("field") == "email"
        assert error.details.get("format") == "email"


class TestCRDTError:
    """Tests for CRDTError."""

    def test_is_subclass(self):
        """CRDTError should be subclass of CodeFlowError."""
        assert issubclass(CRDTError, CodeFlowError)

    def test_default_code(self):
        """Should have CRDT_ERROR code."""
        error = CRDTError("CRDT issue")
        assert error.code == "CRDT_ERROR"

    def test_with_operation(self):
        """Should accept operation parameter."""
        error = CRDTError("Merge conflict", operation="merge")
        assert error.details.get("operation") == "merge"

    def test_with_operation_and_details(self):
        """Should merge operation with additional details."""
        error = CRDTError("Conflict", operation="apply", details={"node": "A"})
        assert error.details.get("operation") == "apply"
        assert error.details.get("node") == "A"

    def test_without_operation(self):
        """Should work without operation parameter."""
        error = CRDTError("General error")
        assert "operation" not in error.details


class TestDatabaseError:
    """Tests for DatabaseError."""

    def test_is_subclass(self):
        """DatabaseError should be subclass of CodeFlowError."""
        assert issubclass(DatabaseError, CodeFlowError)

    def test_default_code(self):
        """Should have DATABASE_ERROR code."""
        error = DatabaseError("Database issue")
        assert error.code == "DATABASE_ERROR"

    def test_with_operation(self):
        """Should accept operation parameter."""
        error = DatabaseError("Query failed", operation="SELECT")
        assert error.details.get("operation") == "SELECT"

    def test_with_operation_and_details(self):
        """Should merge operation with additional details."""
        error = DatabaseError("Insert failed", operation="INSERT", details={"table": "users"})
        assert error.details.get("operation") == "INSERT"
        assert error.details.get("table") == "users"

    def test_without_operation(self):
        """Should work without operation parameter."""
        error = DatabaseError("Connection lost")
        assert "operation" not in error.details

    def test_to_dict(self):
        """Should convert to dictionary with operation in details."""
        error = DatabaseError("Failed", operation="UPDATE", details={"rows": 0})
        result = error.to_dict()
        assert result["error"] == "DATABASE_ERROR"
        assert result["details"]["operation"] == "UPDATE"
        assert result["details"]["rows"] == 0


class TestJSONLError:
    """Tests for JSONLError."""

    def test_is_subclass(self):
        """JSONLError should be subclass of CodeFlowError."""
        assert issubclass(JSONLError, CodeFlowError)

    def test_default_code(self):
        """Should have JSONL_ERROR code."""
        error = JSONLError("Parse error")
        assert error.code == "JSONL_ERROR"

    def test_with_line_number(self):
        """Should accept line_number parameter."""
        error = JSONLError("Invalid JSON", line_number=42)
        assert error.details.get("line_number") == 42

    def test_with_line_number_zero(self):
        """Should accept line_number of 0."""
        error = JSONLError("Invalid at start", line_number=0)
        assert error.details.get("line_number") == 0

    def test_with_line_number_and_details(self):
        """Should merge line_number with additional details."""
        error = JSONLError("Parse error", line_number=10, details={"file": "test.jsonl"})
        assert error.details.get("line_number") == 10
        assert error.details.get("file") == "test.jsonl"

    def test_without_line_number(self):
        """Should work without line_number parameter."""
        error = JSONLError("General JSONL error")
        assert "line_number" not in error.details

    def test_to_dict(self):
        """Should convert to dictionary with line_number in details."""
        error = JSONLError("Bad line", line_number=5)
        result = error.to_dict()
        assert result["error"] == "JSONL_ERROR"
        assert result["details"]["line_number"] == 5


class TestScriptError:
    """Tests for ScriptError."""

    def test_is_subclass(self):
        """ScriptError should be subclass of CodeFlowError."""
        assert issubclass(ScriptError, CodeFlowError)

    def test_default_code(self):
        """Should have SCRIPT_ERROR code."""
        error = ScriptError("Script failed")
        assert error.code == "SCRIPT_ERROR"

    def test_with_script(self):
        """Should accept script parameter."""
        error = ScriptError("Execution failed", script="build.sh")
        assert error.details.get("script") == "build.sh"

    def test_with_script_and_details(self):
        """Should merge script with additional details."""
        error = ScriptError("Failed", script="deploy.py", details={"exit_code": 1})
        assert error.details.get("script") == "deploy.py"
        assert error.details.get("exit_code") == 1

    def test_without_script(self):
        """Should work without script parameter."""
        error = ScriptError("General script error")
        assert "script" not in error.details

    def test_to_dict(self):
        """Should convert to dictionary with script in details."""
        error = ScriptError("Timeout", script="long_task.sh", details={"timeout": 30})
        result = error.to_dict()
        assert result["error"] == "SCRIPT_ERROR"
        assert result["details"]["script"] == "long_task.sh"
        assert result["details"]["timeout"] == 30


class TestCodeFlowErrorWithDetails:
    """Additional edge case tests for CodeFlowError."""

    def test_with_details(self):
        """Should store details dictionary."""
        details = {"file": "test.py", "line": 42}
        error = CodeFlowError("Test", details=details)
        assert error.details == details

    def test_none_details_becomes_empty(self):
        """None details should become empty dict."""
        error = CodeFlowError("Test", details=None)
        assert error.details == {}

    def test_none_code_becomes_default(self):
        """None code should become default."""
        error = CodeFlowError("Test", code=None)
        assert error.code == "CODEFLOW_ERROR"

    def test_str_representation(self):
        """str() should return the message."""
        error = CodeFlowError("My error message")
        assert str(error) == "My error message"


class TestConfigErrorEdgeCases:
    """Edge case tests for ConfigError."""

    def test_none_details(self):
        """Should handle None details."""
        error = ConfigError("Error", details=None)
        assert error.details == {}

    def test_empty_details(self):
        """Should handle empty details dict."""
        error = ConfigError("Error", details={})
        assert error.details == {}

    def test_inherits_to_dict(self):
        """Should use parent to_dict with CONFIG_ERROR code."""
        error = ConfigError("Bad config")
        result = error.to_dict()
        assert result["error"] == "CONFIG_ERROR"
        assert result["message"] == "Bad config"
        assert result["details"] == {}


class TestValidationErrorEdgeCases:
    """Edge case tests for ValidationError."""

    def test_no_field_no_value(self):
        """Should work with no field and no value."""
        error = ValidationError("Invalid input")
        assert "field" not in error.details
        assert "value" not in error.details

    def test_field_only(self):
        """Should store field without value."""
        error = ValidationError("Invalid", field="name")
        assert error.details["field"] == "name"
        assert "value" not in error.details

    def test_value_zero(self):
        """Should store zero value (not None)."""
        error = ValidationError("Invalid", value=0)
        assert error.details["value"] == repr(0)

    def test_value_empty_string(self):
        """Should store empty string value."""
        error = ValidationError("Invalid", value="")
        assert error.details["value"] == repr("")


class TestDatabaseErrorEdgeCases:
    """Edge case tests for DatabaseError."""

    def test_none_operation(self):
        """Should handle None operation."""
        error = DatabaseError("Error", operation=None)
        assert "operation" not in error.details

    def test_none_details(self):
        """Should handle None details."""
        error = DatabaseError("Error", details=None)
        assert error.details == {}


class TestJSONLErrorEdgeCases:
    """Edge case tests for JSONLError."""

    def test_none_line_number(self):
        """Should handle None line_number."""
        error = JSONLError("Error", line_number=None)
        assert "line_number" not in error.details

    def test_none_details(self):
        """Should handle None details."""
        error = JSONLError("Error", details=None)
        assert error.details == {}


class TestCRDTErrorEdgeCases:
    """Edge case tests for CRDTError."""

    def test_none_operation(self):
        """Should handle None operation."""
        error = CRDTError("Error", operation=None)
        assert "operation" not in error.details

    def test_none_details(self):
        """Should handle None details."""
        error = CRDTError("Error", details=None)
        assert error.details == {}

    def test_to_dict_with_operation(self):
        """Should include operation in to_dict output."""
        error = CRDTError("Conflict", operation="merge")
        result = error.to_dict()
        assert result["error"] == "CRDT_ERROR"
        assert result["details"]["operation"] == "merge"


class TestScriptErrorEdgeCases:
    """Edge case tests for ScriptError."""

    def test_none_script(self):
        """Should handle None script."""
        error = ScriptError("Error", script=None)
        assert "script" not in error.details

    def test_none_details(self):
        """Should handle None details."""
        error = ScriptError("Error", details=None)
        assert error.details == {}
