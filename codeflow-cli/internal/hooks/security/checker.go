package security

import "fmt"

// Verdict represents the result of a security check.
type Verdict struct {
	// Allow is true if the command is permitted.
	Allow bool

	// Category describes the security violation category (e.g., "Dangerous Command").
	Category string

	// Reason is a human-readable explanation of why the command was blocked.
	Reason string

	// Pattern is the specific pattern that triggered the block.
	Pattern string

	// Module is the name of the module that produced this verdict.
	Module string
}

// Message formats the block message for stderr output, matching the shell
// hook's block_command format.
func (v *Verdict) Message() string {
	if v.Allow {
		return ""
	}
	return fmt.Sprintf(`BLOCKED: Security violation detected

Category: %s
Reason: %s
Pattern: %s

This is a security restriction enforced by CodeFlow.
See: .codeflow/docs/security/README.md
`, v.Category, v.Reason, v.Pattern)
}

// Module is the interface implemented by each security enforcement module.
type Module interface {
	// Name returns the module name for logging and diagnostics.
	Name() string

	// Check evaluates the command against the module's rules.
	// Returns nil if the module has no opinion (allow), or a Verdict
	// with Allow=false to block.
	Check(ctx *CheckContext) *Verdict
}

// CheckContext holds all information needed for security checks.
type CheckContext struct {
	// ToolName is the Claude Code tool being used (e.g., "Bash").
	ToolName string

	// Command is the bash command being checked.
	Command string

	// SandboxBypass is true if dangerouslyDisableSandbox is set.
	SandboxBypass bool

	// ProjectDir is the repository root directory.
	ProjectDir string

	// SessionID is the current CodeFlow session ID.
	SessionID string

	// CurrentBranch is the current git branch name.
	CurrentBranch string

	// IsPathFlowActive is true if a PathFlow session is active.
	IsPathFlowActive bool

	// Policy is the parsed enforcement policy configuration.
	Policy *EnforcementPolicy
}

// Checker orchestrates security enforcement modules.
type Checker struct {
	Modules []Module
}

// NewChecker creates a Checker with the default set of enforcement modules
// in priority order (critical checks first).
func NewChecker() *Checker {
	return &Checker{
		Modules: DefaultModules(),
	}
}

// DefaultModules returns the standard enforcement modules in execution order.
func DefaultModules() []Module {
	return []Module{
		&DangerousModule{},  // Critical: rm -rf /, fork bombs
		&PrivilegeModule{},  // Critical: sudo, su, doas
		&GitModule{},        // High: hook bypass, force push
		&PathModule{},       // High: protected path operations
		&FileOpsModule{},    // High: indirect writes, glob bypass
		&BranchModule{},     // Moderate: writes on protected branches
		&TmpModule{},        // Moderate: managed tmp protection
		&NetworkModule{},    // Moderate: network operations
	}
}

// Check runs all modules against the given context. Returns the first
// blocking verdict, or an allow verdict if all modules pass.
func (c *Checker) Check(ctx *CheckContext) *Verdict {
	for _, m := range c.Modules {
		if v := m.Check(ctx); v != nil && !v.Allow {
			v.Module = m.Name()
			return v
		}
	}
	return &Verdict{Allow: true}
}

// block is a helper to create a blocking verdict.
func block(category, reason, pattern string) *Verdict {
	return &Verdict{
		Allow:    false,
		Category: category,
		Reason:   reason,
		Pattern:  pattern,
	}
}
