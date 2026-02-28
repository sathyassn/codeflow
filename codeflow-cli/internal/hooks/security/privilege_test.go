package security

import (
	"testing"
)

func TestPrivilegeModule(t *testing.T) {
	t.Parallel()

	mod := &PrivilegeModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"sudo", "sudo rm -rf /tmp"},
		{"su", "su root"},
		{"doas", "doas apt install"},
		{"pkexec", "pkexec visudo"},
		{"runuser", "runuser -u user cmd"},
		{"chained sudo", "ls && sudo rm file"},
		{"semicolon sudo", "ls; sudo rm file"},
		{"piped sudo", "echo pass | sudo -S rm"},
		{"bash -c", `bash -c "echo hello"`},
		{"sh -c", `sh -c "rm -rf /"`},
		{"zsh -c", `zsh -c 'whoami'`},
		{"eval dangerous", "eval sudo rm -rf /"},
		{"source", "source /etc/profile"},
		{"dot source", ". /etc/profile"},
		{"LD_PRELOAD", "LD_PRELOAD=/tmp/evil.so cmd"},
		{"LD_LIBRARY_PATH", "LD_LIBRARY_PATH=/tmp cmd"},
		{"PATH tmp", "PATH=/usr/bin:/tmp cmd"},
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
		{"normal cmd", "ls -la"},
		{"git status", "git status"},
		{"echo", "echo hello"},
		{"eval safe", "eval echo hello"},
		{"bash script", "bash script.sh"},
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

// TestPrivilegeModulePipePatterns exercises the pre-compiled privPipePatterns
// for all privilege escalation commands piped from other commands.
func TestPrivilegeModulePipePatterns(t *testing.T) {
	t.Parallel()

	mod := &PrivilegeModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"pipe to doas", "echo password | doas rm -rf /"},
		{"pipe to pkexec", "echo y | pkexec visudo"},
		{"pipe to runuser", "cat creds | runuser -u root cmd"},
		{"pipe to su", "echo password | su root"},
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
