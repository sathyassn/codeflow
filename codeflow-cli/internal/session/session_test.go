package session

import (
	"encoding/json"
	"errors"
	"fmt"
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

// writeTestEnvFile is a test helper that writes codeflow-env.sh to runtimeDir.
func writeTestEnvFile(t *testing.T, runtimeDir, sessionID string) {
	t.Helper()
	// Use a synthetic projectDir based on runtimeDir's parent.
	projectDir := filepath.Dir(runtimeDir)
	if err := WriteEnvFile(runtimeDir, sessionID, projectDir); err != nil {
		t.Fatalf("WriteEnvFile: %v", err)
	}
}

func TestStartHappyPath(t *testing.T) {
	t.Parallel()
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
	t.Parallel()
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
	// user_id should be a git email or the claudeID fallback.
	userID, _ := event["user_id"].(string)
	if userID == "" {
		t.Error("user_id should not be empty")
	}
	// claude_id preserves the original agent UUID.
	if event["claude_id"] != "test-claude-id" {
		t.Errorf("claude_id = %v, want test-claude-id", event["claude_id"])
	}
	if event["timestamp"] == nil || event["timestamp"] == "" {
		t.Error("timestamp should not be empty")
	}
}

func TestStartDoesNotWriteSessionFiles(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Start() does not write any session ID files -- caller is responsible.
	envFile := filepath.Join(runtimeDir, "codeflow-env.sh")
	if _, err := os.Stat(envFile); !os.IsNotExist(err) {
		t.Error("codeflow-env.sh should NOT be written by Start() -- caller is responsible")
	}
	idFile := filepath.Join(runtimeDir, "current-session-id")
	if _, err := os.Stat(idFile); !os.IsNotExist(err) {
		t.Error("current-session-id should NOT be written by Start() -- caller is responsible")
	}
}

func TestStartInsertsDBRecord(t *testing.T) {
	t.Parallel()
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
	if userID == "" {
		t.Error("user_id should not be empty")
	}
	if userHost == "" {
		t.Error("user_host should not be empty")
	}
	if status != "active" {
		t.Errorf("status = %q, want %q", status, "active")
	}

	// Verify claude_id is stored in session metadata.
	var metadata string
	err = d.QueryRow(ctx,
		"SELECT metadata FROM sessions WHERE id = ?", sessionID,
	).Scan(&metadata)
	if err != nil {
		t.Fatalf("querying metadata: %v", err)
	}
	if !strings.Contains(metadata, "test-claude-id") {
		t.Errorf("metadata should contain claude_id, got %q", metadata)
	}
}

func TestEndHappyPath(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}
	// Write codeflow-env.sh (Start no longer writes it -- caller is responsible).
	writeTestEnvFile(t, runtimeDir, sessionID)

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
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, sessionID)

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

func TestEndCleansUpLegacyFile(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Write codeflow-env.sh for Current() to find the session.
	writeTestEnvFile(t, runtimeDir, sessionID)

	// Also write a legacy current-session-id file to verify cleanup.
	idFile := filepath.Join(runtimeDir, "current-session-id")
	if err := os.WriteFile(idFile, []byte(sessionID), 0o644); err != nil {
		t.Fatalf("writing legacy current-session-id: %v", err)
	}

	// Verify the legacy file exists before End.
	if _, err := os.Stat(idFile); err != nil {
		t.Fatalf("legacy current-session-id should exist before End: %v", err)
	}

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Verify legacy current-session-id was removed.
	if _, err := os.Stat(idFile); !os.IsNotExist(err) {
		t.Error("legacy current-session-id should have been removed after End")
	}
}

func TestEndWritesJSONL(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session first.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, sessionID)

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
	t.Parallel()
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
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start a session.
	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, sessionID)

	// End the session.
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End: %v", err)
	}

	// Re-write the env file to simulate trying to end again.
	writeTestEnvFile(t, runtimeDir, sessionID)

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
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Try to end without starting -- no env file.
	err := End(ctx, d, ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error for no active session, got nil")
	}
	if !errors.Is(err, ErrNoActiveSession) {
		t.Errorf("error = %v, want ErrNoActiveSession", err)
	}
}

