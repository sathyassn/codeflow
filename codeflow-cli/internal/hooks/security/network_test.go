package security

import (
	"strings"
	"testing"
)

func TestNetworkModule(t *testing.T) {
	t.Parallel()

	mod := &NetworkModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"git push", "git push origin main"},
		{"git pull", "git pull origin main"},
		{"git fetch", "git fetch --all"},
		{"git clone", "git clone https://github.com/x/y"},
		{"git remote update", "git remote update"},
		{"git ls-remote", "git ls-remote origin"},
		{"gh pr create", "gh pr create --title test"},
		{"gh issue list", "gh issue list"},
		{"gh api", "gh api repos/x/y"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy, SandboxBypass: false}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q without sandbox bypass, got allow", tt.cmd)
			}
		})
	}

	allowed := []struct {
		name    string
		cmd     string
		sandbox bool
	}{
		{"git push with bypass", "git push origin main", true},
		{"gh pr with bypass", "gh pr create --title test", true},
		{"git status", "git status", false},
		{"git commit", "git commit -m 'msg'", false},
		{"git log", "git log --oneline", false},
		{"echo", "echo hello", false},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy, SandboxBypass: tt.sandbox}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q (sandbox=%v), got block: %s", tt.cmd, tt.sandbox, v.Reason)
			}
		})
	}
}

func TestNetworkModuleExtended(t *testing.T) {
	t.Parallel()

	mod := &NetworkModule{}

	t.Run("config-driven git patterns", func(t *testing.T) {
		t.Parallel()
		policy := &EnforcementPolicy{}
		policy.NetworkOperations.GitNetwork.Patterns = []string{`^git\s+push`}
		ctx := &CheckContext{Command: "git push origin main", Policy: policy}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Error("expected block for git push with config pattern")
		}
	})

	t.Run("config-driven gh patterns", func(t *testing.T) {
		t.Parallel()
		policy := &EnforcementPolicy{}
		policy.NetworkOperations.GitHubCLI.Patterns = []string{`^gh\s+pr\s`}
		ctx := &CheckContext{Command: "gh pr create", Policy: policy}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Error("expected block for gh pr with config pattern")
		}
	})

	t.Run("pathflow active message", func(t *testing.T) {
		t.Parallel()
		policy := DefaultPolicy()
		ctx := &CheckContext{
			Command:          "git push origin main",
			IsPathFlowActive: true,
			Policy:           policy,
		}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Fatal("expected block for git push without bypass")
		}
		if !strings.Contains(v.Reason, "cf-git-operations") {
			t.Errorf("reason should mention cf-git-operations for PathFlow mode, got: %s", v.Reason)
		}
	})

	t.Run("non-pathflow message", func(t *testing.T) {
		t.Parallel()
		policy := DefaultPolicy()
		ctx := &CheckContext{
			Command:          "git push origin main",
			IsPathFlowActive: false,
			Policy:           policy,
		}
		v := mod.Check(ctx)
		if v == nil || v.Allow {
			t.Fatal("expected block for git push without bypass")
		}
		if !strings.Contains(v.Reason, "cf-security") {
			t.Errorf("reason should mention cf-security for non-PathFlow mode, got: %s", v.Reason)
		}
	})

	t.Run("invalid regex pattern skipped", func(t *testing.T) {
		t.Parallel()
		// matchesAnyPattern should skip invalid regex without crashing
		result := matchesAnyPattern("git push origin", []string{`[invalid`})
		if result {
			t.Error("expected false for invalid regex pattern")
		}
	})
}
