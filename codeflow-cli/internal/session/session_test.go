package session

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// newTestDB creates a temporary database with the full schema initialized.
func newTestDB(t *testing.T) *db.DB {
	t.Helper()
	path := filepath.Join(t.TempDir(), "test.db")
	d, err := db.NewDB(path)
	if err != nil {
		t.Fatalf("newTestDB: %v", err)
	}
	t.Cleanup(func() { d.Close() })

	ctx := t.Context()
	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("InitFromSchema: %v", err)
	}
	return d
}

func TestStartHappyPath(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Verify session ID format: ses-{ulid} (26 char ULID, lowercase).
	if !strings.HasPrefix(sessionID, "ses-") {
		t.Errorf("session ID should start with 'ses-', got %q", sessionID)
	}
	// ULID is 26 chars, total = 4 (prefix) + 26 = 30.
	if len(sessionID) != 30 {
		t.Errorf("session ID length = %d, want 30 (ses- + 26 char ULID)", len(sessionID))
	}
	// Verify lowercase.
	if sessionID != strings.ToLower(sessionID) {
		t.Errorf("session ID should be lowercase, got %q", sessionID)
	}
}

func TestStartWritesJSONL(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Read the JSONL file and verify the event.
	jsonlPath := filepath.Join(ledgerDir, "sessions.jsonl")
	data, err := os.ReadFile(jsonlPath)
	if err != nil {
		t.Fatalf("reading JSONL: %v", err)
	}

	var event map[string]any
	if err := json.Unmarshal(data, &event); err != nil {
		t.Fatalf("parsing JSONL event: %v", err)
	}

	if event["event"] != "session_start" {
		t.Errorf("event type = %v, want session_start", event["event"])
	}
	if event["session_id"] != sessionID {
		t.Errorf("session_id = %v, want %s", event["session_id"], sessionID)
	}
	if event["claude_id"] != "test-claude-id" {
		t.Errorf("claude_id = %v, want test-claude-id", event["claude_id"])
	}
	if event["timestamp"] == nil || event["timestamp"] == "" {
		t.Error("timestamp should not be empty")
	}
}

func TestStartWritesCurrentSessionID(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Verify the current-session-id file was written.
	idFile := filepath.Join(runtimeDir, "current-session-id")
	data, err := os.ReadFile(idFile)
	if err != nil {
		t.Fatalf("reading current-session-id: %v", err)
	}

	got := string(data)
	if got != sessionID {
		t.Errorf("current-session-id = %q, want %q", got, sessionID)
	}
}

func TestStartInsertsDBRecord(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Query the sessions table to verify the record.
	var id, userID, userHost, status string
	err = d.QueryRow(ctx,
		"SELECT id, user_id, user_host, status FROM sessions WHERE id = ?", sessionID,
	).Scan(&id, &userID, &userHost, &status)
	if err != nil {
		t.Fatalf("querying session: %v", err)
	}

	if id != sessionID {
		t.Errorf("id = %q, want %q", id, sessionID)
	}
	if userID != "test-claude-id" {
		t.Errorf("user_id = %q, want %q", userID, "test-claude-id")
	}
	if userHost == "" {
		t.Error("user_host should not be empty")
	}
	if status != "active" {
		t.Errorf("status = %q, want %q", status, "active")
	}
}

func TestEndHappyPath(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Verify the session record was updated.
	var status string
	err = d.QueryRow(ctx,
		"SELECT status FROM sessions WHERE id = ?", sessionID,
	).Scan(&status)
	if err != nil {
		t.Fatalf("querying session: %v", err)
	}

	if status != "completed" {
		t.Errorf("status = %q, want %q", status, "completed")
	}
}

func TestEndCalculatesDuration(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Verify duration_seconds is non-negative.
	var durationSeconds int64
	err = d.QueryRow(ctx,
		"SELECT duration_seconds FROM sessions WHERE id = ?", sessionID,
	).Scan(&durationSeconds)
	if err != nil {
		t.Fatalf("querying duration: %v", err)
	}

	if durationSeconds < 0 {
		t.Errorf("duration_seconds = %d, want >= 0", durationSeconds)
	}
}

func TestEndCleansUp(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	if _, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir); err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Verify the file exists before End.
	idFile := filepath.Join(runtimeDir, "current-session-id")
	if _, err := os.Stat(idFile); err != nil {
		t.Fatalf("current-session-id should exist before End: %v", err)
	}

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Verify current-session-id was removed.
	if _, err := os.Stat(idFile); !os.IsNotExist(err) {
		t.Error("current-session-id should have been removed after End")
	}
}

