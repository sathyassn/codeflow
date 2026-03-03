package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewNormalizeCmd(t *testing.T) {
	t.Parallel()

	cmd := newNormalizeCmd()
	if cmd.Use != "normalize" {
		t.Errorf("Use = %q, want %q", cmd.Use, "normalize")
	}
	if cmd.Short == "" {
		t.Error("normalize command should have a short description")
	}
}

func TestNormalizeCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newNormalizeCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	if !subcommands["ledger"] {
		t.Error("expected subcommand 'ledger' to be registered on normalize cmd")
	}
	if len(subcommands) != 1 {
		t.Errorf("len(subcommands) = %d, want 1", len(subcommands))
	}
}

func TestNormalizeCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["normalize"] {
		t.Error("expected 'normalize' subcommand to be registered in root cmd")
	}
}

func TestNormalizeLedgerCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newNormalizeLedgerCmd()

	dryRunFlag := cmd.Flags().Lookup("dry-run")
	if dryRunFlag == nil {
		t.Fatal("normalize ledger should have --dry-run flag")
	}
	if dryRunFlag.DefValue != "false" {
		t.Errorf("--dry-run default = %q, want %q", dryRunFlag.DefValue, "false")
	}

	ledgerDirFlag := cmd.Flags().Lookup("ledger-dir")
	if ledgerDirFlag == nil {
		t.Fatal("normalize ledger should have --ledger-dir flag")
	}
	if ledgerDirFlag.DefValue != defaultLedgerDir() {
		t.Errorf("--ledger-dir default = %q, want %q", ledgerDirFlag.DefValue, defaultLedgerDir())
	}
}

func TestNormalizeLedgerCmd_RejectsArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"normalize", "ledger", "extra-arg"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error when passing extra arguments to normalize ledger")
	}
}

func TestNormalizeCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"normalize", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("normalize --help returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "ledger") {
		t.Errorf("normalize help missing 'ledger':\n%s", got)
	}
	if !strings.Contains(got, "canonical schemas") {
		t.Errorf("normalize help missing 'canonical schemas':\n%s", got)
	}
}

func TestNormalizeLedgerCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"normalize", "ledger", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("normalize ledger --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"dry-run", "ledger-dir", "Idempotent"} {
		if !strings.Contains(got, want) {
			t.Errorf("normalize ledger help missing %q:\n%s", want, got)
		}
	}
}

func TestNormalizeCmd_NoSubcommandShowsHelp(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"normalize"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("normalize with no subcommand returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "ledger") {
		t.Errorf("normalize without subcommand should show help with 'ledger': %q", got)
	}
}

// --- runNormalizeLedger tests ---

