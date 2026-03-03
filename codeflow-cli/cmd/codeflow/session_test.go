package main

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewSessionCmd(t *testing.T) {
	t.Parallel()

	cmd := newSessionCmd()
	if cmd.Use != "session" {
		t.Errorf("Use = %q, want %q", cmd.Use, "session")
	}
	if cmd.Short == "" {
		t.Error("session command should have a short description")
	}
}

func TestSessionCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newSessionCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	for _, name := range []string{"start", "end", "current"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on session cmd", name)
		}
	}
}

func TestSessionCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["session"] {
		t.Error("expected 'session' subcommand to be registered in root cmd")
	}
}

func TestSessionStartCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newSessionStartCmd()

	claudeFlag := cmd.Flags().Lookup("claude-id")
	if claudeFlag == nil {
		t.Fatal("session start should have --claude-id flag")
	}

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("session start should have --db flag")
	}
	if dbFlag.DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", dbFlag.DefValue, defaultDBPath())
	}

	ledgerFlag := cmd.Flags().Lookup("ledger")
	if ledgerFlag == nil {
		t.Fatal("session start should have --ledger flag")
	}

	runtimeFlag := cmd.Flags().Lookup("runtime")
	if runtimeFlag == nil {
		t.Fatal("session start should have --runtime flag")
	}
}

func TestSessionStartCmd_RequiresClaudeID(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"session", "start"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error when --claude-id is not provided")
	}
}

func TestSessionEndCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newSessionEndCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("session end should have --db flag")
	}

	ledgerFlag := cmd.Flags().Lookup("ledger")
	if ledgerFlag == nil {
		t.Fatal("session end should have --ledger flag")
	}

	runtimeFlag := cmd.Flags().Lookup("runtime")
	if runtimeFlag == nil {
		t.Fatal("session end should have --runtime flag")
	}
}

func TestSessionCurrentCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newSessionCurrentCmd()

	runtimeFlag := cmd.Flags().Lookup("runtime")
	if runtimeFlag == nil {
		t.Fatal("session current should have --runtime flag")
	}
	if runtimeFlag.DefValue != defaultRuntimeDir() {
		t.Errorf("--runtime default = %q, want %q", runtimeFlag.DefValue, defaultRuntimeDir())
	}
}

func TestSessionCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"session", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("session --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"session", "start", "end", "current"} {
		if !strings.Contains(got, want) {
			t.Errorf("session help missing %q:\n%s", want, got)
		}
	}
}

func TestDefaultRuntimeDir(t *testing.T) {
	t.Parallel()

	got := defaultRuntimeDir()
	if got == "" {
		t.Fatal("defaultRuntimeDir should return non-empty string")
	}
	if !strings.Contains(got, "runtime") {
		t.Errorf("defaultRuntimeDir should contain 'runtime', got: %s", got)
	}
}

func TestContextFromWriter(t *testing.T) {
	t.Parallel()

	ctx := contextFromWriter(io.Discard)
	if ctx == nil {
		t.Fatal("contextFromWriter should return non-nil context")
	}
	// Verify it's a background context (not cancelled).
	if ctx.Err() != nil {
		t.Errorf("contextFromWriter should return non-cancelled context, got err: %v", ctx.Err())
	}
	// Verify it's equivalent to context.Background(). // NOCHECK: context.Background
	if ctx.Done() != context.Background().Done() { // NOCHECK: context.Background
		t.Error("contextFromWriter should return context.Background()") // NOCHECK: context.Background
	}
}

func TestSessionCurrent_MissingFile(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	runtimeDir := filepath.Join(tmpDir, "nonexistent-runtime")

	var buf strings.Builder
	err := runSessionCurrent(&buf, runtimeDir)
	// Should return an error because the session ID file doesn't exist.
	if err == nil {
		t.Error("expected error when runtime dir does not exist")
	}
}

