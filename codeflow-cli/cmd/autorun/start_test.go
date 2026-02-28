package autorun

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewCmd(t *testing.T) {
	t.Parallel()

	cmd := NewCmd()
	if cmd.Use != "autorun" {
		t.Errorf("NewCmd().Use = %q, want %q", cmd.Use, "autorun")
	}
	if cmd.Short == "" {
		t.Error("autorun command should have a short description")
	}

	// Verify subcommands are registered.
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}
	for _, name := range []string{"start", "status", "sessions"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on autorun cmd", name)
		}
	}
}

func TestNewCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := NewCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("autorun --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"autorun", "start", "status", "sessions"} {
		if !strings.Contains(got, want) {
			t.Errorf("autorun help missing %q:\n%s", want, got)
		}
	}
}

func TestNewStartCmd(t *testing.T) {
	t.Parallel()

	cmd := newStartCmd()
	if cmd.Use != "start <batch-file>" {
		t.Errorf("Use = %q, want %q", cmd.Use, "start <batch-file>")
	}
	if cmd.Short == "" {
		t.Error("start command should have a short description")
	}
}

func TestStartCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newStartCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("start command should have --db flag")
	}
	if dbFlag.DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", dbFlag.DefValue, defaultDBPath())
	}

	maxWorkersFlag := cmd.Flags().Lookup("max-workers")
	if maxWorkersFlag == nil {
		t.Fatal("start command should have --max-workers flag")
	}
	if maxWorkersFlag.DefValue != "0" {
		t.Errorf("--max-workers default = %q, want %q", maxWorkersFlag.DefValue, "0")
	}
}

func TestStartCmd_RequiresExactlyOneArg(t *testing.T) {
	t.Parallel()

	cmd := NewCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"start"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for start with no args")
	}
}

func TestStartCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := NewCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"start", "file1.yaml", "file2.yaml"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for start with extra args")
	}
}

func TestDefaultDBPath(t *testing.T) {
	t.Parallel()

	got := defaultDBPath()
	want := filepath.Join(".state", "db", "codeflow.db")
	if got != want {
		t.Errorf("defaultDBPath() = %q, want %q", got, want)
	}
}

func TestRunStart_InvalidBatchFile(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runStart(&buf, "nonexistent.db", "/nonexistent/batch.yaml", 0)
	if err == nil {
		t.Fatal("expected error for nonexistent batch file")
	}
	if !strings.Contains(err.Error(), "parsing batch file") {
		t.Errorf("error = %v, want it to contain %q", err, "parsing batch file")
	}
}

