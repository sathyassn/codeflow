package main

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/spf13/cobra"
)

// newHooksCmd creates the "hooks" command group.
func newHooksCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "hooks",
		Short: "Hook subcommands for Claude Code integration",
		Long:  "Subcommands invoked by Claude Code hooks for enforcement and validation.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newPreToolUseCmd())
	return cmd
}

// newPreToolUseCmd creates the "pre-tool-use" subcommand group.
func newPreToolUseCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "pre-tool-use",
		Short: "Pre-tool-use hook handlers",
		Long:  "Enforcement modules that run before tool execution.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newSecurityCmd())
	return cmd
}

// ExitHookBlock is the exit code for blocked commands, matching the
// Claude Code hook convention (exit 2 = block).
const ExitHookBlock = 2

// newSecurityCmd creates the "security" subcommand that runs all
// security enforcement modules.
func newSecurityCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "security",
		Short: "Run security enforcement checks on Bash commands",
		Long: `Run all security enforcement modules against a Bash command.

Reads Claude Code hook JSON from stdin, extracts the command,
and runs all security checks. Exits 0 if allowed, 2 if blocked.

Stdin format:
  {"tool_name":"Bash","tool_input":{"command":"..."}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSecurity(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runSecurity implements the security enforcement logic.
func runSecurity(stdin io.Reader, _, errW io.Writer) error {
	toolName, command, sandboxBypass, err := security.ParseHookInput(stdin)
	if err != nil {
		// Parse errors are not security violations — allow through
		return nil
	}

	// Only check Bash tool calls
	if toolName != "Bash" {
		return nil
	}

	// Empty command — allow
	if command == "" {
		return nil
	}

	// Detect project directory
	projectDir := detectProjectDir()

	// Read enforcement policy
	policy, err := security.ReadEnforcementPolicy(projectDir)
	if err != nil {
		// If policy can't be read, use defaults
		policy = security.DefaultPolicy()
	}

	// Detect current git branch
	currentBranch := detectCurrentBranch()

	// Detect PathFlow state
	sessionID := os.Getenv("CODEFLOW_SESSION_ID")
	isPathFlowActive := detectPathFlowActive(projectDir, sessionID)

	// Build check context
	ctx := &security.CheckContext{
		ToolName:         toolName,
		Command:          command,
		SandboxBypass:    sandboxBypass,
		ProjectDir:       projectDir,
		SessionID:        sessionID,
		CurrentBranch:    currentBranch,
		IsPathFlowActive: isPathFlowActive,
		Policy:           policy,
	}

	// Run all security checks
	checker := security.NewChecker()
	verdict := checker.Check(ctx)

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Message())
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("security: %s", verdict.Reason)}
	}

	return nil
}

// detectProjectDir returns the project root directory.
func detectProjectDir() string {
	// Try git rev-parse first
	out, err := exec.Command("git", "rev-parse", "--show-toplevel").Output()
	if err == nil {
		return strings.TrimSpace(string(out))
	}
	// Fallback to current directory
	dir, err := os.Getwd()
	if err != nil {
		return "."
	}
	return dir
}

// detectCurrentBranch returns the current git branch name.
func detectCurrentBranch() string {
	out, err := exec.Command("git", "branch", "--show-current").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// detectPathFlowActive checks if a PathFlow session is active.
func detectPathFlowActive(projectDir, sessionID string) bool {
	if sessionID == "" {
		return false
	}
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	_, err := os.Stat(flagPath)
	return err == nil
}
