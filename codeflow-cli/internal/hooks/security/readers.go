package security

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// EnforcementPolicy represents the parsed enforcement-policy.json.
type EnforcementPolicy struct {
	ProtectedResources struct {
		Critical []string `json:"critical"`
		High     []string `json:"high"`
		Moderate []string `json:"moderate"`
	} `json:"protected_resources"`

	ProtectedBranches []string `json:"protected_branches"`

	NetworkOperations struct {
		GitNetwork struct {
			Patterns []string `json:"patterns"`
		} `json:"git_network"`
		GitHubCLI struct {
			Patterns []string `json:"patterns"`
		} `json:"github_cli"`
	} `json:"network_operations"`

	ManagedTmp struct {
		ProtectedFolders []string `json:"protected_folders"`
		StateFolder      string   `json:"state_folder"`
	} `json:"managed_tmp"`

}

// AllProtectedPaths returns all protected paths (critical + high + moderate).
func (p *EnforcementPolicy) AllProtectedPaths() []string {
	var paths []string
	paths = append(paths, p.ProtectedResources.Critical...)
	paths = append(paths, p.ProtectedResources.High...)
	paths = append(paths, p.ProtectedResources.Moderate...)
	return paths
}

// ProtectedBranchList returns the protected branches list, falling back
// to defaults if none configured.
func (p *EnforcementPolicy) ProtectedBranchList() []string {
	if len(p.ProtectedBranches) > 0 {
		return p.ProtectedBranches
	}
	return []string{"main", "master", "release/*", "production"}
}

// ManagedTmpFolders returns the managed tmp folders, substituting
// the CF_PROJECT_ROOT environment variable.
func (p *EnforcementPolicy) ManagedTmpFolders(projectRoot string) []string {
	if len(p.ManagedTmp.ProtectedFolders) == 0 {
		return defaultManagedTmpFolders(projectRoot)
	}

	folders := make([]string, len(p.ManagedTmp.ProtectedFolders))
	for i, f := range p.ManagedTmp.ProtectedFolders {
		folders[i] = expandProjectRoot(f, projectRoot)
	}
	return folders
}

// StateFolderPath returns the state folder path with variable expansion.
func (p *EnforcementPolicy) StateFolderPath(projectRoot string) string {
	if p.ManagedTmp.StateFolder == "" {
		return fmt.Sprintf("/tmp/claude/%s/managed/state", projectRoot)
	}
	return expandProjectRoot(p.ManagedTmp.StateFolder, projectRoot)
}

// ReadEnforcementPolicy reads and parses the enforcement-policy.json file.
func ReadEnforcementPolicy(projectDir string) (*EnforcementPolicy, error) {
	path := filepath.Join(projectDir, ".codeflow", "config", "enforcement", "enforcement-policy.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read enforcement policy: %w", err)
	}

	var policy EnforcementPolicy
	if err := json.Unmarshal(data, &policy); err != nil {
		return nil, fmt.Errorf("parse enforcement policy: %w", err)
	}

	return &policy, nil
}

// DefaultPolicy returns a minimal policy with default protected paths
// and branches for use when the config file is unavailable.
func DefaultPolicy() *EnforcementPolicy {
	p := &EnforcementPolicy{}
	p.ProtectedResources.Critical = []string{
		".claude/settings.json",
		".claude/settings.local.json",
		".claude/CLAUDE.md",
	}
	p.ProtectedResources.High = []string{
		".claude/hooks/codeflow/**",
		".claude/settings-templates/**",
		".codeflow/config/**",
		".codeflow/scripts/security/**",
		".codeflow/scripts/git-hooks/**",
		".codeflow/scripts/shell-lib/**",
		".github/workflows/**",
		".github/**",
		".git/hooks/**",
		".state/sentinels/**",
		".state/session/**",
	}
	p.ProtectedResources.Moderate = []string{
		"project/mission.md",
		"project/tech-stack/**",
	}
	p.ProtectedBranches = []string{"main", "master", "release/*", "production"}
	return p
}

func expandProjectRoot(s, projectRoot string) string {
	return strings.ReplaceAll(s, "${CF_PROJECT_ROOT}", projectRoot)
}

func defaultManagedTmpFolders(projectRoot string) []string {
	return []string{
		fmt.Sprintf("/tmp/claude/%s/managed", projectRoot),
		fmt.Sprintf("/tmp/claude/%s/managed/protected-edits", projectRoot),
		fmt.Sprintf("/tmp/claude/%s/managed/state", projectRoot),
	}
}
