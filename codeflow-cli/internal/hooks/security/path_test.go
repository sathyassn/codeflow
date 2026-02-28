package security

import (
	"testing"
)

func TestPathModule(t *testing.T) {
	t.Parallel()

	mod := &PathModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"rm settings", "rm .claude/settings.json"},
		{"mv settings", "mv .claude/settings.json /tmp/backup"},
		{"rm hooks", "rm -rf .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"chmod settings", "chmod 777 .claude/settings.json"},
		{"git rm config", "git rm .codeflow/config/enforcement/policy.json"},
		{"rm .claude dir", "rm -rf .claude"},
		{"rm .codeflow dir", "rm -rf .codeflow"},
		{"redirect to settings", "echo x > .claude/settings.json"},
		{"append to settings", "echo x >> .claude/settings.json"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
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
		{"read settings", "cat .claude/settings.json"},
		{"rm unprotected", "rm some-file.txt"},
		{"bash hook", "bash .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"ls config", "ls .codeflow/config/"},
		{"cp to tmp", "cp .claude/settings.json /tmp/claude/backup.json"},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q, got block: %s", tt.cmd, v.Reason)
			}
		})
	}
}

// TestPathModuleDirectoryProtection exercises the pre-compiled
// protectedDirPatterns for .claude and .codeflow directory-level detection.
func TestPathModuleDirectoryProtection(t *testing.T) {
	t.Parallel()

	mod := &PathModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"rm .claude with quotes", `rm ".claude"`},
		{"rm .codeflow trailing slash", "rm -rf .codeflow/"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q, got allow", tt.cmd)
			}
		})
	}
}
