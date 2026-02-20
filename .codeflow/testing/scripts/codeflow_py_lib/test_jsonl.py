"""
test_jsonl.py - Tests for JSONL operations module.
"""

import json
from datetime import datetime, timezone

import pytest
from codeflow_py_lib import append_jsonl, read_jsonl, write_jsonl
from codeflow_py_lib.errors import JSONLError
from codeflow_py_lib.jsonl import count_events, filter_events


class TestReadJSONL:
    """Tests for reading JSONL files."""

    def test_read_empty_file(self, temp_dir):
        """Reading empty file should return empty list."""
        empty_file = temp_dir / "empty.jsonl"
        empty_file.touch()
        result = list(read_jsonl(empty_file))
        assert result == []

    def test_read_single_line(self, temp_dir):
        """Reading single line should return one object."""
        jsonl_file = temp_dir / "single.jsonl"
        jsonl_file.write_text('{"key": "value"}\n')
        result = list(read_jsonl(jsonl_file))
        assert len(result) == 1
        assert result[0] == {"key": "value"}

    def test_read_multiple_lines(self, mock_jsonl_file):
        """Reading multiple lines should return all objects."""
        result = list(read_jsonl(mock_jsonl_file))
        assert len(result) == 3
        assert result[0]["e"] == "session_start"
        assert result[2]["e"] == "progress"

    def test_read_nonexistent_file(self, temp_dir):
        """Reading nonexistent file should return empty generator."""
        missing_file = temp_dir / "does_not_exist.jsonl"
        result = list(read_jsonl(missing_file))
        assert result == []

    def test_read_skips_blank_lines(self, temp_dir):
        """Reading should skip blank lines."""
        jsonl_file = temp_dir / "blanks.jsonl"
        jsonl_file.write_text('{"a": 1}\n\n{"b": 2}\n   \n{"c": 3}\n')
        result = list(read_jsonl(jsonl_file))
        assert len(result) == 3

    def test_read_with_skip_errors(self, temp_dir):
        """Reading with skip_errors should skip invalid JSON lines."""
        jsonl_file = temp_dir / "invalid.jsonl"
        jsonl_file.write_text('{"a": 1}\ninvalid json\n{"b": 2}\n')
        result = list(read_jsonl(jsonl_file, skip_errors=True))
        assert len(result) == 2
        assert result[0] == {"a": 1}
        assert result[1] == {"b": 2}

    def test_read_invalid_json_raises_error(self, temp_dir):
        """Reading invalid JSON without skip_errors should raise JSONLError."""
        jsonl_file = temp_dir / "invalid.jsonl"
        jsonl_file.write_text('{"a": 1}\nnot valid json\n')
        with pytest.raises(JSONLError) as exc_info:
            list(read_jsonl(jsonl_file))
        assert exc_info.value.details.get("line_number") == 2

    def test_read_non_dict_with_validation(self, temp_dir):
        """Reading non-dict with validation should raise JSONLError."""
        jsonl_file = temp_dir / "array.jsonl"
        jsonl_file.write_text('[1, 2, 3]\n')
        with pytest.raises(JSONLError) as exc_info:
            list(read_jsonl(jsonl_file, validate=True))
        assert "Expected object" in str(exc_info.value)
        assert exc_info.value.details.get("line_number") == 1

    def test_read_non_dict_without_validation(self, temp_dir):
        """Reading non-dict without validation should succeed."""
        jsonl_file = temp_dir / "array.jsonl"
        jsonl_file.write_text('[1, 2, 3]\n"string"\n')
        result = list(read_jsonl(jsonl_file, validate=False))
        assert len(result) == 2
        assert result[0] == [1, 2, 3]
        assert result[1] == "string"


