package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/doctor"
	"github.com/spf13/cobra"
)

func TestNewDoctorCmd(t *testing.T) {
	t.Parallel()

	cmd := newDoctorCmd()
	if cmd.Use != "doctor" {
		t.Errorf("Use = %q, want %q", cmd.Use, "doctor")
	}
	if cmd.Short == "" {
		t.Error("doctor command should have a short description")
	}
}

func TestDoctorCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newDoctorCmd()

	for _, tc := range []struct {
		name     string
		defValue string
	}{
		{"check", ""},
		{"json", "false"},
		{"reset", "false"},
		{"repair", "false"},
		{"yes", "false"},
	} {
		flag := cmd.Flags().Lookup(tc.name)
		if flag == nil {
			t.Errorf("doctor command should have --%s flag", tc.name)
			continue
		}
		if flag.DefValue != tc.defValue {
			t.Errorf("--%s default should be %q, got %q", tc.name, tc.defValue, flag.DefValue)
		}
	}
}

func TestDoctorCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["doctor"] {
		t.Error("expected 'doctor' subcommand to be registered in root cmd")
	}
}

func TestDoctorCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"doctor", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for doctor with extra args")
	}
}

func TestClassifyDoctorResult_RuntimeErrors(t *testing.T) {
	t.Parallel()

	for _, name := range []string{"database", "jsonl", "crdt", "config"} {
		r := doctor.Result{Name: name, Status: doctor.StatusFail}
		got := classifyDoctorResult(r)
		if got != ExitRuntimeError {
			t.Errorf("classifyDoctorResult(%q) = %d, want %d", name, got, ExitRuntimeError)
		}
	}
}

func TestClassifyDoctorResult_ExternalErrors(t *testing.T) {
	t.Parallel()

	for _, name := range []string{"claude", "auth", "python", "network"} {
		r := doctor.Result{Name: name, Status: doctor.StatusFail}
		got := classifyDoctorResult(r)
		if got != ExitExternalError {
			t.Errorf("classifyDoctorResult(%q) = %d, want %d", name, got, ExitExternalError)
		}
	}
}

func TestClassifyDoctorResult_UnknownDefaultsToRuntime(t *testing.T) {
	t.Parallel()

	r := doctor.Result{Name: "unknown-check", Status: doctor.StatusFail}
	got := classifyDoctorResult(r)
	if got != ExitRuntimeError {
		t.Errorf("classifyDoctorResult(%q) = %d, want %d", "unknown-check", got, ExitRuntimeError)
	}
}

func TestOutputJSON(t *testing.T) {
	t.Parallel()

	results := []doctor.Result{
		{
			Name:     "database",
			Status:   doctor.StatusPass,
			Message:  "OK",
			Duration: 42 * time.Millisecond,
		},
		{
			Name:     "network",
			Status:   doctor.StatusFail,
			Message:  "unreachable",
			Duration: 150 * time.Millisecond,
		},
	}

	cmd := &cobra.Command{}
	var buf bytes.Buffer
	cmd.SetOut(&buf)

	err := outputJSON(cmd, results)
	if err != nil {
		t.Fatalf("outputJSON returned error: %v", err)
	}

	// Parse the JSON output.
	var parsed []struct {
		Name       string `json:"name"`
		Status     string `json:"status"`
		Message    string `json:"message"`
		DurationMs int64  `json:"duration_ms"`
	}
	if err := json.Unmarshal(buf.Bytes(), &parsed); err != nil {
		t.Fatalf("failed to parse JSON output: %v\nraw: %s", err, buf.String())
	}

	if len(parsed) != 2 {
		t.Fatalf("len(parsed) = %d, want 2", len(parsed))
	}

	if parsed[0].Name != "database" {
		t.Errorf("result[0].name = %q, want %q", parsed[0].Name, "database")
	}
	if parsed[0].Status != "pass" {
		t.Errorf("result[0].status = %q, want %q", parsed[0].Status, "pass")
	}
	if parsed[0].DurationMs != 42 {
		t.Errorf("result[0].duration_ms = %d, want %d", parsed[0].DurationMs, 42)
	}

	if parsed[1].Name != "network" {
		t.Errorf("result[1].name = %q, want %q", parsed[1].Name, "network")
	}
	if parsed[1].Status != "fail" {
		t.Errorf("result[1].status = %q, want %q", parsed[1].Status, "fail")
	}
	if parsed[1].DurationMs != 150 {
		t.Errorf("result[1].duration_ms = %d, want %d", parsed[1].DurationMs, 150)
	}
}

