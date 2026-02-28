package security

import (
	"regexp"
)

// NetworkModule blocks network operations without sandbox bypass.
// Go equivalent of cf-network-protection.sh.
type NetworkModule struct{}

// Name returns the module name.
func (m *NetworkModule) Name() string { return "network-protection" }

// Default network operation patterns (used when config is unavailable).
var (
	defaultGitNetworkPatterns = []string{
		`^git\s+(push|pull|fetch|clone)`,
		`^git\s+remote\s+update`,
		`^git\s+ls-remote`,
	}
	defaultGHCLIPatterns = []string{
		`^gh\s+(pr|issue|release|api|workflow|run|repo|gist)\s`,
	}
)

// Check evaluates the command for network operation violations.
func (m *NetworkModule) Check(ctx *CheckContext) *Verdict {
	// If sandbox bypass is enabled, network operations are allowed
	if ctx.SandboxBypass {
		return nil
	}

	cmd := ctx.Command

	// Check git network operations
	gitPatterns := defaultGitNetworkPatterns
	if ctx.Policy != nil && len(ctx.Policy.NetworkOperations.GitNetwork.Patterns) > 0 {
		gitPatterns = ctx.Policy.NetworkOperations.GitNetwork.Patterns
	}

	if matchesAnyPattern(cmd, gitPatterns) {
		reason := "Git network operation requires sandbox bypass (dangerouslyDisableSandbox: true)."
		if ctx.IsPathFlowActive {
			reason += " In PathFlow mode, delegate to cf-git-operations teammate."
		} else {
			reason += " Delegate to cf-security teammate for sandbox-check, then cf-git-operations for sync-remote."
		}
		return block("Network Operation", reason, "git network")
	}

	// Check GitHub CLI operations
	ghPatterns := defaultGHCLIPatterns
	if ctx.Policy != nil && len(ctx.Policy.NetworkOperations.GitHubCLI.Patterns) > 0 {
		ghPatterns = ctx.Policy.NetworkOperations.GitHubCLI.Patterns
	}

	if matchesAnyPattern(cmd, ghPatterns) {
		return block("Network Operation",
			"GitHub CLI requires sandbox bypass (dangerouslyDisableSandbox: true)",
			"gh cli")
	}

	return nil
}

// matchesAnyPattern checks if the command matches any of the given regex patterns.
func matchesAnyPattern(cmd string, patterns []string) bool {
	for _, p := range patterns {
		re, err := regexp.Compile(p)
		if err != nil {
			continue
		}
		if re.MatchString(cmd) {
			return true
		}
	}
	return false
}
