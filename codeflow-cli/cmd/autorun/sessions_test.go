package autorun

import (
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/db"
)

func TestNewSessionsCmd(t *testing.T) {
	t.Parallel()

	cmd := newSessionsCmd()
	if cmd.Use != "sessions" {
		t.Errorf("Use = %q, want %q", cmd.Use, "sessions")
	}
	if cmd.Short == "" {
		t.Error("sessions command should have a short description")
	}
}

func TestSessionsCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newSessionsCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("sessions command should have --db flag")
	}
	if dbFlag.DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", dbFlag.DefValue, defaultDBPath())
	}

	limitFlag := cmd.Flags().Lookup("limit")
	if limitFlag == nil {
		t.Fatal("sessions command should have --limit flag")
	}
	if limitFlag.DefValue != "20" {
		t.Errorf("--limit default = %q, want %q", limitFlag.DefValue, "20")
	}
}

func TestSessionsCmd_RejectsArgs(t *testing.T) {
	t.Parallel()

	cmd := NewCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"sessions", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for sessions with extra args")
	}
}

func TestRunSessions_InvalidDB(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runSessions(&buf, "/nonexistent/dir/test.db", 20)
	if err == nil {
		t.Fatal("expected error for invalid DB path")
	}
	if !strings.Contains(err.Error(), "opening database") {
		t.Errorf("error = %v, want it to contain %q", err, "opening database")
	}
}

func TestRunSessions_WithData(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	err := runSessions(&buf, dbPath, 20)
	if err != nil {
		t.Fatalf("runSessions() = %v, want nil", err)
	}

	got := buf.String()

	// Verify header row.
	if !strings.Contains(got, "SESSION") || !strings.Contains(got, "STATUS") {
		t.Errorf("output missing header row:\n%s", got)
	}

	// Both sessions should appear.
	if !strings.Contains(got, "ars-session-001") {
		t.Errorf("output missing session-001:\n%s", got)
	}
	if !strings.Contains(got, "ars-session-002") {
		t.Errorf("output missing session-002:\n%s", got)
	}

	// Verify status values.
	if !strings.Contains(got, "running") {
		t.Errorf("output missing 'running' status:\n%s", got)
	}
	if !strings.Contains(got, "completed") {
		t.Errorf("output missing 'completed' status:\n%s", got)
	}
}

func TestRunSessions_BatchNameDisplay(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	if err := runSessions(&buf, dbPath, 20); err != nil {
		t.Fatalf("runSessions() = %v", err)
	}

	got := buf.String()
	// Session-001 has batch_name="integration-tests", should display it.
	if !strings.Contains(got, "integration-tests") {
		t.Errorf("output should show batch_name 'integration-tests':\n%s", got)
	}
	// Session-002 has NULL batch_name, should display batch_file path.
	if !strings.Contains(got, "/path/to/batch2.ya") {
		t.Errorf("output should show truncated batch_file for session-002:\n%s", got)
	}
}

func TestRunSessions_EmptyDB(t *testing.T) {
	t.Parallel()

	// Create a DB with schema but no session data.
	dbPath := setupTestDB(t)
	// Remove all session data.
	d, err := db.NewDB(dbPath)
	if err != nil {
		t.Fatalf("reopening db: %v", err)
	}
	defer d.Close()
	ctx := t.Context()
	if _, err := d.Execute(ctx, "DELETE FROM autorun_workers"); err != nil {
		t.Fatalf("deleting workers: %v", err)
	}
	if _, err := d.Execute(ctx, "DELETE FROM autorun_sessions"); err != nil {
		t.Fatalf("deleting sessions: %v", err)
	}

	var buf strings.Builder
	if err := runSessions(&buf, dbPath, 20); err != nil {
		t.Fatalf("runSessions() = %v", err)
	}
	got := buf.String()
	if !strings.Contains(got, "No autorun sessions found") {
		t.Errorf("expected 'No autorun sessions found', got:\n%s", got)
	}
}

func TestRunSessions_Limit(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	// Limit to 1 — should only show the newest session.
	if err := runSessions(&buf, dbPath, 1); err != nil {
		t.Fatalf("runSessions() = %v", err)
	}

	got := buf.String()
	// Session-002 is newer (created 2025-06-02), should appear.
	if !strings.Contains(got, "ars-session-002") {
		t.Errorf("with limit=1, expected newest session:\n%s", got)
	}
	// Session-001 should NOT appear with limit=1.
	if strings.Contains(got, "ars-session-001") {
		t.Errorf("with limit=1, should not show older session:\n%s", got)
	}
}

func TestRunSessions_BatchNameTruncation(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)

	// Insert a session with a very long batch name (>20 chars).
	d, err := db.NewDB(dbPath)
	if err != nil {
		t.Fatalf("reopening db: %v", err)
	}
	defer d.Close()
	ctx := t.Context()
	if _, err := d.Execute(ctx,
		`INSERT INTO autorun_sessions (id, batch_file, batch_name, status, max_session_workers, total_tasks, completed_tasks, failed_tasks, created_at)
		 VALUES ('ars-session-003', '/batch.yaml', 'very-long-batch-name-that-exceeds-twenty', 'running', 1, 1, 0, 0, '2025-06-03T10:00:00Z')`,
	); err != nil {
		t.Fatalf("inserting session: %v", err)
	}

	var buf strings.Builder
	if err := runSessions(&buf, dbPath, 20); err != nil {
		t.Fatalf("runSessions() = %v", err)
	}

	got := buf.String()
	// The long name should be truncated to 17 chars + "..."
	if !strings.Contains(got, "very-long-batch-n...") {
		t.Errorf("expected truncated batch name, got:\n%s", got)
	}
}
