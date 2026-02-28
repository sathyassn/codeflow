package validate

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// writeFixture creates a temporary file with the given content and returns its path.
func writeFixture(t *testing.T, name, content string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("writeFixture(%s): %v", name, err)
	}
	return path
}

// hasError checks if any validation error matches the given field and message substring.
func hasError(errs []ValidationError, field, msgSubstr string) bool {
	for _, e := range errs {
		if e.Field == field && (msgSubstr == "" || strings.Contains(e.Error(), msgSubstr)) {
			return true
		}
	}
	return false
}

// hasWarning checks if any validation warning matches the given field and message substring.
func hasWarning(warns []ValidationWarning, field, msgSubstr string) bool {
	for _, w := range warns {
		if w.Field == field && (msgSubstr == "" || strings.Contains(w.String(), msgSubstr)) {
			return true
		}
	}
	return false
}
