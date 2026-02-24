// Package testutil provides test helper functions for the codeflow CLI.
package testutil

import (
	"io/fs"
	"os"
	"path/filepath"
	"testing"
)

// TempProject creates a temporary directory with a minimal .codeflow/ structure
// suitable for testing. It registers cleanup with t.Cleanup and returns the path
// to the temporary project root.
func TempProject(t *testing.T) string {
	t.Helper()

	dir := t.TempDir()

	dirs := []string{
		filepath.Join(dir, ".codeflow", "config"),
		filepath.Join(dir, ".codeflow", "scripts"),
		filepath.Join(dir, ".claude"),
		filepath.Join(dir, ".state", "db"),
		filepath.Join(dir, ".state", "ledger"),
	}

	for _, d := range dirs {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatalf("testutil.TempProject: failed to create %s: %v", d, err)
		}
	}

	return dir
}

// CopyFixture copies a named test fixture from testdata/projects/ into a
// temporary directory and returns the path to the copy. The fixture name
// corresponds to a subdirectory under testdata/projects/ (e.g., "valid-project").
func CopyFixture(t *testing.T, fixtureName string) string {
	t.Helper()

	fixtureRoot := filepath.Join(findModuleRoot(t), "testdata", "projects", fixtureName)

	if _, err := os.Stat(fixtureRoot); os.IsNotExist(err) {
		t.Fatalf("testutil.CopyFixture: fixture %q not found at %s", fixtureName, fixtureRoot)
	}

	dest := t.TempDir()

	if err := copyDir(fixtureRoot, dest); err != nil {
		t.Fatalf("testutil.CopyFixture: failed to copy fixture %q: %v", fixtureName, err)
	}

	return dest
}

// copyDir recursively copies src directory contents to dest. Both src and dest
// must already exist. Returns an error if any file operation fails.
func copyDir(src, dest string) error {
	return filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}

		relPath, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}

		destPath := filepath.Join(dest, relPath)

		if d.IsDir() {
			return os.MkdirAll(destPath, 0o755)
		}

		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		return os.WriteFile(destPath, data, info.Mode())
	})
}

// AssertNoError fails the test immediately if err is not nil. The msg parameter
// provides context about what operation produced the error.
func AssertNoError(t *testing.T, err error, msg string) {
	t.Helper()
	if err != nil {
		t.Fatalf("%s: unexpected error: %v", msg, err)
	}
}

// findModuleRoot walks up from the current working directory to find the
// directory containing go.mod, which is the module root (codeflow-cli/).
func findModuleRoot(t *testing.T) string {
	t.Helper()

	dir, err := os.Getwd()
	if err != nil {
		t.Fatalf("testutil: failed to get working directory: %v", err)
	}

	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("testutil: could not find go.mod in any parent directory")
		}
		dir = parent
	}
}