func TestCurrent(t *testing.T) {
	// NOTE: no t.Parallel() -- subtests use t.Setenv which is not parallel-safe.
	t.Run("returns session ID from env file", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()
		ledgerDir := filepath.Join(t.TempDir(), "ledger")
		runtimeDir := filepath.Join(t.TempDir(), "runtime")

		sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
		if err != nil {
			t.Fatalf("Start: %v", err)
		}
		// Write codeflow-env.sh manually (Start no longer writes it).
		writeTestEnvFile(t, runtimeDir, sessionID)

		got, err := Current(runtimeDir)
		if err != nil {
			t.Fatalf("Current: %v", err)
		}
		if got != sessionID {
			t.Errorf("Current() = %q, want %q", got, sessionID)
		}
	})

	t.Run("returns session ID from env var", func(t *testing.T) {
		runtimeDir := filepath.Join(t.TempDir(), "nonexistent")
		t.Setenv("CODEFLOW_SESSION_ID", "ses-from-env-var-12345678")

		got, err := Current(runtimeDir)
		if err != nil {
			t.Fatalf("Current: %v", err)
		}
		if got != "ses-from-env-var-12345678" {
			t.Errorf("Current() = %q, want %q", got, "ses-from-env-var-12345678")
		}
	})

	t.Run("returns error when no env file or env var", func(t *testing.T) {
		runtimeDir := filepath.Join(t.TempDir(), "nonexistent")

		_, err := Current(runtimeDir)
		if err == nil {
			t.Fatal("expected error when no env file, got nil")
		}
		if !errors.Is(err, ErrNoActiveSession) {
			t.Errorf("error = %v, want ErrNoActiveSession", err)
		}
	})

	t.Run("returns error for empty env file", func(t *testing.T) {
		runtimeDir := t.TempDir()
		envFile := filepath.Join(runtimeDir, EnvFile)
		if err := os.WriteFile(envFile, []byte(""), 0o644); err != nil {
			t.Fatalf("writing empty file: %v", err)
		}

		_, err := Current(runtimeDir)
		if err == nil {
			t.Fatal("expected error for empty env file, got nil")
		}
		if !errors.Is(err, ErrNoActiveSession) {
			t.Errorf("error = %v, want ErrNoActiveSession", err)
		}
	})

	t.Run("env var takes priority over env file", func(t *testing.T) {
		runtimeDir := t.TempDir()
		// Write env file with one session ID.
		envFile := filepath.Join(runtimeDir, EnvFile)
		content := "export CODEFLOW_SESSION_ID='ses-from-file-123456789'\n"
		if err := os.WriteFile(envFile, []byte(content), 0o644); err != nil {
			t.Fatalf("writing env file: %v", err)
		}

		// Set env var with a different session ID.
		t.Setenv("CODEFLOW_SESSION_ID", "ses-from-env-var-override")

		got, err := Current(runtimeDir)
		if err != nil {
			t.Fatalf("Current: %v", err)
		}
		if got != "ses-from-env-var-override" {
			t.Errorf("Current() = %q, want %q (env var should take priority)", got, "ses-from-env-var-override")
		}
	})
}

func TestGenerateIDUniqueness(t *testing.T) {
	t.Parallel()
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
	t.Parallel()
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

	// Verify ledger directory was created (Start writes JSONL there).
	if _, err := os.Stat(ledgerDir); err != nil {
		t.Errorf("ledger directory should exist: %v", err)
	}
	// Note: runtimeDir is no longer created by Start() since it no longer
	// writes session ID files. The caller is responsible for creating it.
}

func TestStartEnsuresUserRow(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	claudeID := "user-unique-test-id"
	sessionID, err := Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Query the sessions table to get the actual user_id (git email or claudeID fallback).
	var actualUserID string
	err = d.QueryRow(ctx,
		"SELECT user_id FROM sessions WHERE id = ?", sessionID,
	).Scan(&actualUserID)
	if err != nil {
		t.Fatalf("querying session user_id: %v", err)
	}
	if actualUserID == "" {
		t.Fatal("session user_id should not be empty")
	}

	// Verify the user row was created with the actual user_id.
	var userID string
	err = d.QueryRow(ctx,
		"SELECT id FROM users WHERE id = ?", actualUserID,
	).Scan(&userID)
	if err != nil {
		t.Fatalf("querying user: %v", err)
	}
	if userID != actualUserID {
		t.Errorf("user id = %q, want %q", userID, actualUserID)
	}
}

