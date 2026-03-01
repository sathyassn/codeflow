package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// --- Error-path tests (parallel, no file I/O verification) ---

func TestSentinelCreateCmd_Errors(t *testing.T) {
	t.Parallel()

	t.Run("missing name", func(t *testing.T) {
		t.Parallel()
		_, err := runSentinelCmdSimple(t, "sentinel", "create", "--scope", "pathflow", "--session", "ses-abc123")
		if err == nil {
			t.Fatal("expected error for missing name")
		}
	})

	t.Run("missing session", func(t *testing.T) {
		t.Parallel()
		_, err := runSentinelCmdSimple(t, "sentinel", "create", "--scope", "pathflow", "--name", "pf-3")
		if err == nil {
			t.Fatal("expected error for missing session")
		}
	})

	t.Run("invalid scope", func(t *testing.T) {
		t.Parallel()
		_, err := runSentinelCmdSimple(t, "sentinel", "create", "--scope", "skills", "--name", "pf-3", "--session", "ses-abc123")
		if err == nil {
			t.Fatal("expected error for invalid scope")
		}
	})
}

func TestSentinelCheckCmd_NonExistent(t *testing.T) {
	t.Parallel()
	_, err := runSentinelCmdSimple(t, "sentinel", "check", "--scope", "pathflow", "--name", "pf-99", "--session", "ses-abc123")
	if err == nil {
		t.Fatal("expected error for non-existent sentinel")
	}
}

// --- Success-path tests (non-parallel, use os.Chdir for detectProjectDir) ---

// TestSentinelCreateCmd_Success verifies create writes a sentinel file.
// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
func TestSentinelCreateCmd_Success(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	out, err := runSentinelCmdSimple(t, "sentinel", "create", "--scope", "pathflow", "--name", "pf-3", "--session", "ses-abc123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Created sentinel: pf-3") {
		t.Errorf("output %q missing expected message", out)
	}

	path := filepath.Join(tmpDir, ".state", "sentinels", "pathflow", "ses-abc123", "pathflow-pf-3")
	if _, err := os.Stat(path); err != nil {
		t.Errorf("sentinel file not created: %v", err)
	}
}

// TestSentinelCreateCmd_EnvSession tests env var fallback for session ID.
// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir.
func TestSentinelCreateCmd_EnvSession(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)
	t.Setenv("CODEFLOW_SESSION_ID", "ses-env123")

	out, err := runSentinelCmdSimple(t, "sentinel", "create", "--scope", "pathflow", "--name", "ws-dev")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Created sentinel: ws-dev") {
		t.Errorf("output %q missing expected message", out)
	}
}

// TestSentinelCheckCmd_Existing verifies check finds a pre-created sentinel.
// NOTE: no t.Parallel() -- uses os.Chdir.
func TestSentinelCheckCmd_Existing(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	sentinelDir := filepath.Join(tmpDir, ".state", "sentinels", "pathflow", "ses-abc123")
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), nil, 0o644); err != nil {
		t.Fatal(err)
	}

	out, err := runSentinelCmdSimple(t, "sentinel", "check", "--scope", "pathflow", "--name", "pf-3", "--session", "ses-abc123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Sentinel exists: pf-3") {
		t.Errorf("output %q missing expected message", out)
	}
}

// TestSentinelListCmd_WithSentinels verifies list returns sorted sentinel names.
// NOTE: no t.Parallel() -- uses os.Chdir.
func TestSentinelListCmd_WithSentinels(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	sentinelDir := filepath.Join(tmpDir, ".state", "sentinels", "pathflow", "ses-list123")
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"pathflow-pf-1", "pathflow-ws-dev", "pathflow-pf-3"} {
		if err := os.WriteFile(filepath.Join(sentinelDir, name), nil, 0o644); err != nil {
			t.Fatal(err)
		}
	}

	out, err := runSentinelCmdSimple(t, "sentinel", "list", "--scope", "pathflow", "--session", "ses-list123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "pf-1") || !strings.Contains(out, "pf-3") || !strings.Contains(out, "ws-dev") {
		t.Errorf("output %q missing expected sentinels", out)
	}
}

