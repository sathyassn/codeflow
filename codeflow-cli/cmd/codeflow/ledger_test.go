package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewLedgerCmd(t *testing.T) {
	t.Parallel()

	cmd := newLedgerCmd()
	if cmd.Use != "ledger" {
		t.Errorf("Use = %q, want %q", cmd.Use, "ledger")
	}
	if cmd.Short == "" {
		t.Error("ledger command should have a short description")
	}
}

func TestLedgerCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newLedgerCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	for _, name := range []string{"append", "validate"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on ledger cmd", name)
		}
	}

	if len(subcommands) != 2 {
		t.Errorf("len(subcommands) = %d, want 2", len(subcommands))
	}
}

func TestLedgerCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["ledger"] {
		t.Error("expected 'ledger' subcommand to be registered in root cmd")
	}
}

func TestLedgerAppendCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newLedgerAppendCmd()

	ledgerFlag := cmd.Flags().Lookup("ledger")
	if ledgerFlag == nil {
		t.Fatal("ledger append should have --ledger flag")
	}
	if ledgerFlag.DefValue != defaultLedgerDir() {
		t.Errorf("--ledger default = %q, want %q", ledgerFlag.DefValue, defaultLedgerDir())
	}

	eventFlag := cmd.Flags().Lookup("event")
	if eventFlag == nil {
		t.Fatal("ledger append should have --event flag")
	}
}

func TestLedgerCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"ledger", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("ledger --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"append", "validate"} {
		if !strings.Contains(got, want) {
			t.Errorf("ledger help missing %q:\n%s", want, got)
		}
	}
}

// --- runLedgerAppend tests ---

func TestRunLedgerAppend_HappyPath(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	eventJSON := `{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-test"}`
	var buf strings.Builder
	err := runLedgerAppend(&buf, strings.NewReader(""), ledgerDir, eventJSON)
	if err != nil {
		t.Fatalf("runLedgerAppend: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Event appended successfully") {
		t.Errorf("output missing success message: %q", got)
	}

	// Verify file was created with content.
	data, err := os.ReadFile(filepath.Join(ledgerDir, "sessions.jsonl"))
	if err != nil {
		t.Fatalf("reading sessions.jsonl: %v", err)
	}
	if !strings.Contains(string(data), "session_start") {
		t.Errorf("sessions.jsonl should contain session_start event")
	}
}

func TestRunLedgerAppend_ViaStdin(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	stdin := strings.NewReader(`{"event":"config_set","timestamp":"2026-02-28T14:00:00Z"}`)
	var buf strings.Builder
	err := runLedgerAppend(&buf, stdin, ledgerDir, "")
	if err != nil {
		t.Fatalf("runLedgerAppend via stdin: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Event appended successfully") {
		t.Errorf("output missing success message: %q", got)
	}

	// Verify written to config.jsonl.
	data, err := os.ReadFile(filepath.Join(ledgerDir, "config.jsonl"))
	if err != nil {
		t.Fatalf("reading config.jsonl: %v", err)
	}
	if !strings.Contains(string(data), "config_set") {
		t.Errorf("config.jsonl should contain config_set event")
	}
}

func TestRunLedgerAppend_EmptyInput(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	var buf strings.Builder
	err := runLedgerAppend(&buf, strings.NewReader(""), ledgerDir, "")
	if err == nil {
		t.Fatal("expected error for empty input")
	}
	if !strings.Contains(err.Error(), "no event data provided") {
		t.Errorf("error should mention no data: %v", err)
	}
	if exitCode(err) != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", exitCode(err), ExitConfigError)
	}
}

func TestRunLedgerAppend_InvalidJSON(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	var buf strings.Builder
	err := runLedgerAppend(&buf, strings.NewReader(""), ledgerDir, "not-json")
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}
	if !strings.Contains(err.Error(), "parsing event JSON") {
		t.Errorf("error should mention parsing: %v", err)
	}
	if exitCode(err) != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", exitCode(err), ExitConfigError)
	}
}