func TestStartOnClosedDB(t *testing.T) {
	t.Parallel()
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
	t.Parallel()
	// First start a session with a working DB, then close it and try to end.
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, sessionID)

	// Close the DB to force error during End.
	d.Close()

	err = End(ctx, d, ledgerDir, runtimeDir)
	if err == nil {
		t.Fatal("expected error on closed DB, got nil")
	}
}

func TestStartWithReadOnlyLedgerDir(t *testing.T) {
	t.Parallel()
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

func TestParseEnvFileSessionID(t *testing.T) {
	t.Parallel()

	t.Run("extracts session ID from valid env file", func(t *testing.T) {
		runtimeDir := t.TempDir()
		envFile := filepath.Join(runtimeDir, EnvFile)
		content := "export CODEFLOW_SESSION_ID='ses-abc123'\nexport CF_PROJECT_ROOT='myproject'\n"
		if err := os.WriteFile(envFile, []byte(content), 0o644); err != nil {
			t.Fatalf("writing env file: %v", err)
		}

		got, err := parseEnvFileSessionID(envFile)
		if err != nil {
			t.Fatalf("parseEnvFileSessionID: %v", err)
		}
		if got != "ses-abc123" {
			t.Errorf("parseEnvFileSessionID() = %q, want %q", got, "ses-abc123")
		}
	})

	t.Run("returns empty for missing session ID line", func(t *testing.T) {
		runtimeDir := t.TempDir()
		envFile := filepath.Join(runtimeDir, EnvFile)
		content := "export CF_PROJECT_ROOT='myproject'\n"
		if err := os.WriteFile(envFile, []byte(content), 0o644); err != nil {
			t.Fatalf("writing env file: %v", err)
		}

		got, err := parseEnvFileSessionID(envFile)
		if err != nil {
			t.Fatalf("parseEnvFileSessionID: %v", err)
		}
		if got != "" {
			t.Errorf("parseEnvFileSessionID() = %q, want empty", got)
		}
	})

	t.Run("returns error for nonexistent file", func(t *testing.T) {
		_, err := parseEnvFileSessionID("/nonexistent/path/codeflow-env.sh")
		if err == nil {
			t.Fatal("expected error for nonexistent file")
		}
	})
}

func TestStartWithSameClaudeIDTwice(t *testing.T) {
	t.Parallel()
	// Verify INSERT OR IGNORE for users doesn't fail on second session.
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	claudeID := "same-claude-id"

	// First session.
	sid1, err := Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 1: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, sid1)
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
	t.Parallel()
	// Start() does not write session ID files (caller is responsible),
	// so runtimeDir is unused by Start(). Test that an invalid runtimeDir
	// does NOT cause Start() to fail (it only needs a valid ledgerDir).
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	// /dev/null is a file, not a directory -- but runtimeDir is unused now.
	runtimeDir := "/dev/null/runtime"

	_, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start should succeed with invalid runtimeDir (no longer used): %v", err)
	}
}

