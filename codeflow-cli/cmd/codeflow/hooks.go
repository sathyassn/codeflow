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
	"github.com/codeflow/codeflow-cli/internal/hooks/ghpr"
	"github.com/codeflow/codeflow-cli/internal/hooks/resource"
	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
	"github.com/codeflow/codeflow-cli/internal/hooks/session"
	"github.com/codeflow/codeflow-cli/internal/hooks/team"
	"github.com/codeflow/codeflow-cli/internal/hooks/webfetch"
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
	cmd.AddCommand(newPostToolUseCmd())
	cmd.AddCommand(newTaskCompletedCmd())
	cmd.AddCommand(newHookSessionStartCmd())
	cmd.AddCommand(newHookSessionEndCmd())
	cmd.AddCommand(newStopCmd())
	cmd.AddCommand(newUserPromptSubmitCmd())
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
	cmd.AddCommand(newGHPRGuardCmd())
	cmd.AddCommand(newWebFetchGuardCmd())
	cmd.AddCommand(newTeamGuardCmd())
	cmd.AddCommand(newProtectionGuardCmd())
	cmd.AddCommand(newEditWriteGuardCmd())
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

// newGHPRGuardCmd creates the "gh-pr-guard" subcommand that blocks
// gh pr merge commands targeting protected branches.
func newGHPRGuardCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "gh-pr-guard",
		Short: "Block gh pr merge against protected branches",
		Long: `Check whether a gh pr merge command targets a protected branch.

Reads Claude Code hook JSON from stdin, extracts the command,
and blocks merges targeting protected branches (main, master,
release/*, production). Exits 0 if allowed, 2 if blocked.

Protected branches are read from enforcement-policy.json.

Stdin format:
  {"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runGHPRGuard(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// ghPRResolver resolves PR target branches using the gh CLI.
type ghPRResolver struct{}

func (ghPRResolver) ResolveTargetBranch(prNumber string) string {
	out, err := exec.Command("gh", "pr", "view", prNumber, "--json", "baseRefName", "--jq", ".baseRefName").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// runGHPRGuard implements the gh-pr merge protection logic.
func runGHPRGuard(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	branches := ghpr.LoadProtectedBranches(projectDir)

	checker := &ghpr.PRChecker{ProtectedBranches: branches}
	verdict, err := checker.CheckWithResolver(stdin, ghPRResolver{})
	if err != nil {
		// Errors are not security violations — allow through.
		return nil
	}

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Reason)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("ghpr: blocked")}
	}

	return nil
}

// newWebFetchGuardCmd creates the "webfetch-guard" subcommand that validates
// URLs against trusted domain allowlists.
func newWebFetchGuardCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "webfetch-guard",
		Short: "Validate URLs against trusted domain allowlists",
		Long: `Validate URLs in WebFetch, WebSearch, and Bash network commands.

Reads Claude Code hook JSON from stdin, extracts URLs, and validates
them against the trusted domain allowlist. Blocks internal IPs,
localhost, file:// URLs, and data: URLs. Exits 0 if allowed, 2 if blocked.

