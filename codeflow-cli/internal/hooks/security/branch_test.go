package security

import (
	"testing"
)

func TestBranchModule(t *testing.T) {
	t.Parallel()

	mod := &BranchModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name   string
		cmd    string
		branch string
	}{
		{"redirect on main", "echo x > file.txt", "main"},
		{"append on main", "echo x >> file.txt", "main"},
		{"sed -i on main", "sed -i '' 's/a/b/' file.txt", "main"},
		{"touch on main", "touch newfile.txt", "main"},
		{"tee on main", "echo x | tee output.txt", "main"},
		{"cp relative on master", "cp /tmp/x file.txt", "master"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: tt.branch, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q on %q, got allow", tt.cmd, tt.branch)
			}
		})
	}

	allowed := []struct {
		name   string
		cmd    string
		branch string
	}{
		{"redirect on feature", "echo x > file.txt", "feat/x"},
		{"cp on feature", "cp file1 file2", "feat/x"},
		{"tmp claude on main", "cp x /tmp/claude/backup", "main"},
		{"cp absolute on main", "cp /tmp/x /tmp/y", "main"},
		{"touch tmp on main", "touch /tmp/file", "main"},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: tt.branch, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q on %q, got block: %s", tt.cmd, tt.branch, v.Reason)
			}
		})
	}
}

// TestBranchModuleTmpExclusions verifies the package-level tmp exclusion
// patterns (tmpRedirect, tmpAppend, touchTmp, teeTmp) correctly skip
// /tmp/-targeted operations on protected branches.
func TestBranchModuleTmpExclusions(t *testing.T) {
	t.Parallel()

	mod := &BranchModule{}
	policy := DefaultPolicy()

	allowed := []struct {
		name   string
		cmd    string
		branch string
	}{
		{"redirect to tmp on main", "echo x > /tmp/output.txt", "main"},
		{"append to tmp on main", "echo x >> /tmp/log.txt", "main"},
		{"tee to tmp on main", "echo x | tee /tmp/out.txt", "main"},
		{"redirect to tmp on production", "echo x > /tmp/debug.log", "production"},
	}

	for _, tt := range allowed {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, CurrentBranch: tt.branch, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q on %q, got block: %s", tt.cmd, tt.branch, v.Reason)
			}
		})
	}
}
