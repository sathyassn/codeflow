package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// runReportCmdSimple runs a report CLI command via the root cobra command.
func runReportCmdSimple(t *testing.T, args ...string) (string, error) {
	t.Helper()
	cmd := newRootCmd()
	var out bytes.Buffer
	cmd.SetOut(&out)
	cmd.SetErr(&bytes.Buffer{})
	cmd.SetArgs(args)
	err := cmd.Execute()
	return out.String(), err
}

func TestReportCmd_Help(t *testing.T) {
	t.Parallel()
	out, err := runReportCmdSimple(t, "report", "--help")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Generate markdown reports") {
		t.Errorf("help output missing description: %s", out)
	}
	if !strings.Contains(out, "epic-tracker") {
		t.Errorf("help output missing epic-tracker subcommand: %s", out)
	}
}

func TestReportEpicTrackerCmd_NoFlags(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create a minimal epic structure.
	epicsDir := filepath.Join(tmpDir, "project-management", "epics", "INF", "INF-EPC-001")
	if err := os.MkdirAll(epicsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	epicContent := "---\ntitle: CLI Test Epic\nstatus: active\n---\n\n# Test\n"
	if err := os.WriteFile(filepath.Join(epicsDir, "INF-EPC-001-epic.md"), []byte(epicContent), 0o644); err != nil {
		t.Fatal(err)
	}

	out, err := runReportCmdSimple(t, "report", "epic-tracker")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "# Epic Tracker") {
		t.Errorf("output missing Epic Tracker heading: %s", out)
	}
	if !strings.Contains(out, "CLI Test Epic") {
		t.Errorf("output missing epic title: %s", out)
	}
}

func TestReportEpicTrackerCmd_SpecificEpic(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	epicsDir := filepath.Join(tmpDir, "project-management", "epics", "INF", "INF-EPC-001")
	if err := os.MkdirAll(epicsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	epicContent := "---\ntitle: Specific Epic\nstatus: active\npriority: high\n---\n\n# Test\n"
	if err := os.WriteFile(filepath.Join(epicsDir, "INF-EPC-001-epic.md"), []byte(epicContent), 0o644); err != nil {
		t.Fatal(err)
	}

	out, err := runReportCmdSimple(t, "report", "epic-tracker", "--epic", "INF-EPC-001")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "# Epic Report: INF-EPC-001") {
		t.Errorf("output missing single epic header: %s", out)
	}
	if !strings.Contains(out, "**Title:** Specific Epic") {
		t.Errorf("output missing epic title: %s", out)
	}
}

func TestReportEpicTrackerCmd_EpicNotFound(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create the epics directory structure but no matching epic.
	if err := os.MkdirAll(filepath.Join(tmpDir, "project-management", "epics", "INF"), 0o755); err != nil {
		t.Fatal(err)
	}

	_, err := runReportCmdSimple(t, "report", "epic-tracker", "--epic", "NONEXISTENT")
	if err == nil {
		t.Fatal("expected error for nonexistent epic")
	}
}

func TestReportEpicTrackerCmd_WriteFlag(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	epicsDir := filepath.Join(tmpDir, "project-management", "epics", "INF", "INF-EPC-001")
	if err := os.MkdirAll(epicsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	epicContent := "---\ntitle: Write Flag Test\nstatus: active\n---\n\n# Test\n"
	if err := os.WriteFile(filepath.Join(epicsDir, "INF-EPC-001-epic.md"), []byte(epicContent), 0o644); err != nil {
		t.Fatal(err)
	}

	out, err := runReportCmdSimple(t, "report", "epic-tracker", "--write")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Generated:") {
		t.Errorf("output missing 'Generated:' prefix: %s", out)
	}

	// Verify file was written.
	outputPath := filepath.Join(tmpDir, "project-management", "tracking", "epic-tracker.md")
	if _, err := os.Stat(outputPath); err != nil {
		t.Errorf("output file not created: %v", err)
	}
}

func TestReportEpicTrackerCmd_EmptyProject(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	out, err := runReportCmdSimple(t, "report", "epic-tracker")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// Should produce an empty report, not an error.
	if !strings.Contains(out, "| Total Epics | 0 |") {
		t.Errorf("expected zero epics for empty project, got: %s", out)
	}
}
