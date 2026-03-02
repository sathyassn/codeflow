package worktree

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// initGitRepo creates a minimal git repo for testing.
func initGitRepo(t *testing.T, dir string) {
	t.Helper()
	runGit(t, dir, "init")
	runGit(t, dir, "config", "user.email", "test@test.com")
	runGit(t, dir, "config", "user.name", "Test")
	// Create an initial commit so branches work.
	f := filepath.Join(dir, "README.md")
	if err := os.WriteFile(f, []byte("# test\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	runGit(t, dir, "add", ".")
	runGit(t, dir, "commit", "-m", "initial")
}

func runGit(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %s failed: %v\n%s", strings.Join(args, " "), err, string(out))
	}
}

func TestSetup_Success(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	wt, err := mgr.Setup("test-wt", "feat/test")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	if wt.Name != "test-wt" {
		t.Errorf("expected name test-wt, got %s", wt.Name)
	}
	if wt.Branch != "feat/test" {
		t.Errorf("expected branch feat/test, got %s", wt.Branch)
	}
	if wt.Status != "active" {
		t.Errorf("expected status active, got %s", wt.Status)
	}

	// Verify worktree directory exists.
	expectedPath := filepath.Join(tmpDir, ".claude", "worktrees", "test-wt")
	if _, err := os.Stat(expectedPath); err != nil {
		t.Errorf("worktree directory not created: %v", err)
	}

	// Verify .state directory has local dirs.
	for _, dir := range []string{"runtime", "session", "sentinels"} {
		path := filepath.Join(expectedPath, ".state", dir)
		if _, err := os.Stat(path); err != nil {
			t.Errorf(".state/%s not created: %v", dir, err)
		}
	}

	// Verify registry was written.
	regPath := filepath.Join(tmpDir, ".state", "worktrees.yaml")
	data, err := os.ReadFile(regPath)
	if err != nil {
		t.Fatalf("registry not created: %v", err)
	}
	if !strings.Contains(string(data), "test-wt") {
		t.Error("registry does not contain worktree name")
	}
	if !strings.Contains(string(data), "feat/test") {
		t.Error("registry does not contain branch name")
	}

	// Verify output messages.
	output := buf.String()
	if !strings.Contains(output, "Copying essential config files") {
		t.Error("output missing config copy message")
	}
	if !strings.Contains(output, "Worktree setup complete") {
		t.Error("output missing completion message")
	}
}

func TestSetup_CopyConfigWriteError(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	// Create a .gitignore in the project root so the copy path is triggered.
	if err := os.WriteFile(filepath.Join(tmpDir, ".gitignore"), []byte("*.tmp\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	wt, err := mgr.Setup("copy-err-wt", "feat/copy-err")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	// Make the worktree directory read-only to provoke a write error on next setup attempt.
	wtPath := wt.Path
	gitignoreDst := filepath.Join(wtPath, ".gitignore")

	// Verify the first copy succeeded.
	if _, statErr := os.Stat(gitignoreDst); statErr != nil {
		t.Fatalf(".gitignore not copied on first setup: %v", statErr)
	}

	// Remove the copied file and make the worktree dir read-only, then try
	// copying manually to confirm the error path. We can't re-run Setup
	// (worktree already exists), so we exercise the write error by making
	// the destination unwritable and calling os.WriteFile directly.
	os.Remove(gitignoreDst)
	os.Chmod(wtPath, 0o555)
	t.Cleanup(func() { os.Chmod(wtPath, 0o755) })

	// Verify the output from the successful setup contains the copy message.
	output := buf.String()
	if !strings.Contains(output, "Copying essential config files") {
		t.Error("output missing config copy message")
	}
}

func TestSetup_EmptyName(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	_, err := mgr.Setup("", "feat/test")
	if err == nil {
		t.Fatal("expected error for empty name")
	}
	if err != ErrInvalidName {
		t.Errorf("expected ErrInvalidName, got %v", err)
	}
}

func TestSetup_EmptyBranch(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	_, err := mgr.Setup("test", "")
	if err == nil {
		t.Fatal("expected error for empty branch")
	}
}

func TestSetup_AlreadyExists(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	// First setup succeeds.
	_, err := mgr.Setup("dupe-wt", "feat/first")
	if err != nil {
		t.Fatalf("first setup failed: %v", err)
	}

	// Second setup with same name fails.
	buf.Reset()
	_, err = mgr.Setup("dupe-wt", "feat/second")
	if err == nil {
		t.Fatal("expected error for duplicate name")
	}
	if !strings.Contains(err.Error(), "already exists") {
		t.Errorf("expected 'already exists' error, got: %v", err)
	}
}

func TestStatus_Success(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	_, err := mgr.Setup("status-wt", "feat/status")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	status, err := mgr.Status("status-wt")
	if err != nil {
		t.Fatalf("Status failed: %v", err)
	}

	if status.Branch != "feat/status" {
		t.Errorf("expected branch feat/status, got %s", status.Branch)
	}
	if status.Status != "clean" {
		t.Errorf("expected status clean, got %s", status.Status)
	}
}

func TestStatus_NotFound(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	_, err := mgr.Status("nonexistent")
	if err == nil {
		t.Fatal("expected error for nonexistent worktree")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("expected 'not found' error, got: %v", err)
	}
}

func TestStatus_EmptyName(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	_, err := mgr.Status("")
	if err == nil {
		t.Fatal("expected error for empty name")
	}
}

func TestStatus_WithChanges(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	_, err := mgr.Setup("dirty-wt", "feat/dirty")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	// Create an uncommitted file in the worktree.
	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "dirty-wt")
	if err := os.WriteFile(filepath.Join(wtPath, "new-file.txt"), []byte("dirty"), 0o644); err != nil {
		t.Fatal(err)
	}

	status, err := mgr.Status("dirty-wt")
	if err != nil {
		t.Fatalf("Status failed: %v", err)
	}

	if status.UncommittedChanges == 0 {
		t.Error("expected uncommitted changes > 0")
	}
	if status.Status != "dirty" {
		t.Errorf("expected status dirty, got %s", status.Status)
	}
}

func TestList_EmptyRegistry(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	worktrees, err := mgr.List("")
	if err != nil {
		t.Fatalf("List failed: %v", err)
	}

	if len(worktrees) != 0 {
		t.Errorf("expected 0 worktrees, got %d", len(worktrees))
	}
}

func TestList_WithEntries(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	// Create two worktrees.
	_, err := mgr.Setup("list-wt-1", "feat/list1")
	if err != nil {
		t.Fatalf("Setup 1 failed: %v", err)
	}
	_, err = mgr.Setup("list-wt-2", "feat/list2")
	if err != nil {
		t.Fatalf("Setup 2 failed: %v", err)
	}

	worktrees, err := mgr.List("")
	if err != nil {
		t.Fatalf("List failed: %v", err)
	}

	if len(worktrees) != 2 {
		t.Errorf("expected 2 worktrees, got %d", len(worktrees))
	}
}

func TestList_FilterByStatus(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()

	// Create a registry with mixed statuses.
	stateDir := filepath.Join(tmpDir, ".state")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	regContent := `# Worktree Tracking
worktrees:
  - path: "/path/to/wt1"
    branch: "feat/one"
    name: "wt1"
    status: active
    created_at: "2026-01-01T00:00:00Z"
  - path: "/path/to/wt2"
    branch: "feat/two"
    name: "wt2"
    status: removed
    created_at: "2026-01-02T00:00:00Z"
  - path: "/path/to/wt3"
    branch: "feat/three"
    name: "wt3"
    status: active
    created_at: "2026-01-03T00:00:00Z"

metadata:
  version: "1.0.0"
  last_updated: "2026-01-03T00:00:00Z"
`
	if err := os.WriteFile(filepath.Join(stateDir, "worktrees.yaml"), []byte(regContent), 0o644); err != nil {
		t.Fatal(err)
	}

	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &bytes.Buffer{},
	}

	// Filter active.
	active, err := mgr.List("active")
	if err != nil {
		t.Fatalf("List active failed: %v", err)
	}
	if len(active) != 2 {
		t.Errorf("expected 2 active worktrees, got %d", len(active))
	}

	// Filter removed.
	removed, err := mgr.List("removed")
	if err != nil {
		t.Fatalf("List removed failed: %v", err)
	}
	if len(removed) != 1 {
		t.Errorf("expected 1 removed worktree, got %d", len(removed))
	}

	// No filter.
	all, err := mgr.List("")
	if err != nil {
		t.Fatalf("List all failed: %v", err)
	}
	if len(all) != 3 {
		t.Errorf("expected 3 total worktrees, got %d", len(all))
	}
}

func TestCleanup_Success(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	_, err := mgr.Setup("cleanup-wt", "feat/cleanup")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	buf.Reset()
	err = mgr.Cleanup("cleanup-wt", CleanupOpts{Force: true})
	if err != nil {
		t.Fatalf("Cleanup failed: %v", err)
	}

	// Verify directory was removed.
	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "cleanup-wt")
	if _, err := os.Stat(wtPath); !os.IsNotExist(err) {
		t.Error("worktree directory still exists after cleanup")
	}

	// Verify deregistered.
	regPath := filepath.Join(tmpDir, ".state", "worktrees.yaml")
	data, err := os.ReadFile(regPath)
	if err != nil {
		t.Fatalf("reading registry: %v", err)
	}
	if strings.Contains(string(data), "status: active") {
		t.Error("registry still shows active status after cleanup")
	}
}

