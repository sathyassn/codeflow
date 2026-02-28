package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/workstate"
)

func TestNewStateCmd(t *testing.T) {
	t.Parallel()

	cmd := newStateCmd()
	if cmd.Use != "state" {
		t.Errorf("Use = %q, want %q", cmd.Use, "state")
	}
	if cmd.Short == "" {
		t.Error("state command should have a short description")
	}
}

func TestStateCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newStateCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	for _, name := range []string{"active-task", "memory"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on state cmd", name)
		}
	}

	if len(subcommands) != 2 {
		t.Errorf("len(subcommands) = %d, want 2", len(subcommands))
	}
}

func TestNewStateActiveTaskCmd(t *testing.T) {
	t.Parallel()

	cmd := newStateActiveTaskCmd()
	if cmd.Use != "active-task" {
		t.Errorf("Use = %q, want %q", cmd.Use, "active-task")
	}

	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	for _, name := range []string{"set", "get", "clear"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on active-task cmd", name)
		}
	}

	if len(subcommands) != 3 {
		t.Errorf("len(subcommands) = %d, want 3", len(subcommands))
	}
}

func TestNewStateMemoryCmd(t *testing.T) {
	t.Parallel()

	cmd := newStateMemoryCmd()
	if cmd.Use != "memory" {
		t.Errorf("Use = %q, want %q", cmd.Use, "memory")
	}

	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	if !subcommands["record"] {
		t.Error("expected subcommand 'record' to be registered on memory cmd")
	}
}

func TestStateActiveTaskSetCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newStateActiveTaskSetCmd()

	flags := []struct {
		name     string
		required bool
	}{
		{"runtime", false},
		{"task-id", true},
		{"format-id", true},
		{"epic-id", false},
		{"epic", false},
		{"title", false},
		{"branch", false},
		{"session", false},
		{"status", false},
		{"stage", false},
		{"team", false},
	}

	for _, f := range flags {
		flag := cmd.Flags().Lookup(f.name)
		if flag == nil {
			t.Errorf("set command should have --%s flag", f.name)
		}
	}

	runtimeFlag := cmd.Flags().Lookup("runtime")
	if runtimeFlag != nil && runtimeFlag.DefValue != defaultRuntimeDir() {
		t.Errorf("--runtime default = %q, want %q", runtimeFlag.DefValue, defaultRuntimeDir())
	}
}

func TestStateActiveTaskGetCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newStateActiveTaskGetCmd()

	runtimeFlag := cmd.Flags().Lookup("runtime")
	if runtimeFlag == nil {
		t.Fatal("get command should have --runtime flag")
	}
	if runtimeFlag.DefValue != defaultRuntimeDir() {
		t.Errorf("--runtime default = %q, want %q", runtimeFlag.DefValue, defaultRuntimeDir())
	}
}

func TestStateActiveTaskClearCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newStateActiveTaskClearCmd()

	runtimeFlag := cmd.Flags().Lookup("runtime")
	if runtimeFlag == nil {
		t.Fatal("clear command should have --runtime flag")
	}
	if runtimeFlag.DefValue != defaultRuntimeDir() {
		t.Errorf("--runtime default = %q, want %q", runtimeFlag.DefValue, defaultRuntimeDir())
	}
}

func TestStateMemoryRecordCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newStateMemoryRecordCmd()

	ledgerFlag := cmd.Flags().Lookup("ledger")
	if ledgerFlag == nil {
		t.Fatal("record command should have --ledger flag")
	}
	if ledgerFlag.DefValue != defaultLedgerDir() {
		t.Errorf("--ledger default = %q, want %q", ledgerFlag.DefValue, defaultLedgerDir())
	}

	eventFlag := cmd.Flags().Lookup("event")
	if eventFlag == nil {
		t.Fatal("record command should have --event flag")
	}
}

// --- runStateActiveTaskSet tests ---