func TestOutputJSON_EmptyResults(t *testing.T) {
	t.Parallel()

	cmd := &cobra.Command{}
	var buf bytes.Buffer
	cmd.SetOut(&buf)

	err := outputJSON(cmd, []doctor.Result{})
	if err != nil {
		t.Fatalf("outputJSON(empty) returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if got != "[]" {
		t.Errorf("output = %s, want %q", got, "[]")
	}
}

func TestPrintResultTable(t *testing.T) {
	t.Parallel()

	results := []doctor.Result{
		{
			Name:     "database",
			Status:   doctor.StatusPass,
			Message:  "all good",
			Duration: 10 * time.Millisecond,
		},
		{
			Name:     "network",
			Status:   doctor.StatusFail,
			Message:  "unreachable",
			Duration: 200 * time.Millisecond,
		},
		{
			Name:     "permissions",
			Status:   doctor.StatusWarn,
			Message:  "not ideal",
			Duration: 5 * time.Millisecond,
		},
	}

	cmd := &cobra.Command{}
	var buf bytes.Buffer
	cmd.SetOut(&buf)

	printResultTable(cmd, results)
	got := buf.String()

	// Verify header.
	if !strings.Contains(got, "CodeFlow Doctor") {
		t.Error("expected 'CodeFlow Doctor' header in output")
	}

	// Verify status icons.
	if !strings.Contains(got, "PASS") {
		t.Error("expected 'PASS' in output for passing check")
	}
	if !strings.Contains(got, "FAIL") {
		t.Error("expected 'FAIL' in output for failing check")
	}
	if !strings.Contains(got, "WARN") {
		t.Error("expected 'WARN' in output for warning check")
	}

	// Verify check names.
	if !strings.Contains(got, "database") {
		t.Error("expected check name 'database' in output")
	}
	if !strings.Contains(got, "network") {
		t.Error("expected check name 'network' in output")
	}

	// Verify summary line.
	if !strings.Contains(got, "1 passed") {
		t.Errorf("output missing %q:\n%s", "1 passed", got)
	}
	if !strings.Contains(got, "1 failed") {
		t.Errorf("output missing %q:\n%s", "1 failed", got)
	}
	if !strings.Contains(got, "1 warnings") {
		t.Errorf("output missing %q:\n%s", "1 warnings", got)
	}
}

func TestPrintResultTable_Empty(t *testing.T) {
	t.Parallel()

	cmd := &cobra.Command{}
	var buf bytes.Buffer
	cmd.SetOut(&buf)

	printResultTable(cmd, []doctor.Result{})
	got := buf.String()

	if !strings.Contains(got, "0 passed, 0 failed, 0 warnings") {
		t.Errorf("output missing %q:\n%s", "0 passed, 0 failed, 0 warnings", got)
	}
}

func TestRunDoctor_AllChecks(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()

	// Create minimal state structure.
	for _, dir := range []string{".state/db", ".state/ledger", ".codeflow/config"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, dir), 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"doctor", "--json"})

	// Doctor will likely find failures (no real infra), but the command should
	// execute without a Go error (it returns exitError for failures).
	_ = cmd.Execute()

	got := buf.String()
	// Should produce JSON output.
	if !strings.Contains(got, "[") {
		t.Errorf("output missing JSON array: %q", got)
	}
}

func TestRunDoctor_SingleCheck(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()

	for _, dir := range []string{".state/db", ".state/ledger", ".codeflow/config"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, dir), 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"doctor", "--check", "config"})

	// May pass or fail depending on check — we just need code path coverage.
	_ = cmd.Execute()

	got := buf.String()
	if !strings.Contains(got, "CodeFlow Doctor") && !strings.Contains(got, "config") {
		t.Errorf("output missing doctor/config content: %q", got)
	}
}

func TestRunDoctor_TableOutput(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()

	for _, dir := range []string{".state/db", ".state/ledger"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, dir), 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"doctor"})

	_ = cmd.Execute()

	got := buf.String()
	if !strings.Contains(got, "CodeFlow Doctor") {
		t.Errorf("output missing %q: %q", "CodeFlow Doctor", got)
	}
	if !strings.Contains(got, "Results:") {
		t.Errorf("output missing %q: %q", "Results:", got)
	}
}

func TestRunDoctorRepair_BasicExecution(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()

	for _, dir := range []string{".state/db", ".state/ledger", ".codeflow/config"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, dir), 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"doctor", "--repair"})

	// Repair may encounter errors, but the function should still execute.
	_ = cmd.Execute()

	got := buf.String()
	if !strings.Contains(got, "Running repairs") {
		t.Errorf("output missing %q: %q", "Running repairs", got)
	}
	if !strings.Contains(got, "Repair complete") {
		t.Errorf("output missing %q: %q", "Repair complete", got)
	}
}

func TestDoctorReset_Cancelled(t *testing.T) {
	t.Parallel()

	cmd := &cobra.Command{}
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetIn(strings.NewReader("n\n"))

	tmpDir := t.TempDir()
	opts := &doctor.Options{StateDir: tmpDir}

	err := runDoctorReset(cmd, opts, false)
	if err != nil {
		t.Fatalf("runDoctorReset with 'n' returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "cancelled") {
		t.Errorf("output missing %q: %q", "cancelled", got)
	}
}

func TestDoctorReset_YesFlag(t *testing.T) {
	t.Parallel()

	cmd := &cobra.Command{}
	var buf bytes.Buffer
	cmd.SetOut(&buf)

	tmpDir := t.TempDir()
	opts := &doctor.Options{StateDir: tmpDir}

	err := runDoctorReset(cmd, opts, true)
	if err != nil {
		t.Fatalf("runDoctorReset with --yes returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "reset successfully") {
		t.Errorf("output missing %q: %q", "reset successfully", got)
	}
}
