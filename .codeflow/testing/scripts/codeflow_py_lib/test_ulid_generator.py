"""
test_ulid_generator.py - Tests for ULID generator CLI wrapper.
"""

import subprocess
import sys
import time
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

from codeflow_py_lib.ulid import parse_ulid
from codeflow_py_lib.ulid_generator import generate_ulid, main, parse_args

VALID_CHARS = set("0123456789ABCDEFGHJKMNPQRSTVWXYZ")
SCRIPT_PATH = str(
    Path(__file__).parent.parent.parent.parent / "scripts" / "codeflow_py_lib" / "ulid_generator.py"
)


class TestGenerateULID:
    """Tests for the re-exported generate_ulid function."""

    def test_ulid_length(self):
        """Generated ULID is 26 characters."""
        ulid = generate_ulid()
        assert len(ulid) == 26

    def test_ulid_valid_characters(self):
        """Generated ULID uses only valid Crockford Base32 characters."""
        ulid = generate_ulid()
        assert all(c in VALID_CHARS for c in ulid)

    def test_ulid_monotonic_ordering(self):
        """Two ULIDs generated in sequence are lexicographically ordered."""
        ulid1 = generate_ulid()
        time.sleep(0.002)
        ulid2 = generate_ulid()
        assert ulid1 < ulid2

    def test_ulid_timestamp_reasonable(self):
        """Timestamp portion decodes to a time within the last minute."""
        ulid = generate_ulid()
        timestamp_ms, _ = parse_ulid(ulid)
        now_ms = int(time.time() * 1000)
        one_minute_ms = 60 * 1000
        assert now_ms - timestamp_ms < one_minute_ms

    def test_ulid_uniqueness(self):
        """Multiple ULIDs are unique."""
        ulids = [generate_ulid() for _ in range(100)]
        assert len(set(ulids)) == 100


class TestMainFunction:
    """Tests for the main() function."""

    def test_main_returns_zero(self, capsys):
        """main() should return 0 on success."""
        args = Namespace(count=1)
        result = main(args)
        assert result == 0

    def test_main_prints_one_ulid(self, capsys):
        """main() with count=1 should print one ULID."""
        args = Namespace(count=1)
        main(args)
        output = capsys.readouterr().out.strip()
        lines = output.splitlines()
        assert len(lines) == 1
        assert len(lines[0]) == 26
        assert all(c in VALID_CHARS for c in lines[0])

    def test_main_prints_multiple_ulids(self, capsys):
        """main() with count=3 should print three ULIDs."""
        args = Namespace(count=3)
        main(args)
        output = capsys.readouterr().out.strip()
        lines = output.splitlines()
        assert len(lines) == 3
        for line in lines:
            assert len(line) == 26

    def test_main_prints_zero_ulids(self, capsys):
        """main() with count=0 should print nothing."""
        args = Namespace(count=0)
        result = main(args)
        assert result == 0
        output = capsys.readouterr().out.strip()
        assert output == ""


class TestParseArgs:
    """Tests for parse_args() function."""

    def test_default_count(self):
        """Default count should be 1."""
        with patch("sys.argv", ["ulid_generator.py"]):
            args = parse_args()
            assert args.count == 1

    def test_custom_count(self):
        """Should accept --count flag."""
        with patch("sys.argv", ["ulid_generator.py", "--count", "5"]):
            args = parse_args()
            assert args.count == 5


class TestCLIMode:
    """Tests for CLI invocation."""

    def test_cli_default_output(self):
        """CLI prints one ULID by default."""
        result = subprocess.run(
            [sys.executable, SCRIPT_PATH],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0
        lines = result.stdout.strip().splitlines()
        assert len(lines) == 1
        assert len(lines[0]) == 26
        assert all(c in VALID_CHARS for c in lines[0])

    def test_cli_count_flag(self):
        """CLI --count N generates N ULIDs."""
        result = subprocess.run(
            [sys.executable, SCRIPT_PATH, "--count", "5"],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0
        lines = result.stdout.strip().splitlines()
        assert len(lines) == 5
        for line in lines:
            assert len(line) == 26
            assert all(c in VALID_CHARS for c in line)

    def test_cli_count_zero(self):
        """CLI --count 0 prints nothing."""
        result = subprocess.run(
            [sys.executable, SCRIPT_PATH, "--count", "0"],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0
        assert result.stdout.strip() == ""