func TestRunStateActiveTaskSet_HappyPath(t *testing.T) {
	t.Parallel()

	runtimeDir := filepath.Join(t.TempDir(), "runtime")
	task := workstate.ActiveTask{
		TaskID:   "task-abc123",
		FormatID: "INF-TSK-001-001",
	}

	var buf strings.Builder
	err := runStateActiveTaskSet(&buf, runtimeDir, task)
	if err != nil {
		t.Fatalf("runStateActiveTaskSet: %v", err)
	}

	if !strings.Contains(buf.String(), "Active task set") {
		t.Errorf("output = %q, want 'Active task set'", buf.String())
	}

	// Verify file was written.
	data, err := os.ReadFile(filepath.Join(runtimeDir, workstate.ActiveTaskFile))
	if err != nil {
		t.Fatalf("reading active-task.json: %v", err)
	}
	if !strings.Contains(string(data), "task-abc123") {
		t.Errorf("active-task.json should contain task ID")
	}
}

func TestRunStateActiveTaskSet_Error(t *testing.T) {
	t.Parallel()

	// Use /dev/null as runtime dir -- cannot create directory inside a file.
	var buf strings.Builder
	err := runStateActiveTaskSet(&buf, "/dev/null/impossible", workstate.ActiveTask{TaskID: "x"})
	if err == nil {
		t.Fatal("expected error for invalid runtime dir")
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
	if !strings.Contains(err.Error(), "setting active task") {
		t.Errorf("error should mention setting: %v", err)
	}
}

// --- runStateActiveTaskGet tests ---

func TestRunStateActiveTaskGet_HappyPath(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()
	task := workstate.ActiveTask{
		TaskID:       "task-get-test",
		FormatID:     "INF-TSK-002-001",
		EpicFormatID: "INF-EPC-002",
		Title:        "Test Task",
	}
	if err := workstate.SetActiveTask(runtimeDir, task); err != nil {
		t.Fatalf("setup SetActiveTask: %v", err)
	}

	var buf strings.Builder
	err := runStateActiveTaskGet(&buf, runtimeDir)
	if err != nil {
		t.Fatalf("runStateActiveTaskGet: %v", err)
	}

	// Verify output is valid JSON matching the task.
	var got workstate.ActiveTask
	if err := json.Unmarshal([]byte(buf.String()), &got); err != nil {
		t.Fatalf("output is not valid JSON: %v\nOutput: %s", err, buf.String())
	}
	if got.TaskID != "task-get-test" {
		t.Errorf("TaskID = %q, want %q", got.TaskID, "task-get-test")
	}
	if got.FormatID != "INF-TSK-002-001" {
		t.Errorf("FormatID = %q, want %q", got.FormatID, "INF-TSK-002-001")
	}
	if got.EpicFormatID != "INF-EPC-002" {
		t.Errorf("EpicFormatID = %q, want %q", got.EpicFormatID, "INF-EPC-002")
	}
}

func TestRunStateActiveTaskGet_MissingFile(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	var buf strings.Builder
	err := runStateActiveTaskGet(&buf, runtimeDir)
	if err == nil {
		t.Fatal("expected error for missing active-task.json")
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
	if !strings.Contains(err.Error(), "getting active task") {
		t.Errorf("error should mention getting: %v", err)
	}
}

// --- runStateActiveTaskClear tests ---

func TestRunStateActiveTaskClear_HappyPath(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()
	// Set a task first so we can clear it.
	task := workstate.ActiveTask{TaskID: "task-to-clear", FormatID: "X-TSK-001-001"}
	if err := workstate.SetActiveTask(runtimeDir, task); err != nil {
		t.Fatalf("setup SetActiveTask: %v", err)
	}

	var buf strings.Builder
	err := runStateActiveTaskClear(&buf, runtimeDir)
	if err != nil {
		t.Fatalf("runStateActiveTaskClear: %v", err)
	}

	if !strings.Contains(buf.String(), "Active task cleared") {
		t.Errorf("output = %q, want 'Active task cleared'", buf.String())
	}

	// Verify file was removed.
	if _, err := os.Stat(filepath.Join(runtimeDir, workstate.ActiveTaskFile)); !os.IsNotExist(err) {
		t.Error("active-task.json should be removed after clear")
	}
}

func TestRunStateActiveTaskClear_NonExistent(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	var buf strings.Builder
	err := runStateActiveTaskClear(&buf, runtimeDir)
	if err != nil {
		t.Fatalf("clearing non-existent should succeed: %v", err)
	}

	if !strings.Contains(buf.String(), "Active task cleared") {
		t.Errorf("output = %q, want 'Active task cleared'", buf.String())
	}
}

// --- runStateMemoryRecord tests ---

func TestRunStateMemoryRecord_HappyPath(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	eventJSON := `{"event":"progress","timestamp":"2026-02-28T14:00:00Z","id":"mem-test-001"}`

	var buf strings.Builder
	err := runStateMemoryRecord(&buf, ledgerDir, eventJSON)
	if err != nil {
		t.Fatalf("runStateMemoryRecord: %v", err)
	}

	if !strings.Contains(buf.String(), "Memory event recorded") {
		t.Errorf("output = %q, want 'Memory event recorded'", buf.String())
	}

	// Verify event was written to memory-events.jsonl.
	data, err := os.ReadFile(filepath.Join(ledgerDir, "memory-events.jsonl"))
	if err != nil {
		t.Fatalf("reading memory-events.jsonl: %v", err)
	}
	if !strings.Contains(string(data), "progress") {
		t.Errorf("memory-events.jsonl should contain progress event")
	}
}

func TestRunStateMemoryRecord_InvalidJSON(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	var buf strings.Builder
	err := runStateMemoryRecord(&buf, ledgerDir, "not-valid-json")
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}
	if exitCode(err) != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", exitCode(err), ExitConfigError)
	}
	if !strings.Contains(err.Error(), "parsing event JSON") {
		t.Errorf("error should mention parsing: %v", err)
	}
}

