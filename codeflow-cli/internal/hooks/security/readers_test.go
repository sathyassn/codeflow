package security

import (
	"os"
	"path/filepath"
	"testing"
)

func TestAllProtectedPaths(t *testing.T) {
	t.Parallel()

	p := &EnforcementPolicy{}
	p.ProtectedResources.Critical = []string{"a"}
	p.ProtectedResources.High = []string{"b", "c"}
	p.ProtectedResources.Moderate = []string{"d"}

	got := p.AllProtectedPaths()
	if len(got) != 4 {
		t.Errorf("got %d paths, want 4", len(got))
	}
}

func TestProtectedBranchList(t *testing.T) {
	t.Parallel()

	t.Run("configured", func(t *testing.T) {
		t.Parallel()
		p := &EnforcementPolicy{ProtectedBranches: []string{"main"}}
		got := p.ProtectedBranchList()
		if len(got) != 1 || got[0] != "main" {
			t.Errorf("got %v, want [main]", got)
		}
	})

	t.Run("defaults", func(t *testing.T) {
		t.Parallel()
		p := &EnforcementPolicy{}
		got := p.ProtectedBranchList()
		if len(got) != 4 {
			t.Errorf("got %d branches, want 4 defaults", len(got))
		}
	})
}

func TestManagedTmpFolders(t *testing.T) {
	t.Parallel()

	t.Run("expanded from config", func(t *testing.T) {
		t.Parallel()
		p := &EnforcementPolicy{}
		p.ManagedTmp.ProtectedFolders = []string{"/tmp/claude/${CF_PROJECT_ROOT}/managed"}
		got := p.ManagedTmpFolders("myproject")
		if len(got) != 1 || got[0] != "/tmp/claude/myproject/managed" {
			t.Errorf("got %v", got)
		}
	})

	t.Run("defaults", func(t *testing.T) {
		t.Parallel()
		p := &EnforcementPolicy{}
		got := p.ManagedTmpFolders("testproj")
		if len(got) != 3 {
			t.Errorf("got %d folders, want 3", len(got))
		}
	})
}

func TestReadEnforcementPolicy(t *testing.T) {
	t.Parallel()

	t.Run("valid policy file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		policyDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(policyDir, 0o755); err != nil {
			t.Fatal(err)
		}
		content := `{
			"protected_resources": {
				"critical": [".claude/settings.json"],
				"high": [".github/**"],
				"moderate": ["project/mission.md"]
			},
			"protected_branches": ["main", "release/*"],
			"network_operations": {
				"git_network": {"patterns": ["^git\\s+push"]},
				"github_cli": {"patterns": ["^gh\\s+pr"]}
			},
			"managed_tmp": {
				"protected_folders": ["/tmp/claude/${CF_PROJECT_ROOT}/managed"],
				"state_folder": "/tmp/claude/${CF_PROJECT_ROOT}/managed/state"
			}
		}`
		if err := os.WriteFile(filepath.Join(policyDir, "enforcement-policy.json"), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}

		policy, err := ReadEnforcementPolicy(dir)
		if err != nil {
			t.Fatalf("ReadEnforcementPolicy() error: %v", err)
		}
		if len(policy.ProtectedResources.Critical) != 1 {
			t.Errorf("critical paths = %d, want 1", len(policy.ProtectedResources.Critical))
		}
		if policy.ProtectedResources.Critical[0] != ".claude/settings.json" {
			t.Errorf("critical[0] = %q, want .claude/settings.json", policy.ProtectedResources.Critical[0])
		}
		if len(policy.ProtectedBranches) != 2 {
			t.Errorf("branches = %d, want 2", len(policy.ProtectedBranches))
		}
		if len(policy.NetworkOperations.GitNetwork.Patterns) != 1 {
			t.Errorf("git network patterns = %d, want 1", len(policy.NetworkOperations.GitNetwork.Patterns))
		}
	})

	t.Run("missing file", func(t *testing.T) {
		t.Parallel()
		_, err := ReadEnforcementPolicy(t.TempDir())
		if err == nil {
			t.Error("ReadEnforcementPolicy() expected error for missing file")
		}
	})

	t.Run("invalid json", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		policyDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(policyDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(policyDir, "enforcement-policy.json"), []byte("not json"), 0o644); err != nil {
			t.Fatal(err)
		}

		_, err := ReadEnforcementPolicy(dir)
		if err == nil {
			t.Error("ReadEnforcementPolicy() expected error for invalid JSON")
		}
	})
}