// TestSentinelListCmd_Empty verifies list prints "(none)" for empty directory.
// NOTE: no t.Parallel() -- uses os.Chdir.
func TestSentinelListCmd_Empty(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	out, err := runSentinelCmdSimple(t, "sentinel", "list", "--scope", "pathflow", "--session", "ses-empty123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "(none)") {
		t.Errorf("output %q does not contain '(none)'", out)
	}
}

// TestSentinelDeleteCmd_Existing verifies delete removes a sentinel file.
// NOTE: no t.Parallel() -- uses os.Chdir.
func TestSentinelDeleteCmd_Existing(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	sentinelDir := filepath.Join(tmpDir, ".state", "sentinels", "pathflow", "ses-del123")
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), nil, 0o644); err != nil {
		t.Fatal(err)
	}

	out, err := runSentinelCmdSimple(t, "sentinel", "delete", "--scope", "pathflow", "--name", "pf-3", "--session", "ses-del123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Deleted sentinel: pf-3") {
		t.Errorf("output %q missing expected message", out)
	}
	if _, err := os.Stat(filepath.Join(sentinelDir, "pathflow-pf-3")); !os.IsNotExist(err) {
		t.Error("sentinel file still exists after delete")
	}
}

// TestSentinelDeleteCmd_Idempotent verifies delete succeeds for non-existent sentinel.
// NOTE: no t.Parallel() -- uses os.Chdir.
func TestSentinelDeleteCmd_Idempotent(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	_, err := runSentinelCmdSimple(t, "sentinel", "delete", "--scope", "pathflow", "--name", "pf-99", "--session", "ses-del123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

// TestSentinelCRUDCycle runs a full create -> check -> list -> delete -> check cycle.
// NOTE: no t.Parallel() -- uses os.Chdir.
func TestSentinelCRUDCycle(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)
	sessionID := "ses-cycle123"

	// Create.
	_, err := runSentinelCmdSimple(t, "sentinel", "create", "--scope", "pathflow", "--name", "pf-3", "--session", sessionID)
	if err != nil {
		t.Fatalf("create: %v", err)
	}

	// Check.
	out, err := runSentinelCmdSimple(t, "sentinel", "check", "--scope", "pathflow", "--name", "pf-3", "--session", sessionID)
	if err != nil {
		t.Fatalf("check: %v", err)
	}
	if !strings.Contains(out, "Sentinel exists") {
		t.Errorf("check output %q missing expected message", out)
	}

	// List.
	out, err = runSentinelCmdSimple(t, "sentinel", "list", "--scope", "pathflow", "--session", sessionID)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if !strings.Contains(out, "pf-3") {
		t.Errorf("list output %q missing pf-3", out)
	}

	// Delete.
	_, err = runSentinelCmdSimple(t, "sentinel", "delete", "--scope", "pathflow", "--name", "pf-3", "--session", sessionID)
	if err != nil {
		t.Fatalf("delete: %v", err)
	}

	// Check again (should fail).
	_, err = runSentinelCmdSimple(t, "sentinel", "check", "--scope", "pathflow", "--name", "pf-3", "--session", sessionID)
	if err == nil {
		t.Fatal("check after delete should fail")
	}
}

// --- Helpers ---

// runSentinelCmdSimple runs a sentinel CLI command via the root cobra command.
// For success-path tests, the caller must os.Chdir to a tmpDir with .git so
// that detectProjectDir() returns the correct project root.
func runSentinelCmdSimple(t *testing.T, args ...string) (string, error) {
	t.Helper()
	cmd := newRootCmd()
	var out bytes.Buffer
	cmd.SetOut(&out)
	cmd.SetErr(&bytes.Buffer{})
	cmd.SetArgs(args)
	err := cmd.Execute()
	return out.String(), err
}

// setupGitRepo initializes a minimal git repo for detectProjectDir().
func setupGitRepo(t *testing.T, dir string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Join(dir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
}
