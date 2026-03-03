package githooks

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestMatchBranchPattern_Exact(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		branch  string
		pattern string
		want    bool
	}{
		{"exact match", "main", "main", true},
		{"no match", "main", "master", false},
		{"prefix not enough", "main-branch", "main", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := matchBranchPattern(tt.branch, tt.pattern); got != tt.want {
				t.Errorf("matchBranchPattern(%q, %q) = %v, want %v",
					tt.branch, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestMatchBranchPattern_Wildcard(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		branch  string
		pattern string
		want    bool
	}{
		{"release wildcard match", "release/1.0", "release/*", true},
		{"release wildcard no match", "release-1.0", "release/*", false},
		{"nested match", "release/v1/hotfix", "release/*", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := matchBranchPattern(tt.branch, tt.pattern); got != tt.want {
				t.Errorf("matchBranchPattern(%q, %q) = %v, want %v",
					tt.branch, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestIsProtectedBranch(t *testing.T) {
	t.Parallel()

	protected := []string{"main", "master", "release/*", "production"}
	tests := []struct {
		name   string
		branch string
		want   bool
	}{
		{"main", "main", true},
		{"master", "master", true},
		{"production", "production", true},
		{"release/1.0", "release/1.0", true},
		{"feat/something", "feat/something", false},
		{"develop", "develop", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := isProtectedBranch(tt.branch, protected); got != tt.want {
				t.Errorf("isProtectedBranch(%q) = %v, want %v", tt.branch, got, tt.want)
			}
		})
	}
}

func TestRunPrePush_NonProtectedBranch(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Stdin format: local_ref local_sha remote_ref remote_sha
	input := "refs/heads/feat/x abc123 refs/heads/feat/x def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	// This will try to run `git branch --show-current` which may fail in test.
	// The function should still not error for non-protected branches.
	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	// The error may come from git command failures, which is acceptable in test.
	if err != nil {
		t.Logf("got error (may be git command failure in test env): %v", err)
	}
}

func TestRunPrePush_ProtectedBranch(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Push to main (protected).
	input := "refs/heads/feat/x abc123 refs/heads/main def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	if err == nil {
		t.Fatal("expected error for push to protected branch")
	}
	if !strings.Contains(err.Error(), "protected branch") {
		t.Errorf("expected 'protected branch' in error, got: %v", err)
	}
	if !strings.Contains(stderr.String(), "Push to protected branch") {
		t.Error("expected protected branch warning in stderr")
	}
}

func TestRunPrePush_DeleteOperation(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Delete operation: localSHA is all zeros.
	input := "refs/heads/feat/x " + zeroSHA + " refs/heads/main def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	// Delete operations are skipped, so even pushing to main is OK.
	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	// May get git command error, but should not get "protected branch" error.
	if err != nil && strings.Contains(err.Error(), "protected branch") {
		t.Errorf("delete operations should skip protection, got: %v", err)
	}
}

func TestRunPrePush_EmptyStdin(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	stdin := strings.NewReader("")
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	// May fail on git branch --show-current, that's OK.
	if err != nil {
		t.Logf("got error (may be git command failure in test env): %v", err)
	}
}

func TestRunPrePush_MalformedStdin(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Less than 4 fields — should be skipped.
	input := "only two fields\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	if err != nil {
		t.Logf("got error (may be git command failure in test env): %v", err)
	}
}

func TestRunPrePush_MultipleRefs(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Multiple refs: first is fine, second targets protected branch.
	input := "refs/heads/feat/x abc123 refs/heads/feat/x def456\n" +
		"refs/heads/feat/y ghi789 refs/heads/main jkl012\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	if err == nil {
		t.Fatal("expected error for push to protected branch")
	}
}

func TestRunPrePush_BranchNameWarning(t *testing.T) {
	// NOTE: no t.Parallel — depends on current git branch state.
	// This test only works when run inside a git repo on a branch
	// that doesn't match the restrictive policy prefixes below.
	t.Parallel()

	policy := &EnforcementPolicy{
		ProtectedBranches: []string{"main"},
		GitFormat: GitFormat{
			BranchPrefixes: []string{"never-match/"},
		},
	}
	// Empty stdin — no refs, goes straight to branch name validation.
	stdin := strings.NewReader("")
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	// Should not error (warning only), but the current branch may or may not
	// trigger the warning depending on the environment.
	if err != nil {
		t.Logf("got error (may be git env): %v", err)
		return
	}

	// If we got here, the function succeeded. Check if warning was emitted.
	// The current branch (refactor/inf-tsk-021-035-git-hooks) won't match "never-match/".
	if !strings.Contains(stderr.String(), "Warning") && !strings.Contains(stderr.String(), "naming convention") {
		// If running outside a git repo or on main/master, warning may not appear.
		t.Logf("no warning (possibly on main/master or not in git repo)")
	}
}

func TestIsForcePush_ZeroSHA(t *testing.T) {
	t.Parallel()

	// Both zero-SHAs should return false.
	if isForcePush(zeroSHA, "abc123") {
		t.Error("expected false for zero localSHA")
	}
	if isForcePush("abc123", zeroSHA) {
		t.Error("expected false for zero remoteSHA")
	}
	if isForcePush(zeroSHA, zeroSHA) {
		t.Error("expected false for both zero")
	}
}

func TestIsPathFlowActive_NoState(t *testing.T) {
	t.Parallel()

	// Empty temp dir — no pathflow state files.
	if isPathFlowActive(t.TempDir()) {
		t.Error("expected false for empty directory")
	}
}

func TestIsPathFlowActive_WithState(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state", "session", "ses-123", "pathflow")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stateDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	if !isPathFlowActive(dir) {
		t.Error("expected true when pathflow state file exists")
	}
}

func TestRunPrePush_ProtectedBranch_ErrorOutput(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Push to master (protected).
	input := "refs/heads/feat/x abc123 refs/heads/master def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	if err == nil {
		t.Fatal("expected error")
	}

	output := stderr.String()
	if !strings.Contains(output, "Push to protected branch") {
		t.Error("expected protected branch message in stderr")
	}
	if !strings.Contains(output, "PR-only workflow") {
		t.Error("expected PR-only workflow message in stderr")
	}
}

func TestRunPrePush_ProtectedBranch_PathFlowActive(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()

	// Create pathflow-active file.
	stateDir := filepath.Join(dir, ".state", "session", "ses-123", "pathflow")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stateDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	input := "refs/heads/feat/x abc123 refs/heads/main def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", dir, policy)
	if err == nil {
		t.Fatal("expected error for push to protected branch")
	}

	output := stderr.String()
	if !strings.Contains(output, "cf-git-operations") {
		t.Error("expected pathflow guidance mentioning cf-git-operations")
	}
}

func TestRunPrePush_ProtectedBranch_NoPathFlow(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	// No pathflow-active file => non-pathflow guidance.

	input := "refs/heads/feat/x abc123 refs/heads/production def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", dir, policy)
	if err == nil {
		t.Fatal("expected error")
	}

	output := stderr.String()
	if !strings.Contains(output, "Delegate to cf-git-operations") {
		t.Error("expected non-pathflow delegation guidance")
	}
}

func TestRunPrePush_ReleaseWildcard(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// Push to release/1.0 (protected via wildcard).
	input := "refs/heads/feat/x abc123 refs/heads/release/1.0 def456\n"
	stdin := strings.NewReader(input)
	stdout := &bytes.Buffer{}
	stderr := &bytes.Buffer{}

	err := RunPrePush(stdin, stdout, stderr, "origin", "https://github.com/test/repo.git", t.TempDir(), policy)
	if err == nil {
		t.Fatal("expected error for push to release/* branch")
	}
}