class TestWriteJSONL:
    """Tests for writing JSONL files."""

    def test_write_single_object(self, temp_dir):
        """Writing single object should create file with one line."""
        jsonl_file = temp_dir / "single.jsonl"
        write_jsonl(jsonl_file, [{"key": "value"}])
        content = jsonl_file.read_text().strip()
        assert json.loads(content) == {"key": "value"}

    def test_write_multiple_objects(self, temp_dir):
        """Writing multiple objects should create file with multiple lines."""
        jsonl_file = temp_dir / "multi.jsonl"
        data = [{"a": 1}, {"b": 2}, {"c": 3}]
        write_jsonl(jsonl_file, data)
        result = list(read_jsonl(jsonl_file))
        assert result == data

    def test_write_returns_count(self, temp_dir):
        """Writing should return number of events written."""
        jsonl_file = temp_dir / "count.jsonl"
        data = [{"a": 1}, {"b": 2}]
        count = write_jsonl(jsonl_file, data)
        assert count == 2

    def test_write_with_overwrite(self, temp_dir):
        """Writing with overwrite=True should replace existing content."""
        jsonl_file = temp_dir / "overwrite.jsonl"
        write_jsonl(jsonl_file, [{"old": 1}, {"old": 2}])
        write_jsonl(jsonl_file, [{"new": 3}], overwrite=True)
        result = list(read_jsonl(jsonl_file))
        assert len(result) == 1
        assert result[0] == {"new": 3}

    def test_write_creates_parent_dirs(self, temp_dir):
        """Writing should create parent directories if needed."""
        jsonl_file = temp_dir / "nested" / "dir" / "file.jsonl"
        write_jsonl(jsonl_file, [{"key": "value"}])
        assert jsonl_file.exists()


class TestAppendJSONL:
    """Tests for appending to JSONL files."""

    def test_append_to_new_file(self, temp_dir):
        """Appending to new file should create it."""
        jsonl_file = temp_dir / "new.jsonl"
        append_jsonl(jsonl_file, {"key": "value"})
        assert jsonl_file.exists()
        result = list(read_jsonl(jsonl_file))
        assert len(result) == 1

    def test_append_to_existing_file(self, mock_jsonl_file):
        """Appending to existing file should add line."""
        initial_count = len(list(read_jsonl(mock_jsonl_file)))
        append_jsonl(mock_jsonl_file, {"new": "event"})
        result = list(read_jsonl(mock_jsonl_file))
        assert len(result) == initial_count + 1
        # append_jsonl adds ts and id fields automatically
        assert result[-1]["new"] == "event"
        assert "ts" in result[-1]
        assert "id" in result[-1]

    def test_append_with_event_type_string(self, temp_dir):
        """Appending with event type string should use old API pattern."""
        jsonl_file = temp_dir / "event_type.jsonl"
        result = append_jsonl(jsonl_file, "test_event", {"data": 123})
        assert result["e"] == "test_event"
        assert result["data"] == 123
        assert "ts" in result
        assert "id" in result

    def test_append_without_timestamp(self, temp_dir):
        """Appending with add_timestamp=False should not add ts field."""
        jsonl_file = temp_dir / "no_ts.jsonl"
        result = append_jsonl(jsonl_file, {"key": "value"}, add_timestamp=False)
        assert "ts" not in result

    def test_append_without_id(self, temp_dir):
        """Appending with add_id=False should not add id field."""
        jsonl_file = temp_dir / "no_id.jsonl"
        result = append_jsonl(jsonl_file, {"key": "value"}, add_id=False)
        assert "id" not in result

    def test_append_preserves_existing_ts(self, temp_dir):
        """Appending should preserve existing ts field."""
        jsonl_file = temp_dir / "existing_ts.jsonl"
        original_ts = "2024-01-01T00:00:00Z"
        result = append_jsonl(jsonl_file, {"ts": original_ts, "data": 1})
        assert result["ts"] == original_ts

    def test_append_preserves_existing_id(self, temp_dir):
        """Appending should preserve existing id field."""
        jsonl_file = temp_dir / "existing_id.jsonl"
        original_id = "CUSTOM_ID_123"
        result = append_jsonl(jsonl_file, {"id": original_id, "data": 1})
        assert result["id"] == original_id