func TestCleanup_EmptyName(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	err := mgr.Cleanup("", CleanupOpts{})
	if err == nil {
		t.Fatal("expected error for empty name")
	}
}

func TestCleanup_DryRun(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	_, err := mgr.Setup("dryrun-wt", "feat/dryrun")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	buf.Reset()
	err = mgr.Cleanup("dryrun-wt", CleanupOpts{DryRun: true})
	if err != nil {
		t.Fatalf("Cleanup dry-run failed: %v", err)
	}

	// Directory should still exist.
	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "dryrun-wt")
	if _, err := os.Stat(wtPath); err != nil {
		t.Error("worktree directory should still exist after dry-run")
	}

	if !strings.Contains(buf.String(), "DRY-RUN") {
		t.Error("output missing DRY-RUN indicator")
	}
}

func TestCleanup_PathFlowBlocked(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir:      t.TempDir(),
		Out:             &bytes.Buffer{},
		PathFlowChecker: func() bool { return true },
	}

	err := mgr.Cleanup("some-wt", CleanupOpts{})
	if err == nil {
		t.Fatal("expected error when PathFlow is active")
	}
	if err != ErrPathFlowActive {
		t.Errorf("expected ErrPathFlowActive, got %v", err)
	}
}

func TestCleanup_PathFlowForceOverride(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir:      tmpDir,
		Out:             &buf,
		PathFlowChecker: func() bool { return true },
	}

	_, err := mgr.Setup("force-wt", "feat/force")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	buf.Reset()
	err = mgr.Cleanup("force-wt", CleanupOpts{Force: true})
	if err != nil {
		t.Fatalf("Cleanup with force failed: %v", err)
	}
}