func TestRunStart_InvalidDBPath(t *testing.T) {
	t.Parallel()

	// Create a valid batch file in a temp dir.
	tmpDir := t.TempDir()
	batchPath := filepath.Join(tmpDir, "batch.yaml")
	batchContent := `name: test
max_workers: 1
tasks:
  - id: task-001
    prompt: "test"
`
	if err := os.WriteFile(batchPath, []byte(batchContent), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	var buf strings.Builder
	err := runStart(&buf, "/nonexistent/dir/test.db", batchPath, 0)
	if err == nil {
		t.Fatal("expected error for invalid DB path")
	}
	if !strings.Contains(err.Error(), "opening database") {
		t.Errorf("error = %v, want it to contain %q", err, "opening database")
	}
}

func TestRunStart_SchemaInitAndStartFailure(t *testing.T) {
	t.Parallel()

	// Create a valid batch file with tasks that do NOT exist in the DB.
	// This exercises: ParseBatchFile (success), NewDB (success),
	// InitFromSchema (success), then Start fails on FK constraint.
	tmpDir := t.TempDir()
	batchPath := filepath.Join(tmpDir, "batch.yaml")
	batchContent := `name: integration-test
max_workers: 2
tasks:
  - id: task-nonexistent-001
    prompt: "do something"
`
	if err := os.WriteFile(batchPath, []byte(batchContent), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	dbPath := filepath.Join(tmpDir, "test.db")
	var buf strings.Builder
	err := runStart(&buf, dbPath, batchPath, 0)
	if err == nil {
		t.Fatal("expected error from Start() due to FK constraint on nonexistent task")
	}
	if !strings.Contains(err.Error(), "starting autorun") {
		t.Errorf("error = %v, want it to contain 'starting autorun'", err)
	}
}

func TestRunStart_MaxWorkersOverride(t *testing.T) {
	t.Parallel()

	// Same as above but with maxWorkersOverride > 0.
	// This exercises the maxWorkersOverride branch (lines 69-71 of start.go).
	tmpDir := t.TempDir()
	batchPath := filepath.Join(tmpDir, "batch.yaml")
	batchContent := `name: override-test
max_workers: 1
tasks:
  - id: task-nonexistent-002
    prompt: "test override"
`
	if err := os.WriteFile(batchPath, []byte(batchContent), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	dbPath := filepath.Join(tmpDir, "test.db")
	var buf strings.Builder
	// maxWorkersOverride=5 should override batch's max_workers=1
	err := runStart(&buf, dbPath, batchPath, 5)
	if err == nil {
		t.Fatal("expected error from Start() due to FK constraint")
	}
	// The error is from Start(), meaning we successfully passed through:
	// ParseBatchFile, maxWorkers override, NewDB, InitFromSchema.
	if !strings.Contains(err.Error(), "starting autorun") {
		t.Errorf("error = %v, want it to contain 'starting autorun'", err)
	}
}

func TestRunStart_SuccessPath(t *testing.T) {
	t.Parallel()

	// Create a DB with schema and valid tasks so Start() succeeds.
	// Start() inserts autorun_sessions and autorun_workers (FK to tasks),
	// then launches a background goroutine. The goroutine will fail on
	// tmux operations but runStart returns before that happens.
	dbPath := setupTestDB(t)

	tmpDir := t.TempDir()
	batchPath := filepath.Join(tmpDir, "batch.yaml")
	// task-001 and task-002 exist in the DB via setupTestDB.
	batchContent := `name: success-test
max_workers: 2
tasks:
  - id: task-001
    prompt: "first task"
  - id: task-002
    prompt: "second task"
`
	if err := os.WriteFile(batchPath, []byte(batchContent), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	var buf strings.Builder
	err := runStart(&buf, dbPath, batchPath, 0)
	if err != nil {
		t.Fatalf("runStart() = %v, want nil", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Autorun session started:") {
		t.Errorf("output missing session started message:\n%s", got)
	}
	if !strings.Contains(got, "success-test") {
		t.Errorf("output missing batch name:\n%s", got)
	}
	if !strings.Contains(got, "Tasks: 2") {
		t.Errorf("output missing task count:\n%s", got)
	}
	if !strings.Contains(got, "Max workers: 2") {
		t.Errorf("output missing max workers:\n%s", got)
	}
}

func TestRunStart_SuccessWithMaxWorkersOverride(t *testing.T) {
	t.Parallel()

	// Test success path WITH maxWorkersOverride to cover both branches.
	dbPath := setupTestDB(t)

	tmpDir := t.TempDir()
	batchPath := filepath.Join(tmpDir, "batch.yaml")
	batchContent := `name: override-success
max_workers: 1
tasks:
  - id: task-001
    prompt: "task"
`
	if err := os.WriteFile(batchPath, []byte(batchContent), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	var buf strings.Builder
	err := runStart(&buf, dbPath, batchPath, 5)
	if err != nil {
		t.Fatalf("runStart() with override = %v, want nil", err)
	}

	got := buf.String()
	// maxWorkersOverride=5 should override batch's max_workers=1.
	if !strings.Contains(got, "Max workers: 5") {
		t.Errorf("output should show overridden max workers (5):\n%s", got)
	}
}