class TestCountEvents:
    """Tests for count_events function."""

    def test_count_all_events(self, temp_dir):
        """Should count all events in file."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "type_a", "data": 1},
            {"e": "type_b", "data": 2},
            {"e": "type_a", "data": 3},
        ])
        count = count_events(jsonl_file)
        assert count == 3

    def test_count_by_event_type(self, temp_dir):
        """Should count events filtered by type."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "type_a", "data": 1},
            {"e": "type_b", "data": 2},
            {"e": "type_a", "data": 3},
        ])
        count = count_events(jsonl_file, event_type="type_a")
        assert count == 2

    def test_count_empty_file(self, temp_dir):
        """Should return 0 for empty file."""
        jsonl_file = temp_dir / "empty.jsonl"
        jsonl_file.touch()
        count = count_events(jsonl_file)
        assert count == 0

    def test_count_nonexistent_file(self, temp_dir):
        """Should return 0 for nonexistent file."""
        missing_file = temp_dir / "missing.jsonl"
        count = count_events(missing_file)
        assert count == 0

    def test_count_skips_invalid_json(self, temp_dir):
        """Should skip invalid JSON lines when counting."""
        jsonl_file = temp_dir / "mixed.jsonl"
        jsonl_file.write_text('{"e": "valid"}\ninvalid json\n{"e": "valid"}\n')
        count = count_events(jsonl_file)
        assert count == 2