func TestRunNormalizeLedger_HappyPath(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	// Create a canonical file with a record needing normalization.
	content := `{"type":"memory_stored","ts":"2026-02-28T14:00:00Z","id":"m-1","data":{}}` + "\n"
	if err := os.WriteFile(filepath.Join(ledgerDir, "memory-events.jsonl"), []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runNormalizeLedger(&buf, ledgerDir, false)
	if err != nil {
		t.Fatalf("runNormalizeLedger: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "memory-events.jsonl") {
		t.Errorf("output should mention memory-events.jsonl: %q", got)
	}
	if !strings.Contains(got, "fields renamed") {
		t.Errorf("output should mention fields renamed: %q", got)
	}

	// Verify the file was actually normalized.
	data, err := os.ReadFile(filepath.Join(ledgerDir, "memory-events.jsonl"))
	if err != nil {
		t.Fatalf("reading normalized file: %v", err)
	}
	normalized := string(data)
	if strings.Contains(normalized, `"type"`) {
		t.Error("normalized file should not contain 'type' field")
	}
	if !strings.Contains(normalized, `"event":"memory_store"`) {
		t.Errorf("normalized file should contain event:memory_store: %q", normalized)
	}
	if !strings.Contains(normalized, `"timestamp"`) {
		t.Error("normalized file should contain 'timestamp' field")
	}
}

func TestRunNormalizeLedger_DryRun(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	original := `{"type":"memory_stored","ts":"2026-02-28T14:00:00Z","id":"m-1","data":{}}` + "\n"
	filePath := filepath.Join(ledgerDir, "memory-events.jsonl")
	if err := os.WriteFile(filePath, []byte(original), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runNormalizeLedger(&buf, ledgerDir, true)
	if err != nil {
		t.Fatalf("runNormalizeLedger dry-run: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "DRY RUN") {
		t.Errorf("dry-run output should contain 'DRY RUN': %q", got)
	}

	// Verify file was NOT modified.
	data, err := os.ReadFile(filePath)
	if err != nil {
		t.Fatalf("reading file after dry-run: %v", err)
	}
	if string(data) != original {
		t.Errorf("dry-run should not modify file. got: %q, want: %q", string(data), original)
	}
}

func TestRunNormalizeLedger_EmptyDir(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	var buf strings.Builder
	err := runNormalizeLedger(&buf, ledgerDir, false)
	if err != nil {
		t.Fatalf("runNormalizeLedger empty dir: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "No canonical JSONL files found") {
		t.Errorf("output should mention no files found: %q", got)
	}
}

func TestRunNormalizeLedger_UnreadableFile(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	// Create a directory with the same name as a canonical file to cause an open error.
	if err := os.MkdirAll(filepath.Join(ledgerDir, "memory-events.jsonl"), 0o755); err != nil {
		t.Fatalf("creating directory: %v", err)
	}

	var buf strings.Builder
	err := runNormalizeLedger(&buf, ledgerDir, false)
	if err == nil {
		t.Fatal("expected error when canonical file is a directory")
	}
	if !strings.Contains(err.Error(), "normalization failed") {
		t.Errorf("error should mention normalization failed: %v", err)
	}
	if exitCode(err) != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", exitCode(err), ExitRuntimeError)
	}
}

func TestRunNormalizeLedger_NonexistentDir(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runNormalizeLedger(&buf, "/nonexistent/path/ledger", false)
	if err != nil {
		t.Fatalf("nonexistent dir should not error (files just don't exist): %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "No canonical JSONL files found") {
		t.Errorf("output should mention no files found: %q", got)
	}
}

func TestRunNormalizeLedger_MultipleFiles(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	// Create multiple canonical files.
	files := map[string]string{
		"work-graph.jsonl":     `{"event":"task_created","timestamp":"2026-02-28T14:00:00Z","id":"t-1","epic_id":"e-1","title":"T1"}` + "\n",
		"memory-events.jsonl":  `{"event":"memory_store","timestamp":"2026-02-28T14:00:01Z","id":"m-1","data":{}}` + "\n",
		"sessions.jsonl":       `{"event":"session_start","timestamp":"2026-02-28T14:00:02Z","session_id":"ses-1"}` + "\n",
		"config.jsonl":         `{"event":"config_set","timestamp":"2026-02-28T14:00:03Z"}` + "\n",
	}
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(ledgerDir, name), []byte(content), 0o644); err != nil {
			t.Fatalf("writing %s: %v", name, err)
		}
	}

	var buf strings.Builder
	err := runNormalizeLedger(&buf, ledgerDir, false)
	if err != nil {
		t.Fatalf("runNormalizeLedger multiple files: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Total: 4 files") {
		t.Errorf("output should mention 4 files: %q", got)
	}
}

func TestRunNormalizeLedger_ViaRootCmd(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	content := `{"event":"session_start","timestamp":"2026-02-28T14:00:00Z","session_id":"ses-1"}` + "\n"
	if err := os.WriteFile(filepath.Join(ledgerDir, "sessions.jsonl"), []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"normalize", "ledger", "--ledger-dir", ledgerDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("normalize ledger via root cmd: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "sessions.jsonl") {
		t.Errorf("output should mention sessions.jsonl: %q", got)
	}
}

func TestRunNormalizeLedger_ViaRootCmd_DryRun(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	content := `{"type":"session_start","ts":"2026-02-28T14:00:00Z","sid":"ses-1"}` + "\n"
	if err := os.WriteFile(filepath.Join(ledgerDir, "sessions.jsonl"), []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"normalize", "ledger", "--dry-run", "--ledger-dir", ledgerDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("normalize ledger --dry-run via root cmd: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "DRY RUN") {
		t.Errorf("output should contain 'DRY RUN': %q", got)
	}
	if !strings.Contains(got, "fields renamed") {
		t.Errorf("output should mention fields renamed: %q", got)
	}
}

func TestRunNormalizeLedger_WithNormalizableContent(t *testing.T) {
	t.Parallel()

	ledgerDir := t.TempDir()

	// File with multiple normalization patterns.
	content := `{"type":"memory_stored","ts":"2026-02-28T14:00:00Z","id":"m-1","data":{}}
{"e":"config_set","timestamp":"2026-02-28T14:00:01Z"}
{"op":"INSERT","table":"sessions","timestamp":"2026-02-28T14:00:02Z","session_id":"ses-1"}
{"sid":"ses-1","wid":"w-1","from_status":"todo","to_status":"in_progress","event":"task_status_changed","timestamp":"2026-02-28T14:00:03Z","task_id":"t-1"}
`
	if err := os.WriteFile(filepath.Join(ledgerDir, "work-graph.jsonl"), []byte(content), 0o644); err != nil {
		t.Fatalf("writing test file: %v", err)
	}

	var buf strings.Builder
	err := runNormalizeLedger(&buf, ledgerDir, false)
	if err != nil {
		t.Fatalf("runNormalizeLedger: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "fields renamed") {
		t.Errorf("output should mention fields renamed: %q", got)
	}

	// Verify normalized output.
	data, err := os.ReadFile(filepath.Join(ledgerDir, "work-graph.jsonl"))
	if err != nil {
		t.Fatalf("reading normalized file: %v", err)
	}
	normalized := string(data)

	// type -> event with name normalization.
	if !strings.Contains(normalized, `"event":"memory_store"`) {
		t.Error("should normalize type:memory_stored to event:memory_store")
	}
	// ts -> timestamp.
	if strings.Contains(normalized, `"ts":`) {
		t.Error("should not contain 'ts' field after normalization")
	}
	// e -> event.
	if strings.Contains(normalized, `"e":`) {
		t.Error("should not contain 'e' field after normalization")
	}
	// op+table -> event.
	if strings.Contains(normalized, `"op":`) {
		t.Error("should not contain 'op' field after normalization")
	}
	// sid -> session_id, wid -> work_id.
	if strings.Contains(normalized, `"sid":`) {
		t.Error("should not contain 'sid' field after normalization")
	}
	if strings.Contains(normalized, `"wid":`) {
		t.Error("should not contain 'wid' field after normalization")
	}
	// from_status -> old_status, to_status -> new_status.
	if strings.Contains(normalized, `"from_status":`) {
		t.Error("should not contain 'from_status' field after normalization")
	}
	if strings.Contains(normalized, `"to_status":`) {
		t.Error("should not contain 'to_status' field after normalization")
	}
}
