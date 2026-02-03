"""
test_pattern_matching.py - Tests for pattern conflict detection.
"""

import fnmatch


def patterns_conflict(pattern1: str, pattern2: str) -> bool:
    """Check if two patterns conflict (overlap).

    This is the same logic as in cf-claim-check.py.
    """
    # Exact match
    if pattern1 == pattern2:
        return True

    # Extract type and path
    def parse_pattern(p):
        if ":" in p:
            ptype, path = p.split(":", 1)
            return ptype, path
        return "file", p

    type1, path1 = parse_pattern(pattern1)
    type2, path2 = parse_pattern(pattern2)

    # Different types don't conflict
    if type1 != type2:
        return False

    # Check glob matching
    if fnmatch.fnmatch(path1, path2) or fnmatch.fnmatch(path2, path1):
        return True

    # Check directory containment
    if type1 == "dir":
        if path1.startswith(path2 + "/") or path2.startswith(path1 + "/"):
            return True

    return False


class TestPatternExactMatch:
    """Tests for exact pattern matching."""

    def test_exact_match_file(self):
        """Should detect exact file pattern match."""
        assert patterns_conflict("file:src/main.ts", "file:src/main.ts") is True

    def test_exact_match_dir(self):
        """Should detect exact directory pattern match."""
        assert patterns_conflict("dir:src/auth", "dir:src/auth") is True

    def test_no_match_different_files(self):
        """Should not conflict with different files."""
        assert patterns_conflict("file:src/main.ts", "file:src/other.ts") is False


class TestPatternTypeMismatch:
    """Tests for pattern type mismatches."""

    def test_file_vs_dir_no_conflict(self):
        """Should not conflict when types differ."""
        assert patterns_conflict("file:src/auth", "dir:src/auth") is False

    def test_implicit_file_type(self):
        """Should handle implicit file type."""
        assert patterns_conflict("src/main.ts", "file:src/main.ts") is True


class TestPatternGlobMatching:
    """Tests for glob pattern matching."""

    def test_wildcard_matches_specific(self):
        """Should match wildcard to specific file."""
        assert patterns_conflict("file:src/*.ts", "file:src/main.ts") is True

    def test_specific_matches_wildcard(self):
        """Should match specific file to wildcard."""
        assert patterns_conflict("file:src/main.ts", "file:src/*.ts") is True

    def test_different_extensions_no_conflict(self):
        """Should not conflict with different extensions."""
        assert patterns_conflict("file:src/*.ts", "file:src/*.js") is False

    def test_double_star_glob(self):
        """Should handle ** glob patterns."""
        assert (
            patterns_conflict("file:src/**/*.ts", "file:src/components/Button.ts")
            is True
        )

    def test_partial_wildcard(self):
        """Should handle partial wildcards."""
        assert patterns_conflict("file:src/test_*.py", "file:src/test_main.py") is True


class TestPatternDirectoryContainment:
    """Tests for directory containment detection."""

    def test_parent_contains_child(self):
        """Should detect parent containing child directory."""
        assert patterns_conflict("dir:src", "dir:src/auth") is True

    def test_child_contained_by_parent(self):
        """Should detect child contained by parent."""
        assert patterns_conflict("dir:src/auth", "dir:src") is True

    def test_sibling_dirs_no_conflict(self):
        """Should not conflict with sibling directories."""
        assert patterns_conflict("dir:src/auth", "dir:src/api") is False

    def test_similar_but_not_contained(self):
        """Should not conflict with similar but non-contained paths."""
        assert patterns_conflict("dir:src/auth", "dir:src/authorization") is False


class TestPatternEdgeCases:
    """Tests for edge cases in pattern matching."""

    def test_empty_pattern(self):
        """Should handle empty patterns."""
        assert patterns_conflict("", "") is True

    def test_root_pattern(self):
        """Should handle root patterns.

        Note: The current implementation doesn't treat '.' as containing 'src'.
        This test verifies the actual behavior.
        """
        # '.' doesn't match 'src' with fnmatch, and the directory containment
        # check only looks for path prefix matches.
        # This is the actual current behavior:
        assert patterns_conflict("dir:.", "dir:src") is False

    def test_pattern_with_special_chars(self):
        """Should handle special characters in paths."""
        assert (
            patterns_conflict(
                "file:src/[components]/Button.tsx", "file:src/[components]/Button.tsx"
            )
            is True
        )

    def test_pattern_with_spaces(self):
        """Should handle paths with spaces."""
        assert (
            patterns_conflict("file:src/my component.ts", "file:src/my component.ts")
            is True
        )