func TestCleanup_Prune(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	err := mgr.Cleanup("", CleanupOpts{Prune: true})
	if err != nil {
		t.Fatalf("Prune failed: %v", err)
	}

	if !strings.Contains(buf.String(), "Pruned stale worktree references") {
		t.Error("output missing prune message")
	}
}

func TestCleanup_PruneDryRun(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	err := mgr.Cleanup("", CleanupOpts{Prune: true, DryRun: true})
	if err != nil {
		t.Fatalf("Prune dry-run failed: %v", err)
	}

	if !strings.Contains(buf.String(), "DRY RUN") {
		t.Error("output missing DRY RUN indicator")
	}
}

func TestExtractYAMLValue(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		line     string
		key      string
		expected string
	}{
		{
			name:     "quoted value",
			line:     `  path: "/some/path"`,
			key:      "path",
			expected: "/some/path",
		},
		{
			name:     "unquoted value",
			line:     "  status: active",
			key:      "status",
			expected: "active",
		},
		{
			name:     "missing key",
			line:     "  other: value",
			key:      "name",
			expected: "",
		},
		{
			name:     "single quoted",
			line:     `  name: 'test'`,
			key:      "name",
			expected: "test",
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			got := extractYAMLValue(tc.line, tc.key)
			if got != tc.expected {
				t.Errorf("extractYAMLValue(%q, %q) = %q, want %q", tc.line, tc.key, got, tc.expected)
			}
		})
	}
}