Configuration is loaded from enforcement-policy.json and trusted-domains/*.list.

Stdin format:
  {"tool_name":"WebFetch","tool_input":{"url":"https://example.com"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWebFetchGuard(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runWebFetchGuard implements the URL validation logic.
func runWebFetchGuard(stdin io.Reader, outW io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	checker := loadWebFetchChecker(projectDir)

	verdict, err := checker.Check(stdin)
	if err != nil {
		// Parse errors are not security violations — allow through.
		return nil
	}

	if !verdict.Allow {
		// Hard-block: always-blocked domains (localhost, private IPs, etc.)
		if verdict.AlwaysBlocked {
			fmt.Fprint(errW, verdict.Reason)
			return &exitError{code: ExitHookBlock, err: fmt.Errorf("webfetch: blocked")}
		}
		// Untrusted domain: ask for user approval (matching shell default behavior).
		domain := verdict.Domain
		fmt.Fprintf(outW, "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"Domain '%s' not in standard allowlist\"}}\n", domain)
		return nil
	}

	return nil
}

// loadWebFetchChecker creates a URLChecker configured from enforcement-policy.json
// and the trusted-domains list files.
func loadWebFetchChecker(projectDir string) *webfetch.URLChecker {
	checker := &webfetch.URLChecker{}

	configPath := filepath.Join(projectDir, ".codeflow", "config", "enforcement", "enforcement-policy.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		return checker
	}

	var policy struct {
		Network struct {
			AlwaysBlockDomains struct {
				Enabled        bool     `json:"enabled"`
				Patterns       []string `json:"patterns"`
				PrivateIPRange []string `json:"private_ip_ranges"`
			} `json:"always_block_domains"`
		} `json:"network"`
	}
	if err := json.Unmarshal(data, &policy); err != nil {
		return checker
	}

	if policy.Network.AlwaysBlockDomains.Enabled {
		checker.BlockedPatterns = policy.Network.AlwaysBlockDomains.Patterns
		checker.PrivateIPRanges = policy.Network.AlwaysBlockDomains.PrivateIPRange
	}

	// Load trusted domains from list files (default to standard mode).
	trustedDir := filepath.Join(projectDir, ".codeflow", "config", "enforcement", "trusted-domains")
	listFile := filepath.Join(trustedDir, "standard.list")
	checker.TrustedDomains = loadTrustedDomains(listFile)

	return checker
}

// loadTrustedDomains reads domains from a .list file, skipping comments and blank lines.
func loadTrustedDomains(path string) []string {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}

	var domains []string
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		domains = append(domains, line)
	}
	return domains
}

// newTeamGuardCmd creates the "team-guard" subcommand that blocks
// TeamDelete during active PathFlow sessions.
func newTeamGuardCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "team-guard",
		Short: "Block TeamDelete during active PathFlow sessions",
		Long: `Check whether TeamDelete or team cleanup is safe to execute.

Reads Claude Code hook JSON from stdin and checks:
  - If pathflow-active flag exists AND pf-6 sentinel does NOT exist → block
  - If pf-6 sentinel exists → allow (PF7-END in progress)
  - If no pathflow-active flag → allow (no active session)

Exits 0 if allowed, 2 if blocked.

Stdin format:
  {"tool_name":"TeamDelete","tool_input":{}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runTeamGuard(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runTeamGuard implements the team guard enforcement logic.
func runTeamGuard(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	sessionID := os.Getenv("CODEFLOW_SESSION_ID")
	if sessionID == "" {
		// No session ID — no PathFlow, allow.
		return nil
	}

	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)

	verdict, err := team.CheckTeamDelete(stdin, sessionDir, sentinelDir)
	if err != nil {
		// Parse errors are not security violations — allow through.
		return nil
	}

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Reason)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("team-guard: blocked")}
	}

	return nil
}

// newHookSessionStartCmd creates the "session-start" subcommand group under hooks.
func newHookSessionStartCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "session-start",
		Short: "Session-start hook handlers",
		Long:  "Subcommands invoked by Claude Code session-start hooks for initialization.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newHookSessionStartInitCmd())
	cmd.AddCommand(newSessionStartLogCmd())
	cmd.AddCommand(newHookSessionStartInstructionsCmd())
	return cmd
}

// newHookSessionStartInstructionsCmd creates the "instructions" subcommand
// that outputs session-start instructions for Claude Code injection.
func newHookSessionStartInstructionsCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "instructions",
		Short: "Output session-start instructions for Claude Code injection",
		Long: `Reads enabled SessionStart instructions from config, outputs active task
context and PathFlow recovery info.

Reads Claude Code SessionStart hook JSON from stdin. Outputs enabled instruction
files, active task context, and PathFlow recovery checklist to stdout.
Always exits 0 (SessionStart hooks must never block).`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return session.RunInstructions(cmd.InOrStdin(), cmd.OutOrStdout(), detectProjectDir())
		},
	}
}

// newHookSessionStartInitCmd creates the "init" subcommand that performs all
// session initialization: parse stdin, detect stale sessions, generate
// session ID, create directories, initialize PathFlow state.
func newHookSessionStartInitCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "init",
		Short: "Initialize a new CodeFlow session",
		Long: `Perform all session initialization steps.

Reads Claude Code SessionStart hook JSON from stdin, detects stale sessions,
generates or loads a CODEFLOW_SESSION_ID, creates required .state/ directories,
initializes PathFlow state, and outputs environment variables as JSON.

This replaces the cf-session-start-init.sh shell script with a single binary call.

Stdin format:
  {"session_id":"<claude-uuid>","source":"startup|resume|compact|clear"}

Output (stdout):
  {"env":{"CODEFLOW_SESSION_ID":"ses-...","CF_PROJECT_ROOT":"..."}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionStartInit(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runSessionStartInit implements the session-start init logic.
func runSessionStartInit(stdin io.Reader, outW io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()

	initializer := session.NewInitializer()
	result, err := initializer.StartInit(stdin, projectDir)
	if err != nil {
		fmt.Fprintf(errW, "session-start init error: %v\n", err)
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("session-start init: %w", err)}
	}

	// Emit warnings to stderr.
	for _, w := range result.Warnings {
		fmt.Fprintf(errW, "WARNING: %s\n", w)
	}

	// Emit messages to stderr (informational).
	for _, m := range result.Messages {
		fmt.Fprintf(errW, "%s\n", m)
	}

	// Output env vars as JSON to stdout (for hook framework).
	envJSON, err := result.FormatEnvOutput()
	if err != nil {
		fmt.Fprintf(errW, "session-start init: failed to format output: %v\n", err)
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("session-start init format: %w", err)}
	}
	fmt.Fprintln(outW, string(envJSON))

	return nil
}

// newPostToolUseCmd creates the "post-tool-use" subcommand group.
func newPostToolUseCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "post-tool-use",
		Short: "Post-tool-use hook handlers",
		Long:  "Handlers that run after tool execution for sentinel and checkpoint management.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newSentinelWriteCmd())
	cmd.AddCommand(newHookCheckpointRegisterCmd())
	cmd.AddCommand(newSettingsValidateHookCmd())
	cmd.AddCommand(newPostToolUseLogCmd())
	return cmd
}

// newTaskCompletedCmd creates the "task-completed" subcommand group.
func newTaskCompletedCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "task-completed",
		Short: "Task-completed hook handlers",
		Long:  "Handlers that run when a task is marked complete.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newHookCheckpointCompleteCmd())
	return cmd
}

// newSentinelWriteCmd creates the "sentinel-write" subcommand that creates
// stage sentinel files when STAGE-COMPLETE messages are detected.
func newSentinelWriteCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "sentinel-write",
		Short: "Create stage sentinels from STAGE-COMPLETE messages",
		Long: `Create PathFlow stage sentinel files from STAGE-COMPLETE messages.

Reads Claude Code PostToolUse hook JSON from stdin, detects SendMessage
calls containing "STAGE-COMPLETE: WS-{STAGE}", validates stage ordering,
and creates the corresponding sentinel file. Exits 0 if allowed, 2 if blocked.

Stage ordering rules:
  WS-REV requires a prior primary stage (ws-dev/ws-plan/ws-docs/ws-test)
  WS-QA  requires prior ws-dev or ws-test

Stdin format:
  {"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSentinelWrite(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runSentinelWrite implements the stage sentinel creation logic.
func runSentinelWrite(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	sessionID := os.Getenv("CODEFLOW_SESSION_ID")
	if sessionID == "" {
		// No session ID — no PathFlow, allow.
		return nil
	}

	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentinelDir)

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Reason)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("sentinel-write: blocked")}
	}

	return nil
}

// newHookCheckpointRegisterCmd creates the "checkpoint-register" subcommand that
// registers PF tasks in the checkpoint file when TaskCreate fires.
func newHookCheckpointRegisterCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "checkpoint-register",
		Short: "Register PF tasks in checkpoint on TaskCreate",
		Long: `Register PathFlow phase tasks in the checkpoint file.

Reads Claude Code PostToolUse hook JSON from stdin, detects TaskCreate
calls with PF{N}-TSK-{NN} subjects, validates cross-phase dependencies,
and registers the task in the checkpoint. Exits 0 if allowed, 2 if blocked.

Cross-phase gate: PF{N} tasks cannot register until pf-{N-1} sentinel exists.

Stdin format:
  {"tool_name":"TaskCreate","tool_input":{"subject":"PF3-TSK-01 Classify work"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runHookCheckpointRegister(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runHookCheckpointRegister implements the hook-based checkpoint task registration logic.
func runHookCheckpointRegister(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	sessionID := os.Getenv("CODEFLOW_SESSION_ID")
	if sessionID == "" {
		return nil
	}

	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)

	verdict := sentinel.RegisterCheckpointTask(stdin, sessionDir, sentinelDir)

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Reason)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("checkpoint-register: blocked")}
	}

	return nil
}

// newHookCheckpointCompleteCmd creates the "checkpoint-complete" subcommand that
// marks PF tasks complete and creates phase sentinels when all tasks are done.
func newHookCheckpointCompleteCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "checkpoint-complete",
		Short: "Mark PF tasks complete and create phase sentinels",
		Long: `Mark PathFlow phase tasks complete in the checkpoint file.

Reads Claude Code TaskCompleted hook JSON from stdin, detects tasks with
PF{N}-TSK-{NN} subjects, validates cross-phase dependencies, marks the
task complete, and creates a phase sentinel if all tasks in the phase are
done, skipped, or auto-skipped by condition. Exits 0 if allowed, 2 if blocked.

Stdin format:
  {"task_subject":"PF3-TSK-01 Classify work"}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runHookCheckpointComplete(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runHookCheckpointComplete implements the hook-based checkpoint task completion logic.
func runHookCheckpointComplete(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	sessionID := os.Getenv("CODEFLOW_SESSION_ID")
	if sessionID == "" {
		return nil
	}

	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)

	verdict := sentinel.CompleteCheckpointTask(stdin, sessionDir, sentinelDir)

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Reason)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("checkpoint-complete: blocked")}
	}

	return nil
}

// newHookSessionEndCmd creates the "session-end" subcommand group under hooks.
func newHookSessionEndCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "session-end",
		Short: "Session-end hook handlers",
		Long:  "Subcommands invoked by Claude Code session-end hooks for cleanup.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newHookSessionEndCleanupCmd())
	cmd.AddCommand(newSessionEndLogCmd())
	return cmd
}

// newHookSessionEndCleanupCmd creates the "cleanup" subcommand that performs all
// session cleanup: validate PF7, clean sentinels, archive state, remove stale
// files, write session_end ledger event.
func newHookSessionEndCleanupCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "cleanup",
		Short: "Clean up session state at session end",
		Long: `Perform all session cleanup steps at session end.

Reads Claude Code SessionEnd hook JSON from stdin, validates PF7 completion,
cleans up sentinels, archives session state, removes stale runtime files,
and writes a session_end ledger event.

This replaces the cf-session-end-cleanup.sh shell script with a single binary call.

Stdin format:
  {"session_id":"<claude-uuid>","transcript_path":"<path>"}

Exit codes:
  0 - Cleanup completed (always exits 0, warnings on stderr)`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionEndCleanup(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// newProtectionGuardCmd creates the "protection-guard" subcommand that checks
// file paths against protection tiers and handles auto-staging.
func newProtectionGuardCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "protection-guard",
		Short: "Check file protection tier and auto-stage if protected",
		Long: `Check whether an Edit/Write operation targets a protected resource.

Reads Claude Code PreToolUse hook JSON from stdin, checks the target file
against protection tiers (critical/high/moderate) per enforcement-policy.json.

If protected (critical/high): auto-stages to staging area and provides
structured feedback with tier info and next steps. Exits 2 (block).

If moderate: allows through with a warning. Exits 0.
If unprotected: allows through silently. Exits 0.

Stdin format:
  {"tool_name":"Edit","tool_input":{"file_path":".claude/settings.json"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runProtectionGuard(cmd)
		},
	}
}

// runProtectionGuard implements the protection guard logic.
func runProtectionGuard(cmd *cobra.Command) error {
	projectDir := detectProjectDir()
	guard := resource.NewProtectionGuard(projectDir)

	verdict, err := guard.Check(cmd.InOrStdin())
	if err != nil {
		// Errors are not security violations — allow through.
		return nil
	}

	if !verdict.Allow {
		fmt.Fprint(cmd.ErrOrStderr(), verdict.Message)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("protection-guard: %s tier, path: %s", verdict.Tier, verdict.Path)}
	}

	// Print moderate tier warnings to stderr.
	if verdict.Tier == resource.TierModerate && verdict.Message != "" {
		fmt.Fprintln(cmd.ErrOrStderr(), verdict.Message)
	}

	return nil
}

// newSettingsValidateHookCmd creates the "settings-validate" subcommand
// for the PostToolUse hook entry point.
func newSettingsValidateHookCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "settings-validate",
		Short: "Validate settings templates after edits (hook entry point)",
		Long: `PostToolUse hook for settings template validation.

Reads Claude Code hook JSON from stdin, filters for Edit/Write operations
targeting settings-templates/*.json files. Always exits 0 (advisory).
Outputs hookSpecificOutput JSON with validation results.

Stdin format:
  {"tool_name":"Edit","tool_input":{"file_path":".claude/settings-templates/autonomous.json"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSettingsValidateHook(cmd, detectProjectDir)
		},
	}
}

// runSessionEndCleanup implements the session-end cleanup logic.
func runSessionEndCleanup(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()

	cleaner := session.NewCleaner()
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		fmt.Fprintf(errW, "session-end cleanup error: %v\n", err)
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("session-end cleanup: %w", err)}
	}

	// Emit warnings to stderr.
	for _, w := range result.Warnings {
		fmt.Fprintf(errW, "WARNING: %s\n", w)
	}

	// Emit messages to stderr (informational).
	for _, m := range result.Messages {
		fmt.Fprintf(errW, "%s\n", m)
	}

	// Emit cleanup summary.
	if result.SentinelsCleaned > 0 {
		fmt.Fprintf(errW, "SessionEnd: Cleaned %d sentinel(s)\n", result.SentinelsCleaned)
	} else {
		fmt.Fprintln(errW, "SessionEnd: No expired sentinels to clean")
	}

	return nil
}

// newStopCmd creates the "stop" subcommand group under hooks.
func newStopCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "stop",
		Short: "Stop hook handlers",
		Long:  "Subcommands invoked by Claude Code stop hooks for logging and gate checks.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newStopLogCmd())
	return cmd
}

// newUserPromptSubmitCmd creates the "user-prompt-submit" subcommand group under hooks.
func newUserPromptSubmitCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "user-prompt-submit",
		Short: "User-prompt-submit hook handlers",
		Long:  "Subcommands invoked by Claude Code user-prompt-submit hooks for logging.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newUserPromptSubmitLogCmd())
	cmd.AddCommand(newPromptValidateCmd())
	return cmd
}
