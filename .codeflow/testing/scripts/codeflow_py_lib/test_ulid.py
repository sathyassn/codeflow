"""
test_ulid.py - Tests for ULID generation module.
"""

import time

import pytest
from codeflow_py_lib import generate_ulid, parse_ulid
from codeflow_py_lib.ulid import (
    DECODING,
    ENCODING,
    _encode_random,
    _encode_timestamp,
    ulid_timestamp,
)


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

    def test_generate_ulid_with_custom_timestamp(self):
        """Should accept a custom timestamp_ms."""
        ts = 1704067200000  # 2024-01-01T00:00:00Z
        ulid = generate_ulid(timestamp_ms=ts)
        assert len(ulid) == 26
        parsed_ts, _ = parse_ulid(ulid)
        assert parsed_ts == ts

    def test_generate_ulid_with_zero_timestamp(self):
        """Should handle timestamp of zero."""
        ulid = generate_ulid(timestamp_ms=0)
        assert len(ulid) == 26
        parsed_ts, _ = parse_ulid(ulid)
        assert parsed_ts == 0


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

    def test_parse_ulid_invalid_length(self):
        """Should raise ValueError for wrong-length ULID."""
        with pytest.raises(ValueError, match="Invalid ULID length"):
            parse_ulid("SHORT")

    def test_parse_ulid_roundtrip(self):
        """Parsing a generated ULID should recover the original timestamp."""
        ts = int(time.time() * 1000)
        ulid = generate_ulid(timestamp_ms=ts)
        parsed_ts, _ = parse_ulid(ulid)
        assert parsed_ts == ts

    def test_parse_ulid_case_insensitive(self):
        """Should parse lowercase ULIDs correctly."""
        ulid = generate_ulid()
        ts1, rand1 = parse_ulid(ulid)
        ts2, rand2 = parse_ulid(ulid.lower())
        assert ts1 == ts2
        assert rand1 == rand2


class TestUlidTimestamp:
    """Tests for ulid_timestamp function."""

    def test_extracts_timestamp(self):
        """Should extract timestamp from ULID."""
        ts = 1704067200000
        ulid = generate_ulid(timestamp_ms=ts)
        assert ulid_timestamp(ulid) == ts

    def test_recent_timestamp(self):
        """Should return a recent timestamp for freshly generated ULID."""
        ulid = generate_ulid()
        ts = ulid_timestamp(ulid)
        now_ms = int(time.time() * 1000)
        assert abs(now_ms - ts) < 5000  # Within 5 seconds


class TestEncodeTimestamp:
    """Tests for _encode_timestamp internal function."""

    def test_returns_10_chars(self):
        """Should return exactly 10 characters."""
        result = _encode_timestamp(1704067200000)
        assert len(result) == 10

    def test_zero_timestamp(self):
        """Should encode zero timestamp."""
        result = _encode_timestamp(0)
        assert len(result) == 10
        assert result == "0000000000"

    def test_all_valid_chars(self):
        """All characters should be in the encoding alphabet."""
        result = _encode_timestamp(1704067200000)
        assert all(c in ENCODING for c in result)


class TestEncodeRandom:
    """Tests for _encode_random internal function."""

    def test_returns_16_chars(self):
        """Should return exactly 16 characters."""
        result = _encode_random(b"\x00" * 10)
        assert len(result) == 16

    def test_all_valid_chars(self):
        """All characters should be in the encoding alphabet."""
        result = _encode_random(b"\xff" * 10)
        assert all(c in ENCODING for c in result)

    def test_zero_bytes(self):
        """Zero bytes should encode to all zeros."""
        result = _encode_random(b"\x00" * 10)
        assert result == "0000000000000000"


class TestEncodingConstants:
    """Tests for encoding/decoding constants."""

    def test_encoding_length(self):
        """ENCODING should have 32 characters."""
        assert len(ENCODING) == 32

    def test_decoding_roundtrip(self):
        """Each encoding char should decode back to its index."""
        for i, c in enumerate(ENCODING):
            assert DECODING[c] == i

    def test_decoding_length(self):
        """DECODING should have 32 entries."""
        assert len(DECODING) == 32
