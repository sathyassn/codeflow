"""
ULID (Universally Unique Lexicographically Sortable Identifier) generation.

Provides consistent ID generation across all CodeFlow scripts.
"""

import os
import time
from typing import Tuple

# Crockford's Base32 alphabet
ENCODING = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
DECODING = {c: i for i, c in enumerate(ENCODING)}


def generate_ulid(timestamp_ms: "int | None" = None) -> str:
    """
    Generate a new ULID.

    Args:
        timestamp_ms: Optional timestamp in milliseconds. Uses current time if None.

    Returns:
        26-character ULID string.
    """
    if timestamp_ms is None:
        timestamp_ms = int(time.time() * 1000)

    # 10 characters for timestamp (48 bits)
    timestamp_chars = _encode_timestamp(timestamp_ms)

    # 16 characters for randomness (80 bits)
    random_bytes = os.urandom(10)
    random_chars = _encode_random(random_bytes)

    return timestamp_chars + random_chars


def parse_ulid(ulid: str) -> Tuple[int, bytes]:
    """
    Parse ULID into timestamp and random components.

    Args:
        ulid: 26-character ULID string.

    Returns:
        Tuple of (timestamp_ms, random_bytes).
    """
    if len(ulid) != 26:
        raise ValueError(f"Invalid ULID length: {len(ulid)}")

    ulid = ulid.upper()

    # Decode timestamp (first 10 chars)
    timestamp_ms = 0
    for char in ulid[:10]:
        timestamp_ms = timestamp_ms * 32 + DECODING[char]

    # Decode randomness (last 16 chars)
    random_int = 0
    for char in ulid[10:]:
        random_int = random_int * 32 + DECODING[char]
    random_bytes = random_int.to_bytes(10, "big")

    return timestamp_ms, random_bytes


def ulid_timestamp(ulid: str) -> int:
    """Extract timestamp in milliseconds from ULID."""
    timestamp_ms, _ = parse_ulid(ulid)
    return timestamp_ms


def _encode_timestamp(timestamp_ms: int) -> str:
    """Encode timestamp as 10 Base32 characters."""
    chars = []
    for _ in range(10):
        chars.append(ENCODING[timestamp_ms & 0x1F])
        timestamp_ms >>= 5
    return "".join(reversed(chars))


def _encode_random(random_bytes: bytes) -> str:
    """Encode 10 random bytes as 16 Base32 characters."""
    random_int = int.from_bytes(random_bytes, "big")
    chars = []
    for _ in range(16):
        chars.append(ENCODING[random_int & 0x1F])
        random_int >>= 5
    return "".join(reversed(chars))