func TestEndWritesJSONL(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Read the JSONL file and verify there are 2 events (start + end).
	jsonlPath := filepath.Join(ledgerDir, "sessions.jsonl")
	data, err := os.ReadFile(jsonlPath)
	if err != nil {
		t.Fatalf("reading JSONL: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 2 {
		t.Fatalf("expected 2 JSONL lines, got %d", len(lines))
	}

	// Parse the second event (session_end).
	var event map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &event); err != nil {
		t.Fatalf("parsing end event: %v", err)
	}

	if event["event"] != "session_end" {
		t.Errorf("event type = %v, want session_end", event["event"])
	}
	if event["session_id"] != sessionID {
		t.Errorf("session_id = %v, want %s", event["session_id"], sessionID)
	}
	if event["timestamp"] == nil || event["timestamp"] == "" {
		t.Error("timestamp should not be empty")
	}
	// duration_seconds should be a number >= 0.
	dur, ok := event["duration_seconds"].(float64)
	if !ok {
		t.Errorf("duration_seconds type = %T, want float64", event["duration_seconds"])
	} else if dur < 0 {
		t.Errorf("duration_seconds = %v, want >= 0", dur)
	}
}

func TestStartEmptyClaudeID(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	_, err := Start(ctx, d, "", ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error for empty claude ID, got nil")
	}
	if !errors.Is(err, ErrEmptyClaudeID) {
		t.Errorf("error = %v, want ErrEmptyClaudeID", err)
	}
}

func TestEndAlreadyEndedSession(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Re-write the current-session-id to simulate trying to end again.
	if err := writeCurrentSessionID(runtimeDir, sessionID); err != nil {
		t.Fatalf("writing session ID: %v", err)
	}

	// Try to end again -- should return ErrAlreadyEnded.
	err = End(ctx, d, ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error for already-ended session, got nil")
	}
	if !errors.Is(err, ErrAlreadyEnded) {
		t.Errorf("error = %v, want ErrAlreadyEnded", err)
	}
}

func TestEndNoActiveSession(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Try to end without starting -- no current-session-id file.
	err := End(ctx, d, ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error for no active session, got nil")
	}
	if !errors.Is(err, ErrNoActiveSession) {
		t.Errorf("error = %v, want ErrNoActiveSession", err)
	}
}

func TestCurrent(t *testing.T) {
	t.Run("returns session ID when file exists", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()
		ledgerDir := filepath.Join(t.TempDir(), "ledger")
		runtimeDir := filepath.Join(t.TempDir(), "runtime")

		sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
		if err != nil {
			t.Fatalf("Start: %v", err)
		}

		got, err := Current(runtimeDir)
		if err != nil {
			t.Fatalf("Current: %v", err)
		}
		if got != sessionID {
			t.Errorf("Current() = %q, want %q", got, sessionID)
		}
	})

	t.Run("returns error when no file", func(t *testing.T) {
		runtimeDir := filepath.Join(t.TempDir(), "nonexistent")

		_, err := Current(runtimeDir)
		if err == nil {
			t.Fatal("expected error when no file, got nil")
		}
		if !errors.Is(err, ErrNoActiveSession) {
			t.Errorf("error = %v, want ErrNoActiveSession", err)
		}
	})

	t.Run("returns error for empty file", func(t *testing.T) {
		runtimeDir := t.TempDir()
		idFile := filepath.Join(runtimeDir, CurrentSessionFile)
		if err := os.WriteFile(idFile, []byte(""), 0o644); err != nil {
			t.Fatalf("writing empty file: %v", err)
		}

		_, err := Current(runtimeDir)
		if err == nil {
			t.Fatal("expected error for empty file, got nil")
		}
		if !errors.Is(err, ErrNoActiveSession) {
			t.Errorf("error = %v, want ErrNoActiveSession", err)
		}
	})
}

func TestGenerateIDUniqueness(t *testing.T) {
	seen := make(map[string]bool)
	for range 100 {
		id := generateID()
		if seen[id] {
			t.Fatalf("duplicate session ID generated: %s", id)
		}
		seen[id] = true
	}
}

func TestStartCreatesDirectories(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Use nested paths that don't exist yet.
	baseDir := t.TempDir()
	ledgerDir := filepath.Join(baseDir, "deep", "ledger")
	runtimeDir := filepath.Join(baseDir, "deep", "runtime")

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Verify directories were created.
	if _, err := os.Stat(ledgerDir); err != nil {
		t.Errorf("ledger directory should exist: %v", err)
	}
	if _, err := os.Stat(runtimeDir); err != nil {
		t.Errorf("runtime directory should exist: %v", err)
	}
}