func TestRunStateMemoryRecord_EmptyEventType(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	eventJSON := `{"timestamp":"2026-02-28T14:00:00Z","id":"mem-test-002"}`

	var buf strings.Builder
	err := runStateMemoryRecord(&buf, ledgerDir, eventJSON)
	if err == nil {
		t.Fatal("expected error for empty event type")
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
	if !strings.Contains(err.Error(), "recording memory event") {
		t.Errorf("error should mention recording: %v", err)
	}
}

func TestRunStateMemoryRecord_UnknownEventType(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")
	eventJSON := `{"event":"totally_unknown_event","timestamp":"2026-02-28T14:00:00Z","id":"mem-test-003"}`

	var buf strings.Builder
	err := runStateMemoryRecord(&buf, ledgerDir, eventJSON)
	if err == nil {
		t.Fatal("expected error for unknown event type")
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
}

// --- Via root cmd integration tests ---

func TestStateCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["state"] {
		t.Error("expected 'state' subcommand to be registered in root cmd")
	}
}

func TestStateCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"active-task", "memory"} {
		if !strings.Contains(got, want) {
			t.Errorf("state help missing %q:\n%s", want, got)
		}
	}
}

func TestStateActiveTaskCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "active-task", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state active-task --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"set", "get", "clear"} {
		if !strings.Contains(got, want) {
			t.Errorf("active-task help missing %q:\n%s", want, got)
		}
	}
}

func TestStateActiveTaskSet_ViaRootCmd(t *testing.T) {
	t.Parallel()

	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "active-task", "set",
		"--runtime", runtimeDir,
		"--task-id", "task-via-root",
		"--format-id", "INF-TSK-003-001",
		"--epic", "INF-EPC-003",
		"--title", "Root Test",
		"--branch", "feat/test",
	})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state active-task set via root cmd: %v", err)
	}

	if !strings.Contains(buf.String(), "Active task set") {
		t.Errorf("output = %q, want 'Active task set'", buf.String())
	}

	// Verify file was created.
	data, err := os.ReadFile(filepath.Join(runtimeDir, workstate.ActiveTaskFile))
	if err != nil {
		t.Fatalf("reading active-task.json: %v", err)
	}
	if !strings.Contains(string(data), "task-via-root") {
		t.Errorf("active-task.json should contain task ID from root cmd")
	}
}

func TestStateActiveTaskSet_MissingRequiredFlags(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"state", "active-task", "set", "--runtime", t.TempDir()})

	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for missing required flags")
	}
}