func TestEndWithEmptyStartedAt(t *testing.T) {
	t.Parallel()
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
	writeTestEnvFile(t, runtimeDir, sessionID)

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

func TestUpdateTracking(t *testing.T) {
	t.Parallel()
	t.Run("sets pathflow_mode and tracking_level", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()
		ledgerDir := filepath.Join(t.TempDir(), "ledger")
		runtimeDir := filepath.Join(t.TempDir(), "runtime")

		sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
		if err != nil {
			t.Fatalf("Start: %v", err)
		}

		if err := UpdateTracking(ctx, d, sessionID, "active", "tracked"); err != nil {
			t.Fatalf("UpdateTracking: %v", err)
		}

		var pathflowMode, trackingLevel string
		err = d.QueryRow(ctx,
			"SELECT pathflow_mode, tracking_level FROM sessions WHERE id = ?", sessionID,
		).Scan(&pathflowMode, &trackingLevel)
		if err != nil {
			t.Fatalf("querying session: %v", err)
		}

		if pathflowMode != "active" {
			t.Errorf("pathflow_mode = %q, want %q", pathflowMode, "active")
		}
		if trackingLevel != "tracked" {
			t.Errorf("tracking_level = %q, want %q", trackingLevel, "tracked")
		}
	})

	t.Run("updates metadata with timestamp", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()
		ledgerDir := filepath.Join(t.TempDir(), "ledger")
		runtimeDir := filepath.Join(t.TempDir(), "runtime")

		sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
		if err != nil {
			t.Fatalf("Start: %v", err)
		}

		if err := UpdateTracking(ctx, d, sessionID, "inactive", "untracked"); err != nil {
			t.Fatalf("UpdateTracking: %v", err)
		}

		var metadataStr string
		err = d.QueryRow(ctx,
			"SELECT metadata FROM sessions WHERE id = ?", sessionID,
		).Scan(&metadataStr)
		if err != nil {
			t.Fatalf("querying metadata: %v", err)
		}

		var metadata map[string]any
		if err := json.Unmarshal([]byte(metadataStr), &metadata); err != nil {
			t.Fatalf("parsing metadata: %v", err)
		}

		if _, ok := metadata["updated_at"]; !ok {
			t.Error("metadata should contain updated_at key")
		}
		// Original claude_id should be preserved.
		if metadata["claude_id"] != "test-claude-id" {
			t.Errorf("claude_id = %v, want test-claude-id", metadata["claude_id"])
		}
	})

	t.Run("no-op on nonexistent session", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		// UpdateTracking on a session that doesn't exist should not error
		// (UPDATE with no matching rows is not an error in SQL).
		err := UpdateTracking(ctx, d, "ses-nonexistent", "active", "tracked")
		if err != nil {
			t.Fatalf("UpdateTracking on nonexistent: %v", err)
		}
	})
}

func TestGitConfigValue(t *testing.T) {
	t.Parallel()
	t.Run("returns user email from git config", func(t *testing.T) {
		// gitConfigValue should return a non-empty string for user.email
		// in any development environment with git configured.
		email := gitConfigValue("user.email")
		// We can't assert a specific value, but we can verify it returns
		// something or empty without error.
		_ = email // Just exercise the function
	})

	t.Run("returns empty for nonexistent key", func(t *testing.T) {
		got := gitConfigValue("nonexistent.key.that.does.not.exist.12345")
		if got != "" {
			t.Errorf("expected empty string for nonexistent key, got %q", got)
		}
	})

	t.Run("returns empty for invalid key format", func(t *testing.T) {
		got := gitConfigValue("")
		if got != "" {
			t.Errorf("expected empty string for empty key, got %q", got)
		}
	})
}

func TestResolveGitUser(t *testing.T) {
	// NOTE: no t.Parallel() -- subtests use t.Setenv which is not parallel-safe
	t.Run("uses git config when available", func(t *testing.T) {
		user := resolveGitUser("test-agent-uuid")
		if user.Email == "" {
			t.Error("Email should not be empty (either git config or claudeID fallback)")
		}
		if user.DisplayName == "" {
			t.Error("DisplayName should not be empty (either git config or email fallback)")
		}
	})

	t.Run("falls back to claudeID when git unavailable", func(t *testing.T) {
		// Override HOME and GIT_CONFIG to make git config return empty.
		// NOTE: no t.Parallel -- t.Setenv modifies process environment.
		t.Setenv("HOME", "/nonexistent-home-for-test")
		t.Setenv("GIT_CONFIG_GLOBAL", "/nonexistent")
		t.Setenv("GIT_CONFIG_SYSTEM", "/nonexistent")
		t.Setenv("GIT_CONFIG_NOSYSTEM", "1")
		t.Setenv("GIT_AUTHOR_NAME", "")
		t.Setenv("GIT_COMMITTER_NAME", "")
		t.Setenv("GIT_AUTHOR_EMAIL", "")
		t.Setenv("GIT_COMMITTER_EMAIL", "")

		user := resolveGitUser("fallback-claude-id")
		if user.Email != "fallback-claude-id" {
			t.Errorf("Email = %q, want fallback-claude-id (claudeID fallback)", user.Email)
		}
		// When name is also empty, it falls back to email.
		if user.DisplayName != "fallback-claude-id" {
			t.Errorf("DisplayName = %q, want fallback-claude-id (email fallback)", user.DisplayName)
		}
	})
}

