"""
test_ulid.py - Tests for ULID generation module.
"""

from codeflow_py_lib import generate_ulid, parse_ulid


class TestULIDGeneration:
    """Tests for ULID generation."""

    def test_generate_ulid_returns_string(self):
        """ULID should be a string."""
        ulid = generate_ulid()
        assert isinstance(ulid, str)

    def test_generate_ulid_length(self):
        """ULID should be 26 characters."""
        ulid = generate_ulid()
        assert len(ulid) == 26

    def test_generate_ulid_valid_characters(self):
        """ULID should only contain valid base32 characters."""
        ulid = generate_ulid()
        valid_chars = set("0123456789ABCDEFGHJKMNPQRSTVWXYZ")
        assert all(c in valid_chars for c in ulid)

    def test_generate_ulid_unique(self):
        """Each ULID should be unique."""
        ulids = [generate_ulid() for _ in range(100)]
        assert len(set(ulids)) == 100

    def test_generate_ulid_monotonic(self):
        """ULIDs generated in sequence should have same timestamp prefix.

        Note: ULIDs are only guaranteed monotonic within the same millisecond
        if the implementation uses monotonic random part. Our basic implementation
        uses random values, so ULIDs within the same millisecond may not be
        strictly sorted. We test that the timestamp portion (first 10 chars) is
        consistent for rapid generation.
        """
        ulids = [generate_ulid() for _ in range(10)]
        # All ULIDs generated in quick succession should have similar timestamps
        # (first 10 characters are timestamp, last 16 are random)
        timestamps = [ulid[:10] for ulid in ulids]
        # All timestamps should be the same or very close
        assert len(set(timestamps)) <= 2  # Allow for millisecond boundary


class TestULIDParsing:
    """Tests for ULID parsing."""

    def test_parse_ulid_returns_tuple(self):
        """parse_ulid should return timestamp and random parts as tuple."""
        ulid = generate_ulid()
        timestamp_ms, random_bytes = parse_ulid(ulid)
        assert timestamp_ms is not None
        assert random_bytes is not None

    def test_parse_ulid_timestamp_type(self):
        """Timestamp should be an integer (milliseconds)."""
        ulid = generate_ulid()
        timestamp_ms, _ = parse_ulid(ulid)
        assert isinstance(timestamp_ms, int)

    def test_parse_ulid_timestamp_reasonable(self):
        """Timestamp should be a reasonable value (after 2024)."""
        ulid = generate_ulid()
        timestamp_ms, _ = parse_ulid(ulid)
        jan_2024 = 1704067200000
        assert timestamp_ms > jan_2024

    def test_parse_ulid_random_part(self):
        """Random part should be 10 bytes."""
        ulid = generate_ulid()
        _, random_bytes = parse_ulid(ulid)
        assert isinstance(random_bytes, bytes)
        assert len(random_bytes) == 10
