package security

import (
	"testing"
)

func TestTmpModule(t *testing.T) {
	t.Parallel()

	mod := &TmpModule{}

	// Create a policy with managed tmp folders
	policy := &EnforcementPolicy{}
	policy.ManagedTmp.ProtectedFolders = []string{
		"/tmp/claude/testproj/managed",
		"/tmp/claude/testproj/managed/protected-edits",
		"/tmp/claude/testproj/managed/state",
	}
	policy.ManagedTmp.StateFolder = "/tmp/claude/testproj/managed/state"

	blocked := []struct {
		name string
		cmd  string
	}{
		{"rm managed", "rm /tmp/claude/testproj/managed"},
		{"rmdir managed", "rmdir /tmp/claude/testproj/managed"},
		{"mv managed", "mv /tmp/claude/testproj/managed /tmp/other"},
		{"rm -rf managed", "rm -rf /tmp/claude/testproj/managed"},
		{"rm state files", "rm /tmp/claude/testproj/managed/state/data.json"},
		{"unlink state", "unlink /tmp/claude/testproj/managed/state/x"},
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
		{"create in managed", "mkdir -p /tmp/claude/testproj/managed/x"},
		{"write state", "echo data > /tmp/claude/testproj/managed/state/new.json"},
		{"rm other tmp", "rm /tmp/other-file"},
		{"ls managed", "ls /tmp/claude/testproj/managed"},
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

// TestTmpModuleDynamicPatterns exercises the dynamically compiled regexp
// patterns for managed folder protection with different policy configurations.
func TestTmpModuleDynamicPatterns(t *testing.T) {
	t.Parallel()

	mod := &TmpModule{}

	t.Run("custom managed folder blocked", func(t *testing.T) {
		t.Parallel()
		policy := &EnforcementPolicy{}
		policy.ManagedTmp.ProtectedFolders = []string{"/tmp/claude/myapp/managed"}
		ctx := &CheckContext{Command: "rm -rf /tmp/claude/myapp/managed", Policy: policy}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Error("expected block for rm -rf on custom managed folder")
		}
	})

	t.Run("nil policy uses defaults", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "rm /tmp/claude/codeflow/managed", Policy: nil}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Error("expected block for rm on default managed folder with nil policy")
		}
	})
}
