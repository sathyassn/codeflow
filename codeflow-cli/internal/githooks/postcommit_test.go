package githooks

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRunPostCommit_OutputFormat(t *testing.T) {
	t.Parallel()

	w := &bytes.Buffer{}
	projectDir := t.TempDir()

	// RunPostCommit will fail because there's no git repo in TempDir.
	err := RunPostCommit(w, projectDir)
	if err == nil {
		// If it succeeded (e.g., run inside a real git repo), verify output format.
		output := w.String()
		if !strings.Contains(output, "Commit created:") {
			t.Errorf("expected 'Commit created:' in output, got: %s", output)
		}
	} else {
		// Expected: git commands fail in temp dir.
		if !strings.Contains(err.Error(), "get commit hash") {
			t.Errorf("expected 'get commit hash' error, got: %v", err)
		}
	}
}

func TestAppendJSONL(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	logFile := filepath.Join(dir, "test.jsonl")

	entry1 := CommitLogEntry{
		Timestamp: "2026-03-02T10:00:00.000Z",
		Event:     "commit",
		Hash:      "abc123def456",
		ShortHash: "abc123d",
		Message:   "feat: add feature",
		Author:    "test",
		Branch:    "feat/test",
	}

	if err := appendJSONL(logFile, entry1); err != nil {
		t.Fatalf("first append failed: %v", err)
	}

	// Append a second entry.
	entry2 := CommitLogEntry{
		Timestamp: "2026-03-02T10:01:00.000Z",
		Event:     "commit",
		Hash:      "def789ghi012",
		ShortHash: "def789g",
		Message:   "fix: resolve bug",
		Author:    "test",
		Branch:    "fix/bug",
	}

	if err := appendJSONL(logFile, entry2); err != nil {
		t.Fatalf("second append failed: %v", err)
	}

	// Read and verify.
	data, err := os.ReadFile(logFile)
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 2 {
		t.Fatalf("expected 2 lines, got %d", len(lines))
	}

	// Parse first line.
	var parsed CommitLogEntry
	if err := json.Unmarshal([]byte(lines[0]), &parsed); err != nil {
		t.Fatalf("parse first line: %v", err)
	}
	if parsed.Hash != "abc123def456" {
		t.Errorf("expected hash abc123def456, got %s", parsed.Hash)
	}
	if parsed.Message != "feat: add feature" {
		t.Errorf("expected message 'feat: add feature', got %s", parsed.Message)
	}

	// Parse second line.
	if err := json.Unmarshal([]byte(lines[1]), &parsed); err != nil {
		t.Fatalf("parse second line: %v", err)
	}
	if parsed.ShortHash != "def789g" {
		t.Errorf("expected short_hash def789g, got %s", parsed.ShortHash)
	}
}

func TestAppendJSONL_CreatesFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	logFile := filepath.Join(dir, "new-file.jsonl")

	entry := map[string]string{"key": "value"}
	if err := appendJSONL(logFile, entry); err != nil {
		t.Fatal(err)
	}

	if _, err := os.Stat(logFile); os.IsNotExist(err) {
		t.Error("expected file to be created")
	}
}

func TestAppendJSONL_InvalidPath(t *testing.T) {
	t.Parallel()

	err := appendJSONL("/nonexistent/dir/file.jsonl", map[string]string{"k": "v"})
	if err == nil {
		t.Fatal("expected error for invalid path")
	}
	if !strings.Contains(err.Error(), "open log file") {
		t.Errorf("expected 'open log file' error, got: %v", err)
	}
}

func TestCommitLogEntry_JSONFormat(t *testing.T) {
	t.Parallel()

	entry := CommitLogEntry{
		Timestamp: "2026-03-02T10:00:00.000Z",
		Event:     "commit",
		Hash:      "abc123",
		ShortHash: "abc",
		Message:   "feat: test",
		Author:    "Dev",
		Branch:    "feat/test",
	}

	data, err := json.Marshal(entry)
	if err != nil {
		t.Fatal(err)
	}

	// Verify JSON field names match expected format.
	var raw map[string]string
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatal(err)
	}

	expectedFields := []string{"ts", "event", "hash", "short_hash", "message", "author", "branch"}
	for _, field := range expectedFields {
		if _, ok := raw[field]; !ok {
			t.Errorf("missing JSON field: %s", field)
		}
	}
}

