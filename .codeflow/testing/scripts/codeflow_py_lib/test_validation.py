"""
test_validation.py - Tests for input validation utilities.

Tests validate_pattern, validate_input, validate_schema functions
and PATTERNS/SCHEMAS constants.
"""

import pytest
from codeflow_py_lib import ValidationError
from codeflow_py_lib.validation import (
    PATTERNS,
    SCHEMAS,
    validate_input,
    validate_pattern,
    validate_schema,
)


class TestPatterns:
    """Tests for PATTERNS constant."""

    def test_patterns_is_dict(self):
        """PATTERNS should be a dictionary."""
        assert isinstance(PATTERNS, dict)

    def test_patterns_has_required_keys(self):
        """PATTERNS should have all required pattern keys."""
        required = ["ulid", "epic_id", "task_id", "branch", "domain", "event_type"]
        for key in required:
            assert key in PATTERNS

    def test_ulid_pattern_valid(self):
        """ULID pattern should match valid ULIDs."""
        import re

        pattern = PATTERNS["ulid"]
        # Valid ULID: 26 characters, Crockford base32
        valid_ulid = "01HQXYZ123456789ABCDEFGHJK"
        assert re.match(pattern, valid_ulid)

    def test_ulid_pattern_rejects_invalid(self):
        """ULID pattern should reject invalid values."""
        import re

        pattern = PATTERNS["ulid"]
        invalid_ulids = [
            "01hqxyz123456789abcdefghjk",  # lowercase
            "01HQXYZ123456789ABCDEFGHJI",  # contains I
            "01HQXYZ123456789ABCDEFGHJKL",  # too long
            "01HQXYZ123456789ABCDEFGHJ",  # too short
        ]
        for invalid in invalid_ulids:
            assert not re.match(pattern, invalid)

    def test_epic_id_pattern_valid(self):
        """Epic ID pattern should match valid epic IDs."""
        import re

        pattern = PATTERNS["epic_id"]
        valid_ids = [
            "FRT-EPC-FEAT-AUTH-001",
            "BKD-EPC-FIX-API-023",
            "INF-EPC-CICD-GENL-001",
        ]
        for valid in valid_ids:
            assert re.match(pattern, valid)

    def test_task_id_pattern_valid(self):
        """Task ID pattern should match valid task IDs."""
        import re

        pattern = PATTERNS["task_id"]
        valid_ids = [
            "FRT-TSK-FEAT-AUTH-001",
            "BKD-TSK-FIX-API-023",
        ]
        for valid in valid_ids:
            assert re.match(pattern, valid)

    def test_branch_pattern_valid(self):
        """Branch pattern should match valid branch names."""
        import re

        pattern = PATTERNS["branch"]
        valid_branches = [
            "feat/user-auth",
            "fix/login-bug",
            "refactor/api-cleanup",
            "docs/readme-update",
            "test/unit-tests",
        ]
        for valid in valid_branches:
            assert re.match(pattern, valid)

    def test_branch_pattern_rejects_invalid(self):
        """Branch pattern should reject invalid branch names."""
        import re

        pattern = PATTERNS["branch"]
        invalid_branches = [
            "feature/something",  # wrong prefix
            "feat/UPPERCASE",  # uppercase
            "feat/under_score",  # underscore
            "main",  # no prefix
        ]
        for invalid in invalid_branches:
            assert not re.match(pattern, invalid)


class TestValidatePattern:
    """Tests for validate_pattern function."""

    def test_validate_pattern_with_named_pattern(self):
        """Should validate against named pattern."""
        assert validate_pattern("development", "domain") is True
        assert validate_pattern("invalid", "domain") is False

    def test_validate_pattern_with_custom_pattern(self):
        """Should validate against custom regex."""
        assert validate_pattern("abc123", "custom", r"^[a-z]+\d+$") is True
        assert validate_pattern("ABC123", "custom", r"^[a-z]+\d+$") is False

    def test_validate_pattern_unknown_pattern_raises(self):
        """Should raise ValidationError for unknown pattern name."""
        with pytest.raises(ValidationError) as exc_info:
            validate_pattern("value", "nonexistent")
        assert "Unknown pattern" in str(exc_info.value)