func TestRunLedgerAppend_SchemaRejection(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	// session_start requires session_id.
	eventJSON := `{"event":"session_start","timestamp":"2026-02-28T14:00:00Z"}`
	var buf strings.Builder
	err := runLedgerAppend(&buf, strings.NewReader(""), ledgerDir, eventJSON)
	if err == nil {
		t.Fatal("expected schema validation error")
	}
	if !strings.Contains(err.Error(), "missing required fields") {
		t.Errorf("error should mention missing fields: %v", err)
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
}

func TestRunLedgerAppend_UnknownEventType(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	eventJSON := `{"event":"totally_fake_event","timestamp":"2026-02-28T14:00:00Z"}`
	var buf strings.Builder
	err := runLedgerAppend(&buf, strings.NewReader(""), ledgerDir, eventJSON)
	if err == nil {
		t.Fatal("expected error for unknown event type")
	}
	if !strings.Contains(err.Error(), "unknown event type") {
		t.Errorf("error should mention unknown event type: %v", err)
	}
}

func TestRunLedgerAppend_AllFourFiles(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	events := []struct {
		json string
		file string
	}{
		{`{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}`, "sessions.jsonl"},
		{`{"event":"task_created","timestamp":"2026-02-28T14:00:01Z","id":"t-1","epic_id":"e-1","title":"T1"}`, "work-graph.jsonl"},
		{`{"event":"memory_store","timestamp":"2026-02-28T14:00:02Z","id":"m-1"}`, "memory-events.jsonl"},
		{`{"event":"config_set","timestamp":"2026-02-28T14:00:03Z"}`, "config.jsonl"},
	}

	for _, tt := range events {
		var buf strings.Builder
		if err := runLedgerAppend(&buf, strings.NewReader(""), ledgerDir, tt.json); err != nil {
			t.Fatalf("runLedgerAppend(%s): %v", tt.file, err)
		}
	}

	// Verify all 4 files exist.
	for _, tt := range events {
		if _, err := os.Stat(filepath.Join(ledgerDir, tt.file)); err != nil {
			t.Errorf("expected %s to exist: %v", tt.file, err)
		}
	}
}

func TestRunLedgerAppend_ViaRootCmd(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"ledger", "append",
		"--ledger", ledgerDir,
		"--event", `{"event":"config_set","timestamp":"2026-02-28T14:00:00Z"}`,
	})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("ledger append via root cmd: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Event appended successfully") {
		t.Errorf("output missing success message: %q", got)
	}
}

