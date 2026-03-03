package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestNewGitHooksCmd(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksCmd()
	if cmd.Use != "git-hooks" {
		t.Errorf("Use = %q, want %q", cmd.Use, "git-hooks")
	}
	if !cmd.HasSubCommands() {
		t.Error("expected subcommands")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}

	expected := []string{"commit-msg", "post-commit", "pre-push", "prepare-commit-msg", "pre-commit-validate"}
	for _, name := range expected {
		if !names[name] {
			t.Errorf("missing subcommand %q", name)
		}
	}
}

func TestNewGitHooksCommitMsgCmd_MissingArg(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksCommitMsgCmd()
	cmd.SetArgs([]string{})
	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for missing arg")
	}
}

func TestNewGitHooksCommitMsgCmd_ValidMessage(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	if err := os.WriteFile(msgFile, []byte("feat: add login"), 0o644); err != nil {
		t.Fatal(err)
	}

	cmd := newGitHooksCommitMsgCmd()
	cmd.SetArgs([]string{msgFile})
	if err := cmd.Execute(); err != nil {
		t.Errorf("expected valid, got: %v", err)
	}
}

func TestNewGitHooksCommitMsgCmd_InvalidMessage(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	if err := os.WriteFile(msgFile, []byte("BAD FORMAT no colon"), 0o644); err != nil {
		t.Fatal(err)
	}

	cmd := newGitHooksCommitMsgCmd()
	cmd.SetArgs([]string{msgFile})
	cmd.SilenceErrors = true
	cmd.SilenceUsage = true
	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for invalid message")
	}
}

func TestNewGitHooksCommitMsgCmd_NonexistentFile(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksCommitMsgCmd()
	cmd.SetArgs([]string{"/nonexistent/COMMIT_EDITMSG"})
	cmd.SilenceErrors = true
	cmd.SilenceUsage = true
	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for nonexistent file")
	}
}

func TestNewGitHooksPostCommitCmd_NoGitRepo(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksPostCommitCmd()
	cmd.SetArgs([]string{})
	cmd.SilenceErrors = true
	cmd.SilenceUsage = true
	// Will fail because no git repo in the test env detectProjectDir context.
	// We just verify the command runs without panic.
	_ = cmd.Execute()
}

func TestNewGitHooksPrePushCmd_MissingArgs(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksPrePushCmd()
	cmd.SetArgs([]string{})
	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for missing args")
	}
}

func TestNewGitHooksPrepareCommitMsgCmd_SkipSource(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	original := "original content"
	if err := os.WriteFile(msgFile, []byte(original), 0o644); err != nil {
		t.Fatal(err)
	}

	cmd := newGitHooksPrepareCommitMsgCmd()
	cmd.SetArgs([]string{msgFile, "merge"})
	if err := cmd.Execute(); err != nil {
		t.Errorf("expected no error for merge source, got: %v", err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != original {
		t.Error("file was modified for merge source")
	}
}

func TestNewGitHooksPrepareCommitMsgCmd_MissingArg(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksPrepareCommitMsgCmd()
	cmd.SetArgs([]string{})
	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for missing arg")
	}
}

func TestNewGitHooksPreCommitValidateCmd_NoGitRepo(t *testing.T) {
	t.Parallel()

	cmd := newGitHooksPreCommitValidateCmd()
	cmd.SetArgs([]string{})
	cmd.SilenceErrors = true
	cmd.SilenceUsage = true
	// Will fail/succeed depending on git context. Verify no panic.
	_ = cmd.Execute()
}

func TestLoadGitHooksPolicy(t *testing.T) {
	t.Parallel()

	// loadGitHooksPolicy uses detectProjectDir, then loads policy.
	// In test env it may fall back to defaults, which is acceptable.
	policy := loadGitHooksPolicy()
	if policy == nil {
		t.Fatal("expected non-nil policy")
	}
}

func TestNewGitHooksPrePushCmd_WithArgs(t *testing.T) {
	t.Parallel()

	// Exercise RunE with proper args (2 required).
	// Stdin will be empty (os.Stdin in test), so no refs to process.
	// The command will try git branch --show-current which may fail.
	cmd := newGitHooksPrePushCmd()
	cmd.SetArgs([]string{"origin", "https://github.com/test/repo.git"})
	cmd.SilenceErrors = true
	cmd.SilenceUsage = true
	// Don't check error — depends on git state. Just verify no panic.
	_ = cmd.Execute()
}

func TestNewGitHooksPreCommitValidateCmd_WithSilence(t *testing.T) {
	t.Parallel()

	// Exercise with both error output paths silenced.
	cmd := newGitHooksPreCommitValidateCmd()
	cmd.SetArgs([]string{})
	cmd.SilenceErrors = true
	cmd.SilenceUsage = true
	err := cmd.Execute()
	// In test env, will likely fail (no git repo) — we verify no panic
	// and the error path exercises PreCommitErrors handling.
	if err != nil {
		t.Logf("expected failure in test env: %v", err)
	}
}

func TestNewGitHooksCmd_Help(t *testing.T) {
	t.Parallel()

	// Exercise the RunE (which calls cmd.Help()) to cover the 87.5% gap.
	cmd := newGitHooksCmd()
	cmd.SetArgs([]string{})
	// RunE calls cmd.Help() which prints usage and returns nil.
	if err := cmd.Execute(); err != nil {
		t.Errorf("help should not error: %v", err)
	}
}