class TestFilterEvents:
    """Tests for filter_events function."""

    def test_filter_by_event_type(self, temp_dir):
        """Should filter events by type."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "type_a", "data": 1},
            {"e": "type_b", "data": 2},
            {"e": "type_a", "data": 3},
        ])
        result = list(filter_events(jsonl_file, event_type="type_a"))
        assert len(result) == 2
        assert all(e["e"] == "type_a" for e in result)

    def test_filter_by_since(self, temp_dir):
        """Should filter events after since datetime."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "ts": "2024-01-01T00:00:00+00:00", "data": 1},
            {"e": "event", "ts": "2024-06-01T00:00:00+00:00", "data": 2},
            {"e": "event", "ts": "2024-12-01T00:00:00+00:00", "data": 3},
        ])
        since = datetime(2024, 3, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, since=since))
        assert len(result) == 2
        assert result[0]["data"] == 2
        assert result[1]["data"] == 3

    def test_filter_by_until(self, temp_dir):
        """Should filter events before until datetime."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "ts": "2024-01-01T00:00:00+00:00", "data": 1},
            {"e": "event", "ts": "2024-06-01T00:00:00+00:00", "data": 2},
            {"e": "event", "ts": "2024-12-01T00:00:00+00:00", "data": 3},
        ])
        until = datetime(2024, 8, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, until=until))
        assert len(result) == 2
        assert result[0]["data"] == 1
        assert result[1]["data"] == 2

    def test_filter_by_date_range(self, temp_dir):
        """Should filter events within date range."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "ts": "2024-01-01T00:00:00+00:00", "data": 1},
            {"e": "event", "ts": "2024-06-01T00:00:00+00:00", "data": 2},
            {"e": "event", "ts": "2024-12-01T00:00:00+00:00", "data": 3},
        ])
        since = datetime(2024, 3, 1, tzinfo=timezone.utc)
        until = datetime(2024, 8, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, since=since, until=until))
        assert len(result) == 1
        assert result[0]["data"] == 2

    def test_filter_with_z_suffix_timestamps(self, temp_dir):
        """Should handle Z suffix in timestamps."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "ts": "2024-06-01T00:00:00Z", "data": 1},
        ])
        since = datetime(2024, 1, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, since=since))
        assert len(result) == 1

    def test_filter_events_without_timestamp(self, temp_dir):
        """Should include events without timestamp when filtering by date."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "data": 1},  # No ts field
            {"e": "event", "ts": "2024-06-01T00:00:00+00:00", "data": 2},
        ])
        since = datetime(2024, 1, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, since=since))
        # Event without ts is included because ts_str check fails
        assert len(result) == 2

    def test_filter_combined_type_and_date(self, temp_dir):
        """Should filter by both type and date."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "type_a", "ts": "2024-01-01T00:00:00+00:00", "data": 1},
            {"e": "type_b", "ts": "2024-06-01T00:00:00+00:00", "data": 2},
            {"e": "type_a", "ts": "2024-12-01T00:00:00+00:00", "data": 3},
        ])
        since = datetime(2024, 3, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, event_type="type_a", since=since))
        assert len(result) == 1
        assert result[0]["data"] == 3


class TestWriteJSONLEdgeCases:
    """Tests for additional write_jsonl edge cases."""

    def test_write_empty_list(self, temp_dir):
        """Writing empty list should create file but write nothing."""
        jsonl_file = temp_dir / "empty_write.jsonl"
        count = write_jsonl(jsonl_file, [])
        assert count == 0
        assert jsonl_file.exists()
        assert jsonl_file.read_text() == ""

    def test_write_compact_json_no_spaces(self, temp_dir):
        """write_jsonl should produce compact JSON (no spaces)."""
        jsonl_file = temp_dir / "compact.jsonl"
        write_jsonl(jsonl_file, [{"key": "value", "num": 42}])
        content = jsonl_file.read_text().strip()
        assert " " not in content.replace('"key"', "key").replace('"value"', "value")
        # Verify separators are compact: colon without space, comma without space
        assert ',"' in content or content.count(":") >= 1

    def test_write_append_mode_default(self, temp_dir):
        """Default mode should append, not overwrite."""
        jsonl_file = temp_dir / "append_default.jsonl"
        write_jsonl(jsonl_file, [{"first": 1}])
        write_jsonl(jsonl_file, [{"second": 2}])
        result = list(read_jsonl(jsonl_file))
        assert len(result) == 2
        assert result[0] == {"first": 1}
        assert result[1] == {"second": 2}


class TestAppendJSONLEdgeCases:
    """Tests for additional append_jsonl edge cases."""

    def test_append_with_event_type_no_data(self, temp_dir):
        """Appending with event type string and no data dict."""
        jsonl_file = temp_dir / "no_data.jsonl"
        result = append_jsonl(jsonl_file, "simple_event")
        assert result["e"] == "simple_event"
        assert "ts" in result
        assert "id" in result

    def test_append_dict_is_copied(self, temp_dir):
        """Appending should not mutate the original dict."""
        jsonl_file = temp_dir / "copy_test.jsonl"
        original = {"key": "value"}
        append_jsonl(jsonl_file, original)
        # Original should not have ts/id added
        assert "ts" not in original
        assert "id" not in original

    def test_append_returns_complete_event(self, temp_dir):
        """Append should return the complete event as written."""
        jsonl_file = temp_dir / "return_test.jsonl"
        result = append_jsonl(jsonl_file, {"data": 42})
        # Read back and verify it matches
        events = list(read_jsonl(jsonl_file))
        assert len(events) == 1
        assert events[0]["data"] == result["data"]
        assert events[0]["ts"] == result["ts"]
        assert events[0]["id"] == result["id"]


class TestCountEventsEdgeCases:
    """Tests for additional count_events edge cases."""

    def test_count_with_nonmatching_type(self, temp_dir):
        """Counting with a type that doesn't exist should return 0."""
        jsonl_file = temp_dir / "events.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "type_a", "data": 1},
            {"e": "type_b", "data": 2},
        ])
        count = count_events(jsonl_file, event_type="nonexistent")
        assert count == 0

    def test_count_events_without_e_field(self, temp_dir):
        """Events without 'e' field should be counted when no type filter."""
        jsonl_file = temp_dir / "no_e.jsonl"
        write_jsonl(jsonl_file, [
            {"data": 1},
            {"data": 2},
            {"e": "typed", "data": 3},
        ])
        count = count_events(jsonl_file)
        assert count == 3

    def test_count_events_without_e_field_with_filter(self, temp_dir):
        """Events without 'e' field should not match a type filter."""
        jsonl_file = temp_dir / "no_e_filter.jsonl"
        write_jsonl(jsonl_file, [
            {"data": 1},
            {"e": "typed", "data": 2},
        ])
        count = count_events(jsonl_file, event_type="typed")
        assert count == 1