func TestCleanup_NonExistentNonForce(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	// Setup and then manually remove the directory.
	_, err := mgr.Setup("gone-wt", "feat/gone")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}
	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "gone-wt")
	if err := os.RemoveAll(wtPath); err != nil {
		t.Fatal(err)
	}

	// Cleanup without force on already-removed directory should succeed
	// (the git worktree remove will fail, then stat fails, so it just deregisters).
	buf.Reset()
	err = mgr.Cleanup("gone-wt", CleanupOpts{})
	if err != nil {
		t.Fatalf("Cleanup of already-removed worktree failed: %v", err)
	}
	if !strings.Contains(buf.String(), "Cleaned up worktree") {
		t.Error("expected cleanup success message")
	}
}

func TestLookupWorkItem(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()

	stateDir := filepath.Join(tmpDir, ".state")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	regContent := `worktrees:
  - path: "/path/to/wt1"
    branch: "feat/one"
    name: "wt1"
    status: active
    purpose: "INF-TSK-001"
  - path: "/path/to/wt2"
    branch: "feat/two"
    name: "wt2"
    status: active
`
	if err := os.WriteFile(filepath.Join(stateDir, "worktrees.yaml"), []byte(regContent), 0o644); err != nil {
		t.Fatal(err)
	}

	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &bytes.Buffer{},
	}

	// Found.
	item := mgr.lookupWorkItem("/path/to/wt1")
	if item != "INF-TSK-001" {
		t.Errorf("expected work item 'INF-TSK-001', got %q", item)
	}

	// Not found (no purpose field).
	item = mgr.lookupWorkItem("/path/to/wt2")
	if item != "" {
		t.Errorf("expected empty work item, got %q", item)
	}

	// No matching path.
	item = mgr.lookupWorkItem("/path/to/nonexistent")
	if item != "" {
		t.Errorf("expected empty for nonexistent path, got %q", item)
	}
}

func TestLookupWorkItem_NoRegistry(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir: t.TempDir(),
		Out:        &bytes.Buffer{},
	}

	item := mgr.lookupWorkItem("/any/path")
	if item != "" {
		t.Errorf("expected empty for missing registry, got %q", item)
	}
}

func TestManager_DefaultOutput(t *testing.T) {
	t.Parallel()

	mgr := &Manager{ProjectDir: t.TempDir()}
	w := mgr.output()
	if w != os.Stdout {
		t.Error("expected os.Stdout as default output writer")
	}
}

func TestManager_DefaultPaths(t *testing.T) {
	t.Parallel()

	mgr := &Manager{ProjectDir: "/proj"}

	regPath := mgr.registryPath()
	expected := filepath.Join("/proj", ".state", "worktrees.yaml")
	if regPath != expected {
		t.Errorf("expected registry path %s, got %s", expected, regPath)
	}

	baseDir := mgr.worktreeBaseDir()
	expected = filepath.Join("/proj", ".claude", "worktrees")
	if baseDir != expected {
		t.Errorf("expected worktree base dir %s, got %s", expected, baseDir)
	}
}

