package ghpr

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// Verdict represents the result of a PR merge protection check.
type Verdict struct {
	// Allow is true if the operation is permitted.
	Allow bool

	// Reason is a human-readable explanation of why the operation was blocked.
	Reason string
}

// PRChecker validates `gh pr merge` commands against a list of protected branches.
type PRChecker struct {
	// ProtectedBranches is the list of protected branch patterns.
	// Supports exact matches (e.g., "main") and glob wildcards (e.g., "release/*").
	ProtectedBranches []string
}

// hookInput represents the JSON structure sent by Claude Code on stdin
// to PreToolUse hooks.
type hookInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// bashInput represents the tool_input for Bash tool calls.
type bashInput struct {
	Command string `json:"command"`
}

// mergeProtectionConfig represents the merge_protection section of enforcement-policy.json.
type mergeProtectionConfig struct {
	ProtectedBranches []string `json:"protected_branches"`
}

// enforcementPolicy represents the top-level enforcement-policy.json structure
// (only the fields we need).
type enforcementPolicy struct {
	MergeProtection mergeProtectionConfig `json:"merge_protection"`
}

// DefaultProtectedBranches returns the default set of protected branches
// used when enforcement-policy.json is unavailable.
var DefaultProtectedBranches = []string{"main", "master", "release/*", "production"}

// ghPRMergeRe matches `gh pr merge` commands, accounting for leading
// operators (&&, |, ;) or start of string.
var ghPRMergeRe = regexp.MustCompile(`(?:^|\s|&&|\|)gh\s+pr\s+merge(?:\s|$)`)

// prNumberRe extracts the PR number from a `gh pr merge <number>` command.
var prNumberRe = regexp.MustCompile(`gh\s+pr\s+merge\s+(\d+)`)

// Check reads Claude Code hook JSON from stdin, extracts the command,
// and checks whether it is a `gh pr merge` targeting a protected branch.
//
// The checker requires a PRResolver to look up the target branch for a PR number.
// In production, this calls `gh pr view`; in tests, it can be stubbed.
func (c *PRChecker) Check(stdin io.Reader) (*Verdict, error) {
	return c.CheckWithResolver(stdin, defaultResolver{})
}

// PRResolver looks up the base (target) branch for a given PR number.
type PRResolver interface {
	// ResolveTargetBranch returns the base branch name for the given PR number.
	// Returns empty string if the PR cannot be resolved (e.g., API failure).
	ResolveTargetBranch(prNumber string) string
}

// CheckWithResolver reads Claude Code hook JSON from stdin, extracts the command,
// and checks whether it is a `gh pr merge` targeting a protected branch.
// Uses the provided resolver to look up PR target branches.
func (c *PRChecker) CheckWithResolver(stdin io.Reader, resolver PRResolver) (*Verdict, error) {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return &Verdict{Allow: true}, nil
	}

	if len(data) == 0 {
		return &Verdict{Allow: true}, nil
	}

	var input hookInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}, nil
	}

	// Only check Bash tool calls.
	if input.ToolName != "Bash" {
		return &Verdict{Allow: true}, nil
	}

	// Parse the command from tool_input.
	if len(input.ToolInput) == 0 {
		return &Verdict{Allow: true}, nil
	}

	var bi bashInput
	if err := json.Unmarshal(input.ToolInput, &bi); err != nil {
		return &Verdict{Allow: true}, nil
	}

	if bi.Command == "" {
		return &Verdict{Allow: true}, nil
	}

	// Check if this is a gh pr merge command.
	if !ghPRMergeRe.MatchString(bi.Command) {
		return &Verdict{Allow: true}, nil
	}

	// Extract PR number.
	matches := prNumberRe.FindStringSubmatch(bi.Command)
	if len(matches) < 2 {
		// No PR number found — cannot determine target branch, allow through.
		return &Verdict{Allow: true}, nil
	}
	prNumber := matches[1]

	// Resolve the target branch for this PR.
	targetBranch := resolver.ResolveTargetBranch(prNumber)
	if targetBranch == "" {
		// Cannot resolve target branch (API failure, PR not found) — allow through.
		// The gh pr merge command itself will fail with a proper error.
		return &Verdict{Allow: true}, nil
	}

	// Check if target branch matches any protected branch pattern.
	for _, protected := range c.ProtectedBranches {
		if matchBranch(targetBranch, protected) {
			reason := fmt.Sprintf(
				"BLOCKED: Cannot merge PR #%s into protected branch '%s'.\n"+
					"Protected branches require manual merge via GitHub UI or admin override.\n"+
					"Matched protection pattern: %s\n\n"+
					"MUST: Do not attempt to merge into protected branches via CLI.\n",
				prNumber, targetBranch, protected,
			)
			return &Verdict{Allow: false, Reason: reason}, nil
		}
	}

	return &Verdict{Allow: true}, nil
}

// matchBranch checks if a branch name matches a protection pattern.
// Supports exact match and glob wildcard (e.g., "release/*").
func matchBranch(branch, pattern string) bool {
	// Exact match.
	if branch == pattern {
		return true
	}

	// Glob/wildcard match: pattern contains "*".
	if strings.Contains(pattern, "*") {
		matched, err := filepath.Match(pattern, branch)
		if err == nil && matched {
			return true
		}
	}

	return false
}

// LoadProtectedBranches reads the protected branch list from enforcement-policy.json.
// Falls back to DefaultProtectedBranches if the file cannot be read or parsed.
func LoadProtectedBranches(projectDir string) []string {
	configPath := filepath.Join(projectDir, ".codeflow", "config", "enforcement", "enforcement-policy.json")

	data, err := os.ReadFile(configPath)
	if err != nil {
		return DefaultProtectedBranches
	}

	var policy enforcementPolicy
	if err := json.Unmarshal(data, &policy); err != nil {
		return DefaultProtectedBranches
	}

	if len(policy.MergeProtection.ProtectedBranches) == 0 {
		return DefaultProtectedBranches
	}

	return policy.MergeProtection.ProtectedBranches
}

// defaultResolver resolves PR target branches using the `gh` CLI.
type defaultResolver struct{}

func (defaultResolver) ResolveTargetBranch(_ string) string {
	// In the hook context, PR resolution is done by the CLI adapter in hooks.go.
	// The package-level Check method is designed for testability;
	// production usage goes through the CLI subcommand which handles gh pr view.
	return ""
}
