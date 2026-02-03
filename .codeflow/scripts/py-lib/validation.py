"""
Input validation utilities for CodeFlow scripts.

Provides schema validation and input sanitization.
"""

import re
from typing import Any, Dict, List, Optional, Type

from .errors import ValidationError


# Common patterns
PATTERNS = {
    "ulid": r"^[0-9A-HJKMNP-TV-Z]{26}$",
    "epic_id": r"^[A-Z]{3}-EPC-[A-Z]+-[A-Z]+-\d{3}$",
    "task_id": r"^[A-Z]{3}-TSK-[A-Z]+-[A-Z]+-\d{3}$",
    "branch": r"^(feat|fix|refactor|docs|test|ops|plan|chore)/[a-z0-9-]+$",
    "domain": r"^(planning|development|review|qa|ops|documentation|sessions)$",
    "event_type": r"^(progress|decision|milestone|issue|question|context|summary)$",
}


def validate_pattern(
    value: str, pattern_name: str, custom_pattern: Optional[str] = None
) -> bool:
    """Validate string against named pattern or custom regex."""
    pattern = custom_pattern or PATTERNS.get(pattern_name)
    if not pattern:
        raise ValidationError(f"Unknown pattern: {pattern_name}")
    return bool(re.match(pattern, value))


def validate_input(
    value: Any,
    expected_type: Type,
    field_name: str,
    required: bool = True,
    min_length: Optional[int] = None,
    max_length: Optional[int] = None,
    pattern: Optional[str] = None,
    allowed_values: Optional[List[Any]] = None,
) -> Any:
    """Validate a single input value."""

    # Check required
    if value is None:
        if required:
            raise ValidationError("Required field missing", field=field_name)
        return None

    # Check type
    if not isinstance(value, expected_type):
        raise ValidationError(
            f"Expected {expected_type.__name__}, got {type(value).__name__}",
            field=field_name,
            value=value,
        )

    # String validations
    if isinstance(value, str):
        if min_length and len(value) < min_length:
            raise ValidationError(
                f"Value too short (min {min_length})",
                field=field_name,
                value=value,
            )
        if max_length and len(value) > max_length:
            raise ValidationError(
                f"Value too long (max {max_length})",
                field=field_name,
                value=value,
            )
        if pattern and not validate_pattern(value, pattern):
            raise ValidationError(
                f"Value does not match pattern '{pattern}'",
                field=field_name,
                value=value,
            )

    # Allowed values
    if allowed_values and value not in allowed_values:
        raise ValidationError(
            f"Value not in allowed list: {allowed_values}",
            field=field_name,
            value=value,
        )

    return value


def validate_schema(data: Dict[str, Any], schema: Dict[str, Any]) -> Dict[str, Any]:
    """Validate dictionary against schema definition."""
    validated = {}

    for field_name, field_schema in schema.items():
        value = data.get(field_name)
        validated[field_name] = validate_input(
            value=value,
            expected_type=field_schema.get("type", str),
            field_name=field_name,
            required=field_schema.get("required", False),
            min_length=field_schema.get("min_length"),
            max_length=field_schema.get("max_length"),
            pattern=field_schema.get("pattern"),
            allowed_values=field_schema.get("allowed_values"),
        )

    return validated


# Common schemas
SCHEMAS = {
    "memory_event": {
        "event_type": {"type": str, "required": True, "pattern": "event_type"},
        "domain": {"type": str, "required": True, "pattern": "domain"},
        "content": {"type": str, "required": True, "min_length": 1},
        "work_id": {"type": str, "required": False},
        "metadata": {"type": dict, "required": False},
    },
    "claim": {
        "pattern": {"type": str, "required": True, "min_length": 1},
        "mode": {
            "type": str,
            "required": True,
            "allowed_values": ["exclusive", "shared"],
        },
        "owner": {"type": str, "required": True},
        "ttl_seconds": {"type": int, "required": False},
    },
}