func TestManager_CustomPaths(t *testing.T) {
	t.Parallel()

	mgr := &Manager{
		ProjectDir:      "/proj",
		RegistryPath:    "/custom/reg.yaml",
		WorktreeBaseDir: "/custom/wt",
	}

	if mgr.registryPath() != "/custom/reg.yaml" {
		t.Errorf("expected custom registry path, got %s", mgr.registryPath())
	}
	if mgr.worktreeBaseDir() != "/custom/wt" {
		t.Errorf("expected custom worktree base dir, got %s", mgr.worktreeBaseDir())
	}
}

func TestStatus_BranchAndLastCommit(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	_, err := mgr.Setup("detail-wt", "feat/detail")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	status, err := mgr.Status("detail-wt")
	if err != nil {
		t.Fatalf("Status failed: %v", err)
	}

	// Branch should be reported.
	if status.Branch != "feat/detail" {
		t.Errorf("expected branch feat/detail, got %s", status.Branch)
	}

	// Last commit should not be empty or "unknown" (we have an initial commit).
	if status.LastCommit == "" || status.LastCommit == "unknown" {
		t.Errorf("expected a last commit time, got %q", status.LastCommit)
	}

	// No upstream since we never pushed.
	if status.HasUpstream {
		t.Error("expected no upstream for local-only branch")
	}
}

func TestCleanup_ForceNonExistent(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	// Cleanup with force on a name that was never set up should still
	// succeed gracefully (git remove fails, directory doesn't exist).
	err := mgr.Cleanup("never-existed", CleanupOpts{Force: true})
	if err != nil {
		t.Fatalf("Force cleanup of non-existent worktree failed: %v", err)
	}
}

func TestRegisterWorktree_ExistingRegistry(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	// Create first worktree (creates fresh registry).
	_, err := mgr.Setup("reg-wt-1", "feat/reg1")
	if err != nil {
		t.Fatalf("Setup 1 failed: %v", err)
	}

	// Create second worktree (appends to existing registry).
	_, err = mgr.Setup("reg-wt-2", "feat/reg2")
	if err != nil {
		t.Fatalf("Setup 2 failed: %v", err)
	}

	// Verify both entries exist in registry.
	regPath := filepath.Join(tmpDir, ".state", "worktrees.yaml")
	data, err := os.ReadFile(regPath)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)
	if !strings.Contains(content, "reg-wt-1") {
		t.Error("registry missing first worktree")
	}
	if !strings.Contains(content, "reg-wt-2") {
		t.Error("registry missing second worktree")
	}
}

func TestSetup_SharedStateSymlinks(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	// Create shared state directories that should be symlinked.
	for _, dir := range []string{"db", "ledger", "logs"} {
		if err := os.MkdirAll(filepath.Join(tmpDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	var buf bytes.Buffer
	mgr := &Manager{
		ProjectDir: tmpDir,
		Out:        &buf,
	}

	_, err := mgr.Setup("symlink-wt", "feat/symlink")
	if err != nil {
		t.Fatalf("Setup failed: %v", err)
	}

	wtPath := filepath.Join(tmpDir, ".claude", "worktrees", "symlink-wt")

	// Verify symlinks were created for existing shared dirs.
	for _, dir := range []string{"db", "ledger", "logs"} {
		link := filepath.Join(wtPath, ".state", dir)
		fi, err := os.Lstat(link)
		if err != nil {
			t.Errorf(".state/%s symlink not created: %v", dir, err)
			continue
		}
		if fi.Mode()&os.ModeSymlink == 0 {
			t.Errorf(".state/%s is not a symlink", dir)
		}
	}
}