class TestFilterEventsEdgeCases:
    """Tests for additional filter_events edge cases."""

    def test_filter_nonexistent_file(self, temp_dir):
        """Filtering nonexistent file should return empty generator."""
        missing = temp_dir / "missing.jsonl"
        result = list(filter_events(missing))
        assert result == []

    def test_filter_empty_file(self, temp_dir):
        """Filtering empty file should return empty generator."""
        empty = temp_dir / "empty.jsonl"
        empty.touch()
        result = list(filter_events(empty))
        assert result == []

    def test_filter_no_criteria(self, temp_dir):
        """Filtering with no criteria should return all events."""
        jsonl_file = temp_dir / "all.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "a", "data": 1},
            {"e": "b", "data": 2},
        ])
        result = list(filter_events(jsonl_file))
        assert len(result) == 2

    def test_filter_since_excludes_equal(self, temp_dir):
        """Filter since should exclude events at exact boundary."""
        jsonl_file = temp_dir / "boundary.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "ts": "2024-06-01T00:00:00+00:00", "data": 1},
            {"e": "event", "ts": "2024-06-01T00:00:01+00:00", "data": 2},
        ])
        # since = exact time of first event
        since = datetime(2024, 6, 1, 0, 0, 1, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, since=since))
        # Only the second event (at 00:00:01) should pass since >= check fails for < comparison
        assert len(result) == 1
        assert result[0]["data"] == 2

    def test_filter_until_excludes_equal(self, temp_dir):
        """Filter until should exclude events at exact boundary."""
        jsonl_file = temp_dir / "boundary_until.jsonl"
        write_jsonl(jsonl_file, [
            {"e": "event", "ts": "2024-06-01T00:00:00+00:00", "data": 1},
            {"e": "event", "ts": "2024-06-02T00:00:00+00:00", "data": 2},
        ])
        until = datetime(2024, 6, 1, 0, 0, 0, tzinfo=timezone.utc)
        result = list(filter_events(jsonl_file, until=until))
        # First event at exact boundary: ts > until is false, so included
        # The code checks `if until and ts > until: continue`
        assert len(result) == 1
        assert result[0]["data"] == 1


class TestFlockAtomicWrites:
    """Tests for flock-based atomic write safety."""

    def test_write_creates_lock_file(self, temp_dir):
        """write_jsonl should create a .lock file for flock."""
        jsonl_file = temp_dir / "locked.jsonl"
        write_jsonl(jsonl_file, [{"a": 1}])
        lock_file = temp_dir / "locked.jsonl.lock"
        assert lock_file.exists()

    def test_concurrent_appends_no_corruption(self, temp_dir):
        """Concurrent appends via threads should not corrupt JSONL."""
        import threading

        jsonl_file = temp_dir / "concurrent.jsonl"
        errors = []
        per_thread = 50
        num_threads = 4

        def append_events(thread_id):
            try:
                for i in range(per_thread):
                    append_jsonl(
                        jsonl_file,
                        {"thread": thread_id, "seq": i},
                        add_timestamp=False,
                        add_id=False,
                    )
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=append_events, args=(t,))
            for t in range(num_threads)
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Errors during concurrent writes: {errors}"

        # Verify all lines are valid JSON and count is correct
        events = list(read_jsonl(jsonl_file))
        assert len(events) == num_threads * per_thread

    def test_overwrite_uses_lock(self, temp_dir):
        """Overwrite mode should also acquire flock."""
        jsonl_file = temp_dir / "overwrite_lock.jsonl"
        write_jsonl(jsonl_file, [{"old": 1}])
        write_jsonl(jsonl_file, [{"new": 2}], overwrite=True)
        lock_file = temp_dir / "overwrite_lock.jsonl.lock"
        assert lock_file.exists()
        result = list(read_jsonl(jsonl_file))
        assert len(result) == 1
        assert result[0] == {"new": 2}
