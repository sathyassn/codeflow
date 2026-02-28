package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/hooks/gate"
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
	cmd.AddCommand(newGateCheckCmd())
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

// newGateCheckCmd creates the "gate-check" subcommand that enforces
// PathFlow phase gates via sentinel file checks.
func newGateCheckCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "gate-check",
		Short: "Check PathFlow gate for tool operations",
		Long: `Check PathFlow phase gates for tool operations.

Reads Claude Code hook JSON from stdin, checks sentinel files,
and exits 0 if allowed, 2 if blocked.

Gate rules:
  Edit/Write tools      -> require pf-3 sentinel
  Bash git commit       -> require pf-3 sentinel
  Bash git push / gh pr -> require pf-5 AND ws-rev sentinels
  Task role teammates   -> require pf-3 sentinel

When no PathFlow session is active, all operations are allowed.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			event, _ := cmd.Flags().GetString("event")
			return runGateCheck(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr(), event)
		},
	}
	cmd.Flags().String("event", "PreToolUse", "Hook event type (PreToolUse, Stop, SubagentStop)")
	return cmd
}

// runGateCheck implements the PathFlow gate enforcement logic.
func runGateCheck(stdin io.Reader, _ io.Writer, errW io.Writer, event string) error {

	// Stop and SubagentStop events are informational only — always allow.
	if event == "Stop" || event == "SubagentStop" {
		return nil
	}

	// Read stdin.
	data, err := io.ReadAll(stdin)
	if err != nil {
		// Read error — allow through (graceful degradation).
		return nil
	}

	if len(data) == 0 {
		// Empty stdin — allow through.
		return nil
	}

	// Parse the hook input.
	var input gate.HookInput
	if err := json.Unmarshal(data, &input); err != nil {
		// Parse error — allow through (graceful degradation).
		return nil
	}

	// Detect project directory and session ID.
	projectDir := detectProjectDir()
	sessionID := os.Getenv("CODEFLOW_SESSION_ID")

	// Check if PathFlow is active.
	if !detectPathFlowActive(projectDir, sessionID) {
		// No PathFlow session — allow everything.
		return nil
	}

	// Classify the gate type to determine if we need sentinel checks.
	gateType := gate.ClassifyGateType(input.ToolName, input.ToolInput)

	// Unknown session ID + critical gate → block immediately.
	if sessionID == "unknown" {
		switch gateType {
		case gate.GateGitPushPR, gate.GateRoleTeammateSpawn:
			fmt.Fprintf(errW, "BLOCKED: PathFlow gate - session state unknown\nReason: Cannot verify prerequisites (session ID not registered)\nTool: %s\nGate: %s\n", input.ToolName, gateType)
			return &exitError{code: ExitHookBlock, err: fmt.Errorf("gate: session state unknown for %s", gateType)}
		}
		// Non-critical gates with unknown session: allow (graceful degradation).
	}

	// Build sentinel directory path.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)

	// Create checker and evaluate.
	checker := &gate.GateChecker{
		SentinelDir: sentinelDir,
		SessionID:   sessionID,
	}
	verdict := checker.Check(input.ToolName, input.ToolInput)

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Reason)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("gate: %s", verdict.Reason)}
	}

	return nil
}