func TestRunSessionStart_CreatesSession(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	// Create necessary directories.
	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	var buf strings.Builder
	err := runSessionStart(&buf, dbPath, "test-agent-001", ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("runSessionStart returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if got == "" {
		t.Fatal("runSessionStart output is empty, want session ID")
	}
	// Session IDs should start with "ses-".
	if !strings.HasPrefix(got, "ses-") {
		t.Errorf("session ID = %q, want prefix %q", got, "ses-")
	}

	// Verify codeflow-env.sh is written (session.WriteEnvFile is the authoritative writer).
	envFile := filepath.Join(runtimeDir, "codeflow-env.sh")
	data, err := os.ReadFile(envFile)
	if err != nil {
		t.Fatalf("codeflow-env.sh not written: %v", err)
	}
	if !strings.Contains(string(data), got) {
		t.Errorf("codeflow-env.sh does not contain session ID %q", got)
	}
}

func TestRunSessionStart_ThenCurrent(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Start a session.
	var startBuf strings.Builder
	if err := runSessionStart(&startBuf, dbPath, "test-agent-002", ledgerDir, runtimeDir); err != nil {
		t.Fatalf("runSessionStart returned error: %v", err)
	}
	sessionID := strings.TrimSpace(startBuf.String())

	// Read current session.
	var curBuf strings.Builder
	if err := runSessionCurrent(&curBuf, runtimeDir); err != nil {
		t.Fatalf("runSessionCurrent returned error: %v", err)
	}

	got := strings.TrimSpace(curBuf.String())
	if got != sessionID {
		t.Errorf("current session = %q, want %q", got, sessionID)
	}
}

func TestRunSessionEnd_AfterStart(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Start a session.
	var startBuf strings.Builder
	if err := runSessionStart(&startBuf, dbPath, "test-agent-003", ledgerDir, runtimeDir); err != nil {
		t.Fatalf("runSessionStart returned error: %v", err)
	}

	// End the session.
	var endBuf strings.Builder
	err := runSessionEnd(&endBuf, dbPath, ledgerDir, runtimeDir)
	if err != nil {
		t.Fatalf("runSessionEnd returned error: %v", err)
	}

	got := endBuf.String()
	if !strings.Contains(got, "Session ended") {
		t.Errorf("output missing %q: %q", "Session ended", got)
	}
}

func TestSessionStartCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"session", "start",
		"--claude-id", "test-agent-cmd",
		"--db", dbPath,
		"--ledger", ledgerDir,
		"--runtime", runtimeDir,
	})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("session start via root cmd returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if !strings.HasPrefix(got, "ses-") {
		t.Errorf("session ID = %q, want prefix %q", got, "ses-")
	}
}

func TestRunSessionStart_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runSessionStart(&buf, "/nonexistent/path/db.db", "agent-1",
		t.TempDir(), t.TempDir())
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunSessionEnd_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runSessionEnd(&buf, "/nonexistent/path/db.db", t.TempDir(), t.TempDir())
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunSessionEnd_NoActiveSession(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Init DB but don't start a session.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runSessionEnd(&buf, dbPath, ledgerDir, runtimeDir)
	if err == nil {
		t.Error("expected error when ending session with no active session")
	}
}

func TestRunSessionStart_ThenEnd_ThenCurrent(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Start session.
	var startBuf strings.Builder
	if err := runSessionStart(&startBuf, dbPath, "test-agent-lifecycle", ledgerDir, runtimeDir); err != nil {
		t.Fatalf("start: %v", err)
	}

	// End session.
	var endBuf strings.Builder
	if err := runSessionEnd(&endBuf, dbPath, ledgerDir, runtimeDir); err != nil {
		t.Fatalf("end: %v", err)
	}

	// Current should fail (session file cleaned up).
	var curBuf strings.Builder
	err := runSessionCurrent(&curBuf, runtimeDir)
	if err == nil {
		t.Error("expected error from current after session ended")
	}
}

func TestSessionEndCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Start a session first.
	var startBuf strings.Builder
	if err := runSessionStart(&startBuf, dbPath, "test-agent-via-root", ledgerDir, runtimeDir); err != nil {
		t.Fatalf("start: %v", err)
	}

	// End via root cmd.
	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"session", "end",
		"--db", dbPath,
		"--ledger", ledgerDir,
		"--runtime", runtimeDir,
	})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("session end via root cmd: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Session ended") {
		t.Errorf("output missing %q: %q", "Session ended", got)
	}
}

func TestSessionCurrentCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")
	ledgerDir := filepath.Join(tmpDir, "ledger")
	runtimeDir := filepath.Join(tmpDir, "runtime")

	for _, dir := range []string{filepath.Dir(dbPath), ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Start session.
	var startBuf strings.Builder
	if err := runSessionStart(&startBuf, dbPath, "test-agent-current", ledgerDir, runtimeDir); err != nil {
		t.Fatalf("start: %v", err)
	}
	sessionID := strings.TrimSpace(startBuf.String())

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"session", "current", "--runtime", runtimeDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("session current via root cmd: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if got != sessionID {
		t.Errorf("current = %q, want %q", got, sessionID)
	}
}


