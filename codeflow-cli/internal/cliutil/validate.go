package cliutil

import (
	"errors"
	"fmt"
	"regexp"
	"strings"
)

// Validation sentinel errors.
var (
	ErrInvalidULID     = errors.New("invalid ULID format")
	ErrInvalidFilePath = errors.New("invalid file path")
)

// ulidRe matches a 26-character Crockford Base32 ULID (lowercase).
// Crockford Base32 uses: 0-9 and a-z excluding i, l, o, u.
var ulidRe = regexp.MustCompile(`^[0-9a-hjkmnp-tv-z]{26}$`)

// ValidateULID checks that id is a valid 26-character lowercase Crockford
// Base32 ULID. It does not accept prefixed IDs (e.g., "ses-..." or "task-...").
func ValidateULID(id string) error {
	if len(id) != 26 {
		return fmt.Errorf("%w: expected 26 characters, got %d", ErrInvalidULID, len(id))
	}
	if !ulidRe.MatchString(id) {
		return fmt.Errorf("%w: contains invalid Crockford Base32 characters", ErrInvalidULID)
	}
	return nil
}

// ValidateFilePath checks a file path for common safety issues:
//   - Rejects null bytes (path injection)
//   - Rejects path traversal sequences (..)
//   - Rejects empty paths
func ValidateFilePath(path string) error {
	if path == "" {
		return fmt.Errorf("%w: path is empty", ErrInvalidFilePath)
	}

	if strings.ContainsRune(path, 0) {
		return fmt.Errorf("%w: path contains null byte", ErrInvalidFilePath)
	}

	// Check for path traversal in each path segment.
	for _, segment := range strings.Split(path, "/") {
		if segment == ".." {
			return fmt.Errorf("%w: path contains traversal sequence (..)", ErrInvalidFilePath)
		}
	}

	return nil
}
