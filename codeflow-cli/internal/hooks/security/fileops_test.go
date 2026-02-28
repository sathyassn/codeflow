package security

import (
	"testing"
)

func TestFileOpsModule(t *testing.T) {
	t.Parallel()

	mod := &FileOpsModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"cp to settings", "cp /tmp/x .claude/settings.json"},
		{"tee to hooks", "echo x | tee .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"interpreter python", "python3 -c 'open(\".claude/settings.json\",\"w\")'"},
		{"interpreter perl", "perl -e 'write .state/data'"},
		{"interpreter node", "node -e 'fs.write(\".codeflow/config/x\")'"},
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
		{"cp to tmp", "cp .claude/settings.json /tmp/claude/backup"},
		{"tee to regular", "echo x | tee output.txt"},
		{"python safe", "python3 -c 'print(1)'"},
		{"ls", "ls -la"},
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

func TestFileOpsModuleExtended(t *testing.T) {
	t.Parallel()

	mod := &FileOpsModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"dd to settings", "dd if=/dev/urandom of=.claude/settings.json bs=1k count=1"},
		{"rsync to hooks", "rsync /tmp/x .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"scp to config", "scp user@host:/file .codeflow/config/enforcement/policy.json"},
		{"install to hooks", "install -m 755 /tmp/x .claude/hooks/codeflow/script.sh"},
		{"ln to settings", "ln -s /tmp/x .claude/settings.json"},
		{"piped tee to hooks", "cat /tmp/data | tee .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"cat append to settings", "cat /tmp/data >> .claude/settings.json"},
		{"glob ? bypass", "cp /tmp/x .claude/settings.jso?"},
		{"glob * bypass", "cp /tmp/x .claude/settings.*"},
		{"interpreter ruby", "ruby -e 'File.write(\".claude/settings.json\", \"x\")'"},
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
		{"cp to tmp claude final dest", "cp .claude/settings.json /tmp/claude/backup"},
		{"redirect to tmp claude", "echo x >/tmp/claude/test.json"},
		{"dd safe operation", "dd if=/dev/zero of=/tmp/output bs=1k count=1"},
		{"no protected path", "cp /tmp/a /tmp/b"},
		{"nil policy", "ls -la"},
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

	t.Run("nil policy returns nil", func(t *testing.T) {
		t.Parallel()
		emptyPolicy := &EnforcementPolicy{}
		ctx := &CheckContext{Command: "cp /tmp/a /tmp/b", Policy: emptyPolicy}
		v := mod.Check(ctx)
		if v != nil && !v.Allow {
			t.Errorf("expected allow with empty policy, got block: %s", v.Reason)
		}
	})
}

// TestFileOpsModuleHoistedPatterns exercises the package-level patterns
// (tmpClaudeFinalDest, tmpClaudeRedirect, ddCmd, pipedTee, catAppend).
func TestFileOpsModuleHoistedPatterns(t *testing.T) {
	t.Parallel()

	mod := &FileOpsModule{}
	policy := DefaultPolicy()

	t.Run("dd to protected path blocked", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "dd if=/dev/zero of=.claude/settings.json", Policy: policy}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Error("expected block for dd to protected path")
		}
	})

	t.Run("piped tee to tmp claude allowed", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "echo test | tee /tmp/claude/backup.txt", Policy: policy}
		v := mod.Check(ctx)
		if v != nil && !v.Allow {
			t.Errorf("expected allow for tee to /tmp/claude, got block: %s", v.Reason)
		}
	})

	t.Run("cat append to unprotected allowed", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "cat data.txt >> output.log", Policy: policy}
		v := mod.Check(ctx)
		if v != nil && !v.Allow {
			t.Errorf("expected allow for cat append to unprotected path, got block: %s", v.Reason)
		}
	})
}