func TestStateActiveTaskGet_ViaRootCmd(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()
	task := workstate.ActiveTask{
		TaskID:   "task-via-root-get",
		FormatID: "INF-TSK-004-001",
	}
	if err := workstate.SetActiveTask(runtimeDir, task); err != nil {
		t.Fatalf("setup SetActiveTask: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "active-task", "get", "--runtime", runtimeDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state active-task get via root cmd: %v", err)
	}

	var got workstate.ActiveTask
	if err := json.Unmarshal([]byte(buf.String()), &got); err != nil {
		t.Fatalf("output is not valid JSON: %v\nOutput: %s", err, buf.String())
	}
	if got.TaskID != "task-via-root-get" {
		t.Errorf("TaskID = %q, want %q", got.TaskID, "task-via-root-get")
	}
}

func TestStateActiveTaskClear_ViaRootCmd(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()
	task := workstate.ActiveTask{TaskID: "task-to-clear-root", FormatID: "X-TSK-001-001"}
	if err := workstate.SetActiveTask(runtimeDir, task); err != nil {
		t.Fatalf("setup SetActiveTask: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "active-task", "clear", "--runtime", runtimeDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state active-task clear via root cmd: %v", err)
	}

	if !strings.Contains(buf.String(), "Active task cleared") {
		t.Errorf("output = %q, want 'Active task cleared'", buf.String())
	}
}

func TestStateMemoryRecord_ViaRootCmd(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "memory", "record",
		"--ledger", ledgerDir,
		"--event", `{"event":"decision","timestamp":"2026-02-28T14:00:00Z","id":"mem-root-001"}`,
	})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state memory record via root cmd: %v", err)
	}

	if !strings.Contains(buf.String(), "Memory event recorded") {
		t.Errorf("output = %q, want 'Memory event recorded'", buf.String())
	}

	// Verify event was written.
	data, err := os.ReadFile(filepath.Join(ledgerDir, "memory-events.jsonl"))
	if err != nil {
		t.Fatalf("reading memory-events.jsonl: %v", err)
	}
	if !strings.Contains(string(data), "decision") {
		t.Errorf("memory-events.jsonl should contain decision event")
	}
}

func TestStateMemoryRecord_MissingRequiredFlag(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"state", "memory", "record", "--ledger", t.TempDir()})

	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for missing --event flag")
	}
}

// --- Set with all optional fields via root cmd ---

func TestStateActiveTaskSet_AllFields_ViaRootCmd(t *testing.T) {
	t.Parallel()

	runtimeDir := filepath.Join(t.TempDir(), "runtime")

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"state", "active-task", "set",
		"--runtime", runtimeDir,
		"--task-id", "task-all-fields",
		"--format-id", "INF-TSK-005-001",
		"--epic-id", "epic-ulid-001",
		"--epic", "INF-EPC-005",
		"--title", "All Fields Test",
		"--branch", "feat/all-fields",
		"--session", "ses-all-fields",
		"--status", "in_progress",
		"--stage", "WS-DEV",
		"--team", "inf-tsk-005",
	})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("state active-task set with all fields: %v", err)
	}

	// Verify all fields were written.
	data, err := os.ReadFile(filepath.Join(runtimeDir, workstate.ActiveTaskFile))
	if err != nil {
		t.Fatalf("reading active-task.json: %v", err)
	}

	var got workstate.ActiveTask
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("unmarshaling active-task.json: %v", err)
	}

	checks := []struct {
		field string
		got   string
		want  string
	}{
		{"TaskID", got.TaskID, "task-all-fields"},
		{"FormatID", got.FormatID, "INF-TSK-005-001"},
		{"EpicID", got.EpicID, "epic-ulid-001"},
		{"EpicFormatID", got.EpicFormatID, "INF-EPC-005"},
		{"Title", got.Title, "All Fields Test"},
		{"Branch", got.Branch, "feat/all-fields"},
		{"SessionID", got.SessionID, "ses-all-fields"},
		{"Status", got.Status, "in_progress"},
		{"CurrentStage", got.CurrentStage, "WS-DEV"},
		{"TeamName", got.TeamName, "inf-tsk-005"},
	}

	for _, c := range checks {
		if c.got != c.want {
			t.Errorf("%s = %q, want %q", c.field, c.got, c.want)
		}
	}
}