func TestRunLedgerAppend_ViaRootCmd_Stdin(t *testing.T) {
	t.Parallel()

	ledgerDir := filepath.Join(t.TempDir(), "ledger")

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetIn(strings.NewReader(`{"event":"config_updated","timestamp":"2026-02-28T14:00:00Z"}`))
	cmd.SetArgs([]string{"ledger", "append", "--ledger", ledgerDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("ledger append via root cmd (stdin): %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Event appended successfully") {
		t.Errorf("output missing success message: %q", got)
	}
}

// --- runLedgerValidate tests ---

func TestRunLedgerValidate_ValidFile(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	content := `{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}
{"event":"session_end","timestamp":"2026-02-28T14:01:00Z","session_id":"ses-1"}
`
	if err := os.WriteFile(tmpFile, []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runLedgerValidate(&buf, tmpFile)
	if err != nil {
		t.Fatalf("runLedgerValidate: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Validated 2 events") {
		t.Errorf("output should mention 2 events: %q", got)
	}
	if !strings.Contains(got, "0 errors") {
		t.Errorf("output should mention 0 errors: %q", got)
	}
}

func TestRunLedgerValidate_InvalidJSON(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	content := `{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}
not-valid-json
{"event":"config_set","timestamp":"2026-02-28T14:00:00Z"}
`
	if err := os.WriteFile(tmpFile, []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runLedgerValidate(&buf, tmpFile)
	if err == nil {
		t.Fatal("expected error for file with invalid JSON")
	}

	got := buf.String()
	if !strings.Contains(got, "Validated 2 events") {
		t.Errorf("output should mention 2 valid events: %q", got)
	}
	if !strings.Contains(got, "1 errors") {
		t.Errorf("output should mention 1 error: %q", got)
	}
	if !strings.Contains(got, "line 2") {
		t.Errorf("output should mention line 2: %q", got)
	}
}

func TestRunLedgerValidate_MissingEventField(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	content := `{"timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}
`
	if err := os.WriteFile(tmpFile, []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runLedgerValidate(&buf, tmpFile)
	if err == nil {
		t.Fatal("expected error for missing event field")
	}

	got := buf.String()
	if !strings.Contains(got, "missing 'event' field") {
		t.Errorf("output should mention missing event field: %q", got)
	}
}

func TestRunLedgerValidate_EmptyFile(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	if err := os.WriteFile(tmpFile, []byte(""), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runLedgerValidate(&buf, tmpFile)
	if err != nil {
		t.Fatalf("runLedgerValidate on empty file: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Validated 0 events") {
		t.Errorf("output should mention 0 events: %q", got)
	}
	if !strings.Contains(got, "0 errors") {
		t.Errorf("output should mention 0 errors: %q", got)
	}
}

func TestRunLedgerValidate_NonexistentFile(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runLedgerValidate(&buf, "/nonexistent/path/test.jsonl")
	if err == nil {
		t.Fatal("expected error for nonexistent file")
	}
	if !strings.Contains(err.Error(), "reading file") {
		t.Errorf("error should mention reading file: %v", err)
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
}

func TestRunLedgerValidate_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	content := `{"event":"config_set","timestamp":"2026-02-28T14:00:00Z"}
`
	if err := os.WriteFile(tmpFile, []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"ledger", "validate", tmpFile})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("ledger validate via root cmd: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Validated 1 events") {
		t.Errorf("output missing event count: %q", got)
	}
}

func TestLedgerValidateCmd_RequiresExactlyOneArg(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"ledger", "validate"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for ledger validate with no args")
	}
}

func TestRunLedgerValidate_MultipleErrors(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	content := `not-json-1
{"no_event_field": true}
not-json-2
{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}
`
	if err := os.WriteFile(tmpFile, []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runLedgerValidate(&buf, tmpFile)
	if err == nil {
		t.Fatal("expected error for multiple validation errors")
	}

	got := buf.String()
	if !strings.Contains(got, "Validated 1 events") {
		t.Errorf("output should mention 1 valid event: %q", got)
	}
	if !strings.Contains(got, "3 errors") {
		t.Errorf("output should mention 3 errors: %q", got)
	}
	if !strings.Contains(err.Error(), "3 validation errors found") {
		t.Errorf("error should mention 3 validation errors: %v", err)
	}
}

// --- splitLines tests ---

func TestSplitLines_UnixNewlines(t *testing.T) {
	t.Parallel()

	lines := splitLines([]byte("line1\nline2\nline3\n"))
	if len(lines) != 3 {
		t.Fatalf("expected 3 lines, got %d", len(lines))
	}
	if string(lines[0]) != "line1" {
		t.Errorf("lines[0] = %q, want %q", lines[0], "line1")
	}
	if string(lines[1]) != "line2" {
		t.Errorf("lines[1] = %q, want %q", lines[1], "line2")
	}
	if string(lines[2]) != "line3" {
		t.Errorf("lines[2] = %q, want %q", lines[2], "line3")
	}
}

func TestSplitLines_WindowsNewlines(t *testing.T) {
	t.Parallel()

	lines := splitLines([]byte("line1\r\nline2\r\n"))
	if len(lines) != 2 {
		t.Fatalf("expected 2 lines, got %d", len(lines))
	}
	if string(lines[0]) != "line1" {
		t.Errorf("lines[0] = %q, want %q", lines[0], "line1")
	}
	if string(lines[1]) != "line2" {
		t.Errorf("lines[1] = %q, want %q", lines[1], "line2")
	}
}

func TestSplitLines_TrailingWithoutNewline(t *testing.T) {
	t.Parallel()

	lines := splitLines([]byte("line1\nline2"))
	if len(lines) != 2 {
		t.Fatalf("expected 2 lines, got %d", len(lines))
	}
	if string(lines[0]) != "line1" {
		t.Errorf("lines[0] = %q, want %q", lines[0], "line1")
	}
	if string(lines[1]) != "line2" {
		t.Errorf("lines[1] = %q, want %q", lines[1], "line2")
	}
}

func TestSplitLines_EmptyInput(t *testing.T) {
	t.Parallel()

	lines := splitLines([]byte(""))
	if len(lines) != 0 {
		t.Fatalf("expected 0 lines, got %d", len(lines))
	}
}

func TestSplitLines_SingleNewline(t *testing.T) {
	t.Parallel()

	lines := splitLines([]byte("\n"))
	if len(lines) != 1 {
		t.Fatalf("expected 1 line (empty), got %d", len(lines))
	}
	if len(lines[0]) != 0 {
		t.Errorf("lines[0] should be empty, got %q", lines[0])
	}
}

func TestRunLedgerValidate_WithBlankLines(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "test.jsonl")
	content := `{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}

{"event":"config_set","timestamp":"2026-02-28T14:01:00Z"}
`
	if err := os.WriteFile(tmpFile, []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runLedgerValidate(&buf, tmpFile)
	if err != nil {
		t.Fatalf("runLedgerValidate: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Validated 2 events") {
		t.Errorf("blank lines should be skipped, got: %q", got)
	}
}