func TestStartEnsuresUserRow(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	claudeID := "user-unique-test-id"
	_, err := Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Verify the user row was created.
	var userID string
	err = d.QueryRow(ctx,
		"SELECT id FROM users WHERE id = ?", claudeID,
	).Scan(&userID)
	if err != nil {
		t.Fatalf("querying user: %v", err)
	}
	if userID != claudeID {
		t.Errorf("user id = %q, want %q", userID, claudeID)
	}
}

func TestStartOnClosedDB(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Close the DB to force errors.
	d.Close()

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error on closed DB, got nil")
	}
}

func TestEndOnClosedDB(t *testing.T) {
	// First start a session with a working DB, then close it and try to end.
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Close the DB to force error during End.
	d.Close()

	err = End(ctx, d, ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error on closed DB, got nil")
	}
}

func TestStartWithReadOnlyLedgerDir(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Use /dev/null as ledger dir -- cannot create subdirectories.
	ledgerDir := filepath.Join("/dev/null", "ledger")

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error with invalid ledger dir, got nil")
	}
}

func TestCurrentTrimsWhitespace(t *testing.T) {
	runtimeDir := t.TempDir()
	idFile := filepath.Join(runtimeDir, CurrentSessionFile)
	// Write session ID with trailing whitespace/newline.
	if err := os.WriteFile(idFile, []byte("ses-abc123\n"), 0o644); err != nil {
		t.Fatalf("writing file: %v", err)
	}

	got, err := Current(runtimeDir)
	if err != nil {
		t.Fatalf("Current: %v", err)
	}
	if got != "ses-abc123" {
		t.Errorf("Current() = %q, want %q (whitespace should be trimmed)", got, "ses-abc123")
	}
}

func TestStartWithSameClaudeIDTwice(t *testing.T) {
	// Verify INSERT OR IGNORE for users doesn't fail on second session.
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	claudeID := "same-claude-id"

	// First session.
	_, err := Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 1: %v", err)
	}
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End 1: %v", err)
	}

	// Second session with same claude ID should not fail.
	_, err = Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 2 with same claude ID: %v", err)
	}
}

func TestStartWithInvalidRuntimeDir(t *testing.T) {
	// Exercise the writeCurrentSessionID MkdirAll error path.
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	// /dev/null is a file, not a directory — MkdirAll will fail.
	runtimeDir := "/dev/null/runtime"

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error for invalid runtime dir")
	}
	if !strings.Contains(err.Error(), "current-session-id") {
		t.Errorf("expected error about current-session-id, got: %v", err)
	}
}

func TestEndWithEmptyStartedAt(t *testing.T) {
	// Exercises the duration calculation skip when started_at is empty.
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session normally.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Manually set started_at to empty to exercise the empty-string branch.
	_, err = d.Execute(ctx, `UPDATE sessions SET started_at = '' WHERE id = ?`, sessionID)
	if err != nil {
		t.Fatalf("clearing started_at: %v", err)
	}

	// End should still succeed, with durationSeconds defaulting to 0.
	err = End(ctx, d, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("End with empty started_at: %v", err)
	}
}

func TestMultipleStartEndCycles(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start and end two sessions in sequence.
	id1, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 1: %v", err)
	}
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End 1: %v", err)
	}

	id2, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 2: %v", err)
	}
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End 2: %v", err)
	}

	// Verify both sessions exist and are completed.
	if id1 == id2 {
		t.Error("two sessions should have different IDs")
	}

	for _, id := range []string{id1, id2} {
		var status string
		err := d.QueryRow(ctx,
			"SELECT status FROM sessions WHERE id = ?", id,
		).Scan(&status)
		if err != nil {
			t.Fatalf("querying session %s: %v", id, err)
		}
		if status != "completed" {
			t.Errorf("session %s status = %q, want completed", id, status)
		}
	}

	// Verify 4 JSONL events (2 starts + 2 ends).
	jsonlPath := filepath.Join(ledgerDir, "sessions.jsonl")
	data, err := os.ReadFile(jsonlPath)
	if err != nil {
		t.Fatalf("reading JSONL: %v", err)
	}
	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 4 {
		t.Errorf("JSONL lines = %d, want 4", len(lines))
	}
}
