package security

import (
	"strings"
	"testing"
)

func TestCheckerIntegration(t *testing.T) {
	t.Parallel()

	checker := NewChecker()
	policy := DefaultPolicy()

	t.Run("dangerous blocked", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "rm -rf /", Policy: policy}
		v := checker.Check(ctx)
		if v.Allow {
			t.Error("expected block for rm -rf /")
		}
		if v.Module != "dangerous-commands" {
			t.Errorf("module = %q, want dangerous-commands", v.Module)
		}
	})

	t.Run("privilege blocked", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "sudo apt install", Policy: policy}
		v := checker.Check(ctx)
		if v.Allow {
			t.Error("expected block for sudo")
		}
		if v.Module != "privilege-protection" {
			t.Errorf("module = %q, want privilege-protection", v.Module)
		}
	})

	t.Run("safe command allowed", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "ls -la", CurrentBranch: "feat/x", Policy: policy}
		v := checker.Check(ctx)
		if !v.Allow {
			t.Errorf("expected allow for ls -la, got block: %s (%s)", v.Reason, v.Module)
		}
	})

	t.Run("git status allowed", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "git status", CurrentBranch: "main", Policy: policy}
		v := checker.Check(ctx)
		if !v.Allow {
			t.Errorf("expected allow for git status, got block: %s (%s)", v.Reason, v.Module)
		}
	})

	t.Run("network with bypass allowed", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{
			Command:       "git push origin feat/x",
			CurrentBranch: "feat/x",
			SandboxBypass: true,
			Policy:        policy,
		}
		v := checker.Check(ctx)
		if !v.Allow {
			t.Errorf("expected allow for git push with bypass, got block: %s", v.Reason)
		}
	})

	t.Run("network without bypass blocked", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{
			Command:       "git push origin feat/x",
			CurrentBranch: "feat/x",
			SandboxBypass: false,
			Policy:        policy,
		}
		v := checker.Check(ctx)
		if v.Allow {
			t.Error("expected block for git push without bypass")
		}
	})
}

func TestVerdictMessage(t *testing.T) {
	t.Parallel()

	t.Run("allow returns empty", func(t *testing.T) {
		t.Parallel()
		v := &Verdict{Allow: true}
		if msg := v.Message(); msg != "" {
			t.Errorf("expected empty message for allow, got %q", msg)
		}
	})

	t.Run("block returns formatted message", func(t *testing.T) {
		t.Parallel()
		v := &Verdict{
			Allow:    false,
			Category: "Test Category",
			Reason:   "Test reason",
			Pattern:  "test-pattern",
		}
		msg := v.Message()
		if !strings.Contains(msg, "BLOCKED") {
			t.Error("message should contain BLOCKED")
		}
		if !strings.Contains(msg, "Test Category") {
			t.Error("message should contain category")
		}
		if !strings.Contains(msg, "Test reason") {
			t.Error("message should contain reason")
		}
		if !strings.Contains(msg, "test-pattern") {
			t.Error("message should contain pattern")
		}
	})
}

func TestDefaultModules(t *testing.T) {
	t.Parallel()

	modules := DefaultModules()
	if len(modules) != 8 {
		t.Errorf("expected 8 default modules, got %d", len(modules))
	}

	// Verify execution order
	expectedNames := []string{
		"dangerous-commands",
		"privilege-protection",
		"git-protection",
		"path-protection",
		"file-operations",
		"branch-file-protection",
		"tmp-protection",
		"network-protection",
	}
	for i, name := range expectedNames {
		if modules[i].Name() != name {
			t.Errorf("module[%d].Name() = %q, want %q", i, modules[i].Name(), name)
		}
	}
}

func TestEdgeCases(t *testing.T) {
	t.Parallel()

	checker := NewChecker()
	policy := DefaultPolicy()

	t.Run("empty command", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "", Policy: policy}
		v := checker.Check(ctx)
		if !v.Allow {
			t.Errorf("empty command should be allowed, got block: %s", v.Reason)
		}
	})

	t.Run("compound safe+dangerous segments path check", func(t *testing.T) {
		t.Parallel()
		// bash script.sh && rm /tmp/file — should NOT be blocked
		// because bash execution is allowed and rm /tmp/file is safe
		ctx := &CheckContext{
			Command:       "bash .claude/hooks/codeflow/pre-tool-use/script.sh && rm /tmp/file",
			CurrentBranch: "feat/x",
			Policy:        policy,
		}
		v := checker.Check(ctx)
		if !v.Allow {
			t.Errorf("expected allow for safe compound command, got block: %s (%s)", v.Reason, v.Module)
		}
	})

	t.Run("nil policy uses defaults", func(t *testing.T) {
		t.Parallel()
		ctx := &CheckContext{Command: "rm -rf /", Policy: nil}
		v := checker.Check(ctx)
		if v.Allow {
			t.Error("dangerous command should be blocked even with nil policy")
		}
	})
}