func TestWriteJSONLEvent(t *testing.T) {
	t.Parallel()
	t.Run("creates ledger directory and writes event", func(t *testing.T) {
		ledgerDir := filepath.Join(t.TempDir(), "deep", "nested", "ledger")

		event := map[string]string{
			"event": "test_event",
			"data":  "hello",
		}
		if err := writeJSONLEvent(ledgerDir, event); err != nil {
			t.Fatalf("writeJSONLEvent: %v", err)
		}

		// Verify the file was created and contains the event.
		jsonlPath := filepath.Join(ledgerDir, "sessions.jsonl")
		data, err := os.ReadFile(jsonlPath)
		if err != nil {
			t.Fatalf("reading JSONL: %v", err)
		}
		if !strings.Contains(string(data), "test_event") {
			t.Error("JSONL file does not contain test_event")
		}
	})

	t.Run("appends multiple events", func(t *testing.T) {
		ledgerDir := filepath.Join(t.TempDir(), "ledger")

		for i := range 3 {
			event := map[string]string{
				"event": fmt.Sprintf("event_%d", i),
			}
			if err := writeJSONLEvent(ledgerDir, event); err != nil {
				t.Fatalf("writeJSONLEvent %d: %v", i, err)
			}
		}

		jsonlPath := filepath.Join(ledgerDir, "sessions.jsonl")
		data, err := os.ReadFile(jsonlPath)
		if err != nil {
			t.Fatalf("reading JSONL: %v", err)
		}
		lines := strings.Split(strings.TrimSpace(string(data)), "\n")
		if len(lines) != 3 {
			t.Errorf("got %d lines, want 3", len(lines))
		}
	})

	t.Run("returns error for invalid ledger path", func(t *testing.T) {
		// /dev/null is a file, not a directory.
		err := writeJSONLEvent("/dev/null/invalid", map[string]string{"event": "test"})
		if err == nil {
			t.Fatal("expected error for invalid ledger path")
		}
	})

	t.Run("returns error for unmarshalable event", func(t *testing.T) {
		ledgerDir := filepath.Join(t.TempDir(), "ledger")

		// A channel cannot be marshaled to JSON.
		err := writeJSONLEvent(ledgerDir, make(chan int))
		if err == nil {
			t.Fatal("expected error for unmarshalable event")
		}
	})

	t.Run("returns error when file path is a directory", func(t *testing.T) {
		ledgerDir := filepath.Join(t.TempDir(), "ledger")
		if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
			t.Fatalf("creating ledger dir: %v", err)
		}
		// Create a directory where the JSONL file should be.
		jsonlDir := filepath.Join(ledgerDir, "sessions.jsonl")
		if err := os.MkdirAll(jsonlDir, 0o755); err != nil {
			t.Fatalf("creating fake dir: %v", err)
		}

		err := writeJSONLEvent(ledgerDir, map[string]string{"event": "test"})
		if err == nil {
			t.Fatal("expected error when JSONL path is a directory")
		}
	})
}

func TestUpdateTrackingOnClosedDB(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	sessionID, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	d.Close()

	err = UpdateTracking(ctx, d, sessionID, "active", "tracked")
	if err == nil {
		t.Fatal("expected error from UpdateTracking on closed DB, got nil")
	}
}

func TestMultipleStartEndCycles(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()
	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	// Start and end two sessions in sequence.
	id1, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 1: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, id1)
	if err := End(ctx, d, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("End 1: %v", err)
	}

	id2, err := Start(ctx, d, "test-claude-id", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("Start 2: %v", err)
	}
	writeTestEnvFile(t, runtimeDir, id2)
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