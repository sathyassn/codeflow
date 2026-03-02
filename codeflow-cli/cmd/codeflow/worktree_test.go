package main

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// runWorktreeCmdSimple runs a worktree CLI command via the root cobra command.
func runWorktreeCmdSimple(t *testing.T, args ...string) (string, error) {
	t.Helper()
	cmd := newRootCmd()
	var out bytes.Buffer
	cmd.SetOut(&out)
	cmd.SetErr(&bytes.Buffer{})
	cmd.SetArgs(args)
	err := cmd.Execute()
	return out.String(), err
}

// initRealGitRepo creates a real git repo with an initial commit for worktree tests.
func initRealGitRepo(t *testing.T, dir string) {
	t.Helper()
	runGitCmd(t, dir, "init")
	runGitCmd(t, dir, "config", "user.email", "test@test.com")
	runGitCmd(t, dir, "config", "user.name", "Test")
	f := filepath.Join(dir, "README.md")
	if err := os.WriteFile(f, []byte("# test\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	runGitCmd(t, dir, "add", ".")
	runGitCmd(t, dir, "commit", "-m", "initial")
}

func runGitCmd(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %s failed: %v\n%s", args[0], err, string(out))
	}
}

func TestWorktreeCmd_Help(t *testing.T) {
	t.Parallel()
	out, err := runWorktreeCmdSimple(t, "worktree", "--help")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "isolated git worktrees") {
		t.Errorf("help output missing description: %s", out)
	}
	for _, sub := range []string{"setup", "status", "list", "cleanup"} {
		if !strings.Contains(out, sub) {
			t.Errorf("help output missing subcommand %q: %s", sub, out)
		}
	}
}

func TestWorktreeSetupCmd_MissingName(t *testing.T) {
	t.Parallel()
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--branch", "feat/test")
	if err == nil {
		t.Fatal("expected error for missing --name flag")
	}
}

func TestWorktreeSetupCmd_MissingBranch(t *testing.T) {
	t.Parallel()
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "test")
	if err == nil {
		t.Fatal("expected error for missing --branch flag")
	}
}

func TestWorktreeSetupCmd_Success(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	initRealGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create .state directory with shared subdirs so Setup can symlink.
	for _, dir := range []string{"db", "ledger", "registry", "backups", "coordination", "logs"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	out, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "test-wt", "--branch", "feat/test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Created worktree") {
		t.Errorf("expected 'Created worktree' in output, got: %s", out)
	}
	if !strings.Contains(out, "test-wt") {
		t.Errorf("expected worktree name in output, got: %s", out)
	}
	if !strings.Contains(out, "feat/test") {
		t.Errorf("expected branch name in output, got: %s", out)
	}

	// Verify worktree directory was created.
	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "test-wt")
	if _, err := os.Stat(wtPath); err != nil {
		t.Errorf("worktree directory not created: %v", err)
	}
}

func TestWorktreeStatusCmd_MissingName(t *testing.T) {
	t.Parallel()
	_, err := runWorktreeCmdSimple(t, "worktree", "status")
	if err == nil {
		t.Fatal("expected error for missing --name flag")
	}
}

func TestWorktreeStatusCmd_Success(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	initRealGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create shared state dirs.
	for _, dir := range []string{"db", "ledger"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// First, set up a worktree so we can query its status.
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "status-wt", "--branch", "feat/status-test")
	if err != nil {
		t.Fatalf("setup failed: %v", err)
	}

	// Now query status in text mode.
	out, err := runWorktreeCmdSimple(t, "worktree", "status", "--name", "status-wt")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Worktree:") {
		t.Errorf("expected 'Worktree:' in output, got: %s", out)
	}
	if !strings.Contains(out, "Branch:") {
		t.Errorf("expected 'Branch:' in output, got: %s", out)
	}
	if !strings.Contains(out, "Status:") {
		t.Errorf("expected 'Status:' in output, got: %s", out)
	}
	if !strings.Contains(out, "Last commit:") {
		t.Errorf("expected 'Last commit:' in output, got: %s", out)
	}
}

func TestWorktreeStatusCmd_JSON(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	initRealGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create shared state dirs.
	for _, dir := range []string{"db", "ledger"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// Set up worktree.
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "json-wt", "--branch", "feat/json-test")
	if err != nil {
		t.Fatalf("setup failed: %v", err)
	}

	// Query status in JSON mode.
	out, err := runWorktreeCmdSimple(t, "worktree", "status", "--name", "json-wt", "--json")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, `"path"`) {
		t.Errorf("expected JSON 'path' key in output, got: %s", out)
	}
	if !strings.Contains(out, `"branch"`) {
		t.Errorf("expected JSON 'branch' key in output, got: %s", out)
	}
	if !strings.Contains(out, `"status"`) {
		t.Errorf("expected JSON 'status' key in output, got: %s", out)
	}
}

func TestWorktreeListCmd_NoArgs(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	out, err := runWorktreeCmdSimple(t, "worktree", "list")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "No tracked worktrees") {
		t.Errorf("expected 'No tracked worktrees' message, got: %s", out)
	}
}

func TestWorktreeListCmd_WithEntries(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	initRealGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create shared state dirs.
	for _, dir := range []string{"db", "ledger"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// Set up two worktrees so the list has entries.
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "list-wt-1", "--branch", "feat/list-1")
	if err != nil {
		t.Fatalf("setup 1 failed: %v", err)
	}
	_, err = runWorktreeCmdSimple(t, "worktree", "setup", "--name", "list-wt-2", "--branch", "feat/list-2")
	if err != nil {
		t.Fatalf("setup 2 failed: %v", err)
	}

	// List all worktrees.
	out, err := runWorktreeCmdSimple(t, "worktree", "list")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Tracked Worktrees") {
		t.Errorf("expected 'Tracked Worktrees' header, got: %s", out)
	}
	if !strings.Contains(out, "PATH") {
		t.Errorf("expected column header 'PATH', got: %s", out)
	}
	if !strings.Contains(out, "feat/list-1") {
		t.Errorf("expected branch feat/list-1 in list, got: %s", out)
	}
	if !strings.Contains(out, "feat/list-2") {
		t.Errorf("expected branch feat/list-2 in list, got: %s", out)
	}
}

func TestWorktreeListCmd_WithFilter(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	initRealGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create shared state dirs.
	for _, dir := range []string{"db", "ledger"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// Set up a worktree (status=active).
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "filter-wt", "--branch", "feat/filter-test")
	if err != nil {
		t.Fatalf("setup failed: %v", err)
	}

	// Filter by active -- should find the entry.
	out, err := runWorktreeCmdSimple(t, "worktree", "list", "--status", "active")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "feat/filter-test") {
		t.Errorf("expected entry for active filter, got: %s", out)
	}

	// Filter by stale -- should find nothing.
	out, err = runWorktreeCmdSimple(t, "worktree", "list", "--status", "stale")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "No tracked worktrees") {
		t.Errorf("expected 'No tracked worktrees' for stale filter, got: %s", out)
	}
}

func TestWorktreeCleanupCmd_NeitherNameNorPrune(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	_, err := runWorktreeCmdSimple(t, "worktree", "cleanup")
	if err == nil {
		t.Fatal("expected error when neither --name nor --prune is provided")
	}
	if !strings.Contains(err.Error(), "--name is required") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestWorktreeCleanupCmd_DryRunFlag(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	out, err := runWorktreeCmdSimple(t, "worktree", "cleanup", "--name", "nonexistent", "--dry-run")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "DRY-RUN") {
		t.Errorf("expected DRY-RUN in output, got: %s", out)
	}
}

func TestWorktreeCleanupCmd_PruneFlag(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	setupGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Need a real git repo for prune to work.
	cmd := exec.Command("git", "-C", tmpDir, "init")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git init failed: %v\n%s", err, string(out))
	}

	out, err := runWorktreeCmdSimple(t, "worktree", "cleanup", "--prune")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Pruned stale worktree references") {
		t.Errorf("expected prune message, got: %s", out)
	}
}

func TestWorktreeCleanupCmd_SuccessRemove(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Chdir to control detectProjectDir.
	tmpDir := t.TempDir()
	initRealGitRepo(t, tmpDir)
	t.Chdir(tmpDir)

	// Create shared state dirs.
	for _, dir := range []string{"db", "ledger"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// Set up a worktree then clean it up.
	_, err := runWorktreeCmdSimple(t, "worktree", "setup", "--name", "cleanup-wt", "--branch", "feat/cleanup-test")
	if err != nil {
		t.Fatalf("setup failed: %v", err)
	}

	out, err := runWorktreeCmdSimple(t, "worktree", "cleanup", "--name", "cleanup-wt", "--force")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.Contains(out, "Cleaned up worktree") {
		t.Errorf("expected 'Cleaned up worktree' in output, got: %s", out)
	}

	// Verify the worktree directory was removed.
	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "cleanup-wt")
	if _, err := os.Stat(wtPath); err == nil {
		t.Error("worktree directory should have been removed")
	}
}
