package gate

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// GateType classifies the kind of PathFlow gate that applies to a tool operation.
type GateType string

const (
	// GateEditWrite applies to Edit and Write tool calls (requires pf-3).
	GateEditWrite GateType = "edit_write"

	// GateGitCommit applies to Bash git commit commands (requires pf-3).
	GateGitCommit GateType = "git_commit"

	// GateGitPushPR applies to Bash git push and gh pr commands
	// (requires both pf-5 and ws-rev).
	GateGitPushPR GateType = "git_push_pr"

	// GateRoleTeammateSpawn applies to Task tool calls that spawn role teammates
	// (requires pf-3).
	GateRoleTeammateSpawn GateType = "role_teammate_spawn"

	// GateUngated indicates no gate applies (operation always allowed).
	GateUngated GateType = "ungated"
)

// roleTeammates lists the role teammates that require pf-3 sentinel before spawning.
// Function teammates (cf-security, cf-knowledge-layer, cf-git-operations) are excluded
// because they spawn before pf-3 exists.
var roleTeammates = []string{
	"cf-development",
	"cf-planning",
	"cf-documentation",
	"cf-review",
	"cf-quality-assurance",
}

// Patterns for classifying bash commands. Compiled once at package init.
var (
	// gitPushPRPattern matches git push or gh pr at word boundaries,
	// accounting for leading operators (&&, |, ;) or start of string.
	gitPushPRPattern = regexp.MustCompile(`(?:^|\s|&&|\|)(?:git\s+push|gh\s+pr)(?:\s|$)`)

	// gitCommitPattern matches git commit at word boundaries.
	gitCommitPattern = regexp.MustCompile(`(?:^|\s|&&|\|)git\s+commit(?:\s|$)`)

	// quotedStringPattern matches single-quoted and double-quoted strings
	// (with backslash escaping for double quotes) for stripping before classification.
	quotedStringPattern = regexp.MustCompile(`"(?:[^"\\]|\\.)*"|'[^']*'`)
)

// Verdict represents the result of a gate check.
type Verdict struct {
	// Allow is true if the operation is permitted.
	Allow bool

	// GateType is the classification of the gate that was evaluated.
	GateType GateType

	// Reason is a human-readable explanation of why the operation was blocked.
	Reason string
}

// GateChecker evaluates PathFlow gate rules against sentinel files.
type GateChecker struct {
	// SentinelDir is the path to the sentinel directory for this session:
	// {projectDir}/.state/sentinels/pathflow/{sessionID}/
	SentinelDir string

	// SessionID is the current CodeFlow session ID.
	SessionID string
}

// Check evaluates the gate rules for a given tool invocation.
// It classifies the gate type, checks required sentinels, and returns a verdict.
func (g *GateChecker) Check(toolName string, toolInput json.RawMessage) *Verdict {
	gateType := ClassifyGateType(toolName, toolInput)

	if gateType == GateUngated {
		return &Verdict{Allow: true, GateType: GateUngated}
	}

	switch gateType {
	case GateEditWrite, GateGitCommit, GateRoleTeammateSpawn:
		if g.hasSentinel("pf-3") {
			return &Verdict{Allow: true, GateType: gateType}
		}
		return &Verdict{
			Allow:    false,
			GateType: gateType,
			Reason:   fmt.Sprintf("BLOCKED: PathFlow gate - prerequisite not met\nReason: %s requires PF3-CLASSIFY (branch creation). No pathflow-pf-3 sentinel found.\nGate: %s\n", gateType, gateType),
		}

	case GateGitPushPR:
		if !g.hasSentinel("pf-5") {
			return &Verdict{
				Allow:    false,
				GateType: gateType,
				Reason:   "BLOCKED: PathFlow gate - prerequisite not met\nReason: git_push_pr requires PF5-VERIFY (acceptance criteria verified). No pathflow-pf-5 sentinel found.\nGate: git_push_pr\n",
			}
		}
		if !g.hasSentinel("ws-rev") {
			return &Verdict{
				Allow:    false,
				GateType: gateType,
				Reason:   "BLOCKED: PathFlow gate - prerequisite not met\nReason: git_push_pr requires WS-REV (review completed). No pathflow-ws-rev sentinel found.\nGate: git_push_pr\n",
			}
		}
		return &Verdict{Allow: true, GateType: gateType}
	}

	// Should not reach here, but allow by default.
	return &Verdict{Allow: true, GateType: gateType}
}

// hasSentinel checks whether a sentinel file exists in the sentinel directory.
// Sentinel files are named "pathflow-{name}" (e.g., "pathflow-pf-3").
func (g *GateChecker) hasSentinel(name string) bool {
	path := filepath.Join(g.SentinelDir, "pathflow-"+name)
	_, err := os.Stat(path)
	return err == nil
}

// HookInput represents the JSON structure sent by Claude Code on stdin
// to PreToolUse hooks.
type HookInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// bashInput represents the tool_input for Bash tool calls.
type bashInput struct {
	Command string `json:"command"`
}

// taskInput represents the tool_input for Task tool calls.
type taskInput struct {
	Prompt      string `json:"prompt"`
	Name        string `json:"name"`
	Description string `json:"description"`
}

// ClassifyGateType determines the gate type from the tool name and input.
// It does not check sentinels -- it only classifies which gate applies.
func ClassifyGateType(toolName string, toolInput json.RawMessage) GateType {
	switch toolName {
	case "Edit", "Write":
		return GateEditWrite

	case "Bash":
		return classifyBashGate(toolInput)

	case "Task":
		return classifyTaskGate(toolInput)

	default:
		return GateUngated
	}
}

// classifyBashGate classifies the gate type for a Bash tool call
// based on the command content.
func classifyBashGate(toolInput json.RawMessage) GateType {
	if len(toolInput) == 0 {
		return GateUngated
	}

	var bi bashInput
	if err := json.Unmarshal(toolInput, &bi); err != nil {
		return GateUngated
	}

	if bi.Command == "" {
		return GateUngated
	}

	// Strip quoted strings to prevent false positives from commit messages
	// containing "git push" or "gh pr" as text.
	stripped := quotedStringPattern.ReplaceAllString(bi.Command, "")

	if gitPushPRPattern.MatchString(stripped) {
		return GateGitPushPR
	}
	if gitCommitPattern.MatchString(stripped) {
		return GateGitCommit
	}

	return GateUngated
}

// classifyTaskGate classifies the gate type for a Task tool call.
// Role teammate spawns require the pf-3 sentinel; other tasks (function
// teammates, explore sub-agents) are ungated.
func classifyTaskGate(toolInput json.RawMessage) GateType {
	if len(toolInput) == 0 {
		return GateUngated
	}

	var ti taskInput
	if err := json.Unmarshal(toolInput, &ti); err != nil {
		return GateUngated
	}

	// Concatenate all text fields to search for role teammate names.
	combined := strings.Join([]string{ti.Prompt, ti.Name, ti.Description}, " ")
	if strings.TrimSpace(combined) == "" {
		return GateUngated
	}

	for _, role := range roleTeammates {
		if strings.Contains(combined, role) {
			return GateRoleTeammateSpawn
		}
	}

	return GateUngated
}