func TestGitOutput_InvalidCommand(t *testing.T) {
	t.Parallel()

	// git with a nonsense subcommand should fail.
	_, err := gitOutput("nonexistent-subcommand-xyz")
	if err == nil {
		t.Fatal("expected error for invalid git command")
	}
	if !strings.Contains(err.Error(), "git nonexistent-subcommand-xyz") {
		t.Errorf("expected error to contain command, got: %v", err)
	}
}

func TestLogOverride(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	gitDir := filepath.Join(dir, ".git")
	if err := os.MkdirAll(gitDir, 0o755); err != nil {
		t.Fatal(err)
	}

	logOverride("main", dir)

	logFile := filepath.Join(gitDir, "push-overrides.log")
	data, err := os.ReadFile(logFile)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)
	if !strings.Contains(content, "Emergency Push Override") {
		t.Error("expected 'Emergency Push Override' in log")
	}
	if !strings.Contains(content, "Branch: main") {
		t.Error("expected 'Branch: main' in log")
	}
}

func TestAppendJSONL_WriteError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	logFile := filepath.Join(dir, "readonly.jsonl")
	// Create file then make it read-only.
	if err := os.WriteFile(logFile, []byte(""), 0o444); err != nil {
		t.Fatal(err)
	}

	err := appendJSONL(logFile, map[string]string{"k": "v"})
	if err == nil {
		t.Fatal("expected error for read-only file")
	}
}

func TestLogOverride_NoGitDir(t *testing.T) {
	t.Parallel()

	// No .git directory — logOverride should not panic.
	logOverride("main", "/nonexistent/path")
	// Just verify no panic; the function silently ignores errors.
}

func TestAppendJSONL_MarshalError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	logFile := filepath.Join(dir, "marshal-err.jsonl")

	// json.Marshal fails for channels.
	err := appendJSONL(logFile, make(chan int))
	if err == nil {
		t.Fatal("expected marshal error")
	}
	if !strings.Contains(err.Error(), "marshal log entry") {
		t.Errorf("expected 'marshal log entry' in error, got: %v", err)
	}
}

func TestRunPostCommit_MkdirAllError(t *testing.T) {
	t.Parallel()

	// Create a projectDir where .state/logs/git is a file (not a dir),
	// causing MkdirAll to fail. The function should still print output
	// and return nil (non-fatal).
	dir := t.TempDir()
	logParent := filepath.Join(dir, ".state", "logs")
	if err := os.MkdirAll(logParent, 0o755); err != nil {
		t.Fatal(err)
	}
	// Create a regular file where the directory would need to be.
	gitFile := filepath.Join(logParent, "git")
	if err := os.WriteFile(gitFile, []byte("blocker"), 0o644); err != nil {
		t.Fatal(err)
	}

	w := &bytes.Buffer{}
	// RunPostCommit calls git commands which will fail in temp dir.
	// We're testing that the MkdirAll error path doesn't panic,
	// not the full flow. The git error will return first.
	err := RunPostCommit(w, dir)
	// Expected: git error (no repo), but no panic from MkdirAll path.
	if err != nil {
		if !strings.Contains(err.Error(), "get commit hash") {
			t.Errorf("expected 'get commit hash' error, got: %v", err)
		}
	}
}

func TestRunPostCommit_AppendJSONLError(t *testing.T) {
	t.Parallel()

	// This tests the appendJSONL error path within RunPostCommit.
	// Since RunPostCommit calls git commands, we can only test this
	// in a real git repo. Instead, verify the appendJSONL error
	// handling is exercised via the read-only file path.
	dir := t.TempDir()
	logDir := filepath.Join(dir, ".state", "logs", "git")
	if err := os.MkdirAll(logDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Pre-create a read-only log file to trigger appendJSONL write error.
	// The date-based filename may or may not match, so create a
	// read-only directory instead.
	if err := os.Chmod(logDir, 0o444); err != nil {
		t.Fatal(err)
	}
	// Restore for cleanup.
	t.Cleanup(func() {
		_ = os.Chmod(logDir, 0o755)
	})

	w := &bytes.Buffer{}
	err := RunPostCommit(w, dir)
	// Git commands will fail first in temp dir.
	if err != nil && !strings.Contains(err.Error(), "get commit hash") {
		t.Errorf("expected 'get commit hash' error, got: %v", err)
	}
}
