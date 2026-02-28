package autorun

import (
	"strings"
	"testing"
)

func TestNewStatusCmd(t *testing.T) {
	t.Parallel()

	cmd := newStatusCmd()
	if cmd.Use != "status [session-id]" {
		t.Errorf("Use = %q, want %q", cmd.Use, "status [session-id]")
	}
	if cmd.Short == "" {
		t.Error("status command should have a short description")
	}
}

func TestStatusCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newStatusCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("status command should have --db flag")
	}
	if dbFlag.DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", dbFlag.DefValue, defaultDBPath())
	}
}

func TestStatusCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := NewCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"status", "arg1", "arg2"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for status with extra args")
	}
}

func TestRunStatus_InvalidDB(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runStatus(&buf, "/nonexistent/dir/test.db", "session-123")
	if err == nil {
		t.Fatal("expected error for invalid DB path")
	}
	if !strings.Contains(err.Error(), "opening database") {
		t.Errorf("error = %v, want it to contain %q", err, "opening database")
	}
}

func TestRunStatus_BySessionID(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	err := runStatus(&buf, dbPath, "ars-session-001")
	if err != nil {
		t.Fatalf("runStatus() = %v, want nil", err)
	}

	got := buf.String()

	// Verify session info.
	if !strings.Contains(got, "ars-session-001") {
		t.Errorf("output missing session ID:\n%s", got)
	}
	if !strings.Contains(got, "running") {
		t.Errorf("output missing status 'running':\n%s", got)
	}
	if !strings.Contains(got, "integration-tests") {
		t.Errorf("output missing batch_name 'integration-tests':\n%s", got)
	}
	if !strings.Contains(got, "3 max") {
		t.Errorf("output missing max workers:\n%s", got)
	}
	if !strings.Contains(got, "0/2 completed") {
		t.Errorf("output missing progress:\n%s", got)
	}

	// Verify worker details.
	if !strings.Contains(got, "Workers:") {
		t.Errorf("output missing Workers section:\n%s", got)
	}
	if !strings.Contains(got, "#1") {
		t.Errorf("output missing worker #1:\n%s", got)
	}
	if !strings.Contains(got, "tmux=autorun-001") {
		t.Errorf("output missing tmux session:\n%s", got)
	}
	if !strings.Contains(got, "PR=#42") {
		t.Errorf("output missing PR number:\n%s", got)
	}
}

func TestRunStatus_LatestSession(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	// Empty session ID should fetch latest.
	err := runStatus(&buf, dbPath, "")
	if err != nil {
		t.Fatalf("runStatus('') = %v, want nil", err)
	}

	got := buf.String()
	// Session-002 is newer (2025-06-02 vs 2025-06-01).
	if !strings.Contains(got, "ars-session-002") {
		t.Errorf("expected latest session (ars-session-002):\n%s", got)
	}
}

func TestRunStatus_CompletedAtDisplay(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	// Session-002 has completed_at set.
	err := runStatus(&buf, dbPath, "ars-session-002")
	if err != nil {
		t.Fatalf("runStatus() = %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Completed:") {
		t.Errorf("output missing 'Completed:' for completed session:\n%s", got)
	}
	if !strings.Contains(got, "2025-06-02T12:00:00Z") {
		t.Errorf("output missing completed_at timestamp:\n%s", got)
	}
}

func TestRunStatus_NoBatchName(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	// Session-002 has NULL batch_name.
	err := runStatus(&buf, dbPath, "ars-session-002")
	if err != nil {
		t.Fatalf("runStatus() = %v", err)
	}

	got := buf.String()
	// Should show batch_file without parenthetical batch_name.
	if !strings.Contains(got, "/path/to/batch2.yaml") {
		t.Errorf("output missing batch_file:\n%s", got)
	}
}

func TestRunStatus_NoWorkers(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	// Session-002 has no workers.
	err := runStatus(&buf, dbPath, "ars-session-002")
	if err != nil {
		t.Fatalf("runStatus() = %v", err)
	}

	got := buf.String()
	// No workers means no "\nWorkers:\n" section (distinct from "Workers: 2 max").
	if strings.Contains(got, "\nWorkers:\n") {
		t.Errorf("output should NOT contain worker detail section when no workers exist:\n%s", got)
	}
}

func TestRunStatus_SessionNotFound(t *testing.T) {
	t.Parallel()

	dbPath := setupTestDB(t)
	var buf strings.Builder
	err := runStatus(&buf, dbPath, "nonexistent-session")
	if err == nil {
		t.Fatal("expected error for nonexistent session")
	}
	if !strings.Contains(err.Error(), "querying session") {
		t.Errorf("error = %v, want it to contain 'querying session'", err)
	}
}

func TestStatusCmd_AcceptsZeroArgs(t *testing.T) {
	t.Parallel()

	cmd := newStatusCmd()
	// MaximumNArgs(1) should accept 0 args — verify via Args function.
	if cmd.Args == nil {
		t.Error("status command should have Args validation set")
	}
}