class TestValidateInput:
    """Tests for validate_input function."""

    def test_validate_required_missing(self):
        """Should raise ValidationError for missing required field."""
        with pytest.raises(ValidationError) as exc_info:
            validate_input(None, str, "username", required=True)
        assert "Required field missing" in str(exc_info.value)

    def test_validate_required_none_allowed(self):
        """Should allow None for non-required fields."""
        result = validate_input(None, str, "username", required=False)
        assert result is None

    def test_validate_wrong_type(self):
        """Should raise ValidationError for wrong type."""
        with pytest.raises(ValidationError) as exc_info:
            validate_input(123, str, "username")
        assert "Expected str" in str(exc_info.value)

    def test_validate_correct_type(self):
        """Should return value for correct type."""
        result = validate_input("test", str, "username")
        assert result == "test"

    def test_validate_min_length(self):
        """Should validate minimum string length."""
        with pytest.raises(ValidationError) as exc_info:
            validate_input("ab", str, "username", min_length=3)
        assert "too short" in str(exc_info.value)

    def test_validate_max_length(self):
        """Should validate maximum string length."""
        with pytest.raises(ValidationError) as exc_info:
            validate_input("abcdef", str, "username", max_length=5)
        assert "too long" in str(exc_info.value)

    def test_validate_pattern_match(self):
        """Should validate string against pattern."""
        result = validate_input("development", str, "domain", pattern="domain")
        assert result == "development"

    def test_validate_pattern_no_match(self):
        """Should raise ValidationError when pattern doesn't match."""
        with pytest.raises(ValidationError) as exc_info:
            validate_input("invalid_domain", str, "domain", pattern="domain")
        assert "does not match pattern" in str(exc_info.value)

    def test_validate_allowed_values(self):
        """Should validate against allowed values."""
        result = validate_input("A", str, "choice", allowed_values=["A", "B", "C"])
        assert result == "A"

    def test_validate_not_in_allowed_values(self):
        """Should raise ValidationError when value not in allowed list."""
        with pytest.raises(ValidationError) as exc_info:
            validate_input("D", str, "choice", allowed_values=["A", "B", "C"])
        assert "not in allowed list" in str(exc_info.value)


class TestValidateSchema:
    """Tests for validate_schema function."""

    def test_validate_schema_valid_data(self):
        """Should validate correct data against schema."""
        schema = {
            "name": {"type": str, "required": True},
            "age": {"type": int, "required": False},
        }
        data = {"name": "Alice", "age": 30}
        result = validate_schema(data, schema)
        assert result["name"] == "Alice"
        assert result["age"] == 30

    def test_validate_schema_missing_required(self):
        """Should raise ValidationError for missing required field."""
        schema = {
            "name": {"type": str, "required": True},
        }
        data = {}
        with pytest.raises(ValidationError) as exc_info:
            validate_schema(data, schema)
        assert "Required field missing" in str(exc_info.value)

    def test_validate_schema_optional_missing(self):
        """Should allow missing optional fields."""
        schema = {
            "name": {"type": str, "required": True},
            "age": {"type": int, "required": False},
        }
        data = {"name": "Alice"}
        result = validate_schema(data, schema)
        assert result["name"] == "Alice"
        assert result["age"] is None


class TestSchemas:
    """Tests for SCHEMAS constant."""

    def test_schemas_is_dict(self):
        """SCHEMAS should be a dictionary."""
        assert isinstance(SCHEMAS, dict)

    def test_schemas_has_memory_event(self):
        """SCHEMAS should have memory_event schema."""
        assert "memory_event" in SCHEMAS

    def test_schemas_has_claim(self):
        """SCHEMAS should have claim schema."""
        assert "claim" in SCHEMAS

    def test_memory_event_schema_structure(self):
        """memory_event schema should have required fields."""
        schema = SCHEMAS["memory_event"]
        assert "event_type" in schema
        assert "domain" in schema
        assert "content" in schema
        assert schema["event_type"]["required"] is True

    def test_claim_schema_structure(self):
        """claim schema should have required fields."""
        schema = SCHEMAS["claim"]
        assert "pattern" in schema
        assert "mode" in schema
        assert "owner" in schema
        assert schema["mode"]["allowed_values"] == ["exclusive", "shared"]
