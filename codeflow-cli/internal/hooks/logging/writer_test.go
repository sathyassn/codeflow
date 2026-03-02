package logging

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// writeTestEnvFile creates a codeflow-env.sh in dir so ResolveSessionID picks
// up the session ID without t.Setenv (which is incompatible with t.Parallel).
func writeTestEnvFile(t *testing.T, dir, sessionID string) {
	t.Helper()
	runtimeDir := filepath.Join(dir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatalf("MkdirAll for env file: %v", err)
	}
	content := "export CODEFLOW_SESSION_ID='" + sessionID + "'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile for env file: %v", err)
	}
}

func TestNewActivityWriter(t *testing.T) {
	t.Parallel()

	t.Run("creates directory and writer", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		logDir := filepath.Join(dir, "logs", "sessions")

		w, err := NewActivityWriter(dir, "logs/sessions")
		if err != nil {
			t.Fatalf("NewActivityWriter() error = %v", err)
		}
		if w.Dir() != logDir {
			t.Errorf("Dir() = %q, want %q", w.Dir(), logDir)
		}
		if _, err := os.Stat(logDir); err != nil {
			t.Errorf("log directory not created: %v", err)
		}
	})

	t.Run("absolute log directory", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		absLogDir := filepath.Join(dir, "abs-logs")

		w, err := NewActivityWriter("/unused", absLogDir)
		if err != nil {
			t.Fatalf("NewActivityWriter() error = %v", err)
		}
		if w.Dir() != absLogDir {
			t.Errorf("Dir() = %q, want %q", w.Dir(), absLogDir)
		}
	})
}

func TestActivityWriter_Append(t *testing.T) {
	t.Parallel()

	fixedTime := time.Date(2026, 3, 1, 10, 30, 0, 0, time.UTC)

	t.Run("writes record to date-rotated file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, err := NewActivityWriter(dir, "logs")
		if err != nil {
			t.Fatalf("NewActivityWriter() error = %v", err)
		}
		w.Now = func() time.Time { return fixedTime }

		record := map[string]any{
			"ts":    w.Timestamp(),
			"event": "test_event",
		}
		if err := w.Append("session", record); err != nil {
			t.Fatalf("Append() error = %v", err)
		}

		expectedFile := filepath.Join(dir, "logs", "session-2026-03-01.jsonl")
		data, err := os.ReadFile(expectedFile)
		if err != nil {
			t.Fatalf("reading log file: %v", err)
		}

		var parsed map[string]any
		if err := json.Unmarshal(data, &parsed); err != nil {
			t.Fatalf("parsing log entry: %v", err)
		}
		if parsed["event"] != "test_event" {
			t.Errorf("event = %v, want %q", parsed["event"], "test_event")
		}
	})

	t.Run("appends multiple records", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, err := NewActivityWriter(dir, "logs")
		if err != nil {
			t.Fatalf("NewActivityWriter() error = %v", err)
		}
		w.Now = func() time.Time { return fixedTime }

		for i := range 3 {
			record := map[string]any{"n": i}
			if err := w.Append("test", record); err != nil {
				t.Fatalf("Append(%d) error = %v", i, err)
			}
		}

		data, err := os.ReadFile(filepath.Join(dir, "logs", "test-2026-03-01.jsonl"))
		if err != nil {
			t.Fatalf("reading log file: %v", err)
		}
		lines := strings.Split(strings.TrimSpace(string(data)), "\n")
		if len(lines) != 3 {
			t.Errorf("got %d lines, want 3", len(lines))
		}
	})

	t.Run("different dates produce different files", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, err := NewActivityWriter(dir, "logs")
		if err != nil {
			t.Fatalf("NewActivityWriter() error = %v", err)
		}

		day1 := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
		day2 := time.Date(2026, 3, 2, 0, 0, 0, 0, time.UTC)

		w.Now = func() time.Time { return day1 }
		_ = w.Append("session", map[string]any{"day": 1})

		w.Now = func() time.Time { return day2 }
		_ = w.Append("session", map[string]any{"day": 2})

		if _, err := os.Stat(filepath.Join(dir, "logs", "session-2026-03-01.jsonl")); err != nil {
			t.Error("day 1 file not created")
		}
		if _, err := os.Stat(filepath.Join(dir, "logs", "session-2026-03-02.jsonl")); err != nil {
			t.Error("day 2 file not created")
		}
	})
}

func TestActivityWriter_Timestamp(t *testing.T) {
	t.Parallel()

	fixedTime := time.Date(2026, 3, 1, 14, 5, 30, 123000000, time.UTC)
	w := &ActivityWriter{
		Now: func() time.Time { return fixedTime },
	}

	got := w.Timestamp()
	want := "2026-03-01T14:05:30.123Z"
	if got != want {
		t.Errorf("Timestamp() = %q, want %q", got, want)
	}
}

func TestResolveSessionID(t *testing.T) {
	// NOTE: no t.Parallel — subtests use t.Setenv which is incompatible with parallel ancestors
	t.Run("from env file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(
			filepath.Join(runtimeDir, "codeflow-env.sh"),
			[]byte("export CODEFLOW_SESSION_ID='ses-1234567890123abcdef012345'\n"),
			0o644,
		)

		got := ResolveSessionID(dir)
		if got != "ses-1234567890123abcdef012345" {
			t.Errorf("ResolveSessionID() = %q, want env file value", got)
		}
	})

	t.Run("from environment variable", func(t *testing.T) {
		dir := t.TempDir()
		t.Setenv("CODEFLOW_SESSION_ID", "ses-from-env")

		got := ResolveSessionID(dir)
		if got != "ses-from-env" {
			t.Errorf("ResolveSessionID() = %q, want %q", got, "ses-from-env")
		}
	})

	t.Run("from current-session-id file", func(t *testing.T) {
		dir := t.TempDir()
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(
			filepath.Join(runtimeDir, "current-session-id"),
			[]byte("ses-from-file"),
			0o644,
		)
		// Ensure env var is not set for this subtest.
		t.Setenv("CODEFLOW_SESSION_ID", "")

		got := ResolveSessionID(dir)
		if got != "ses-from-file" {
			t.Errorf("ResolveSessionID() = %q, want %q", got, "ses-from-file")
		}
	})

	t.Run("fallback to unknown", func(t *testing.T) {
		dir := t.TempDir()
		t.Setenv("CODEFLOW_SESSION_ID", "")

		got := ResolveSessionID(dir)
		if got != "unknown" {
			t.Errorf("ResolveSessionID() = %q, want %q", got, "unknown")
		}
	})
}

func TestParseEnvFileSessionID(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		content string
		want    string
	}{
		{"single-quoted", "export CODEFLOW_SESSION_ID='ses-abc123'\n", "ses-abc123"},
		{"double-quoted", "export CODEFLOW_SESSION_ID=\"ses-xyz789\"\n", "ses-xyz789"},
		{"no-quotes", "export CODEFLOW_SESSION_ID=ses-noquote\n", "ses-noquote"},
		{"with-other-vars", "export FOO=bar\nexport CODEFLOW_SESSION_ID='ses-found'\nexport BAZ=qux\n", "ses-found"},
		{"empty-content", "", ""},
		{"no-match", "export OTHER_VAR=value\n", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := parseEnvFileSessionID(tt.content)
			if got != tt.want {
				t.Errorf("parseEnvFileSessionID() = %q, want %q", got, tt.want)
			}
		})
	}
}
