package security

import (
	"testing"
)

func TestGitModule(t *testing.T) {
	t.Parallel()

	mod := &GitModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name   string
		cmd    string
		branch string
	}{
		{"no-verify commit", "git commit --no-verify", "feat/x"},
		{"no-verify push", "git push --no-verify", "feat/x"},
		{"no-verify at git level", "git --no-verify commit", "feat/x"},
		{"commit -n", "git commit -n", "feat/x"},
		{"commit -nm", "git commit -nm 'msg'", "feat/x"},
		{"force push protected", "git push --force", "main"},
		{"force-with-lease protected", "git push --force-with-lease", "main"},
		{"push -f protected", "git push origin -f", "master"},
		{"config hooksPath", "git config core.hooksPath /tmp", "feat/x"},
		{"git -c hooksPath", "git -c core.hooksPath=/tmp commit", "feat/x"},
		{"unset hooksPath", "git config --unset core.hooksPath", "feat/x"},
		{"GIT_HOOKS_PATH", "GIT_HOOKS_PATH=/x git commit", "feat/x"},
		{"SKIP_HOOKS", "SKIP_HOOKS=1 git commit", "feat/x"},
		{"GIT_SKIP_HOOKS", "GIT_SKIP_HOOKS=1 git commit", "feat/x"},
		{"HUSKY=0", "HUSKY=0 git commit", "feat/x"},
		{"PRE_COMMIT_ALLOW_NO_CONFIG", "PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit", "feat/x"},
		{"modify .git/hooks", "rm .git/hooks/pre-commit", "feat/x"},
		{"redirect .git/hooks", "> .git/hooks/pre-commit", "feat/x"},
		{"merge on main", "git merge feat/x", "main"},
		{"cherry-pick on main", "git cherry-pick abc123", "main"},
		{"rebase on main", "git rebase feat/x", "main"},
		{"reset on main", "git reset HEAD~1", "main"},
		{"checkout merge", "git checkout main && git merge feat/x", "feat/x"},
		{"switch merge", "git switch main && git merge feat/x", "feat/x"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: tt.branch, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q on branch %q, got allow", tt.cmd, tt.branch)
			}
		})
	}

	allowed := []struct {
		name   string
		cmd    string
		branch string
	}{
		{"normal commit", "git commit -m 'fix: bug'", "feat/x"},
		{"push feature", "git push origin feat/x", "feat/x"},
		{"push -n (dry run)", "git push -n", "feat/x"},
		{"force push feature", "git push --force", "feat/x"},
		{"merge on feature", "git merge main", "feat/x"},
		{"rebase on feature", "git rebase main", "feat/x"},
		{"reset on feature", "git reset HEAD~1", "feat/x"},
		{"git status", "git status", "main"},
		{"git log", "git log --oneline", "main"},
		{"git diff", "git diff", "main"},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: tt.branch, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q on branch %q, got block: %s", tt.cmd, tt.branch, v.Reason)
			}
		})
	}
}

// TestGitModuleHookBypassFlags exercises the package-level standaloneN and
// combinedN patterns for git commit -n flag detection.
func TestGitModuleHookBypassFlags(t *testing.T) {
	t.Parallel()

	mod := &GitModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"commit -an", "git commit -an"},
		{"commit -anm", "git commit -anm 'quick'"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: "feat/x", Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q, got allow", tt.cmd)
			}
		})
	}

	allowed := []struct {
		name string
		cmd  string
	}{
		{"commit -a (no n)", "git commit -a -m 'msg'"},
		{"commit -am (no n)", "git commit -am 'msg'"},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: "feat/x", Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q, got block: %s", tt.cmd, v.Reason)
			}
		})
	}
}

func TestIsOnProtectedBranch(t *testing.T) {
	t.Parallel()

	policy := DefaultPolicy()

	tests := []struct {
		branch    string
		protected bool
	}{
		{"main", true},
		{"master", true},
		{"production", true},
		{"release/1.0", true},
		{"release/2.0.1", true},
		{"feat/new-feature", false},
		{"fix/bug", false},
		{"develop", false},
		{"", false},
	}

	for _, tt := range tests {
		t.Run(tt.branch, func(t *testing.T) {
			t.Parallel()
			got := isOnProtectedBranch(tt.branch, policy)
			if got != tt.protected {
				t.Errorf("isOnProtectedBranch(%q) = %v, want %v", tt.branch, got, tt.protected)
			}
		})
	}
}
