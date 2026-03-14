package gate

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
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
		return g.checkCumulativePushPRGate()
	}

	// Should not reach here, but allow by default.
	return &Verdict{Allow: true, GateType: gateType}
}

// checkCumulativePushPRGate verifies ALL pf-1 through pf-5 sentinels
// AND ALL pipeline stage sentinels before allowing push/PR operations.
func (g *GateChecker) checkCumulativePushPRGate() *Verdict {
	// Cumulative phase check: ALL pf-1 through pf-5.
	ok, missing := sentinel.VerifyCumulativePhaseSentinels(g.SentinelDir, 5)
	if !ok {
		return &Verdict{
			Allow:    false,
			GateType: GateGitPushPR,
			Reason: fmt.Sprintf(
				"BLOCKED: PathFlow gate - prerequisite not met\n"+
					"Reason: git_push_pr requires all phases through PF5-VERIFY. Missing phase sentinel: pathflow-%s\n"+
					"Gate: git_push_pr\n", missing),
		}
	}

	// Cumulative stage check: ALL pipeline stages.
	sessionDir := deriveSessionDirFromSentinelDir(g.SentinelDir)
	workType := sentinel.ReadWorkTypeFromSessionStatus(sessionDir)

	if workType != "" {
		configDir := deriveConfigDirFromSentinelDir(g.SentinelDir)
		pipelines, err := sentinel.LoadPipelines(configDir)
		if err != nil {
			// Config load failure -- graceful degradation, allow through after phase check.
			slog.Warn("gate: cannot load pipelines for push/PR gate", "error", err)
			return &Verdict{Allow: true, GateType: GateGitPushPR}
		}

		if pipeline, exists := pipelines[workType]; exists {
			allOk, missingStage := sentinel.VerifyCumulativeStageSentinels(g.SentinelDir, pipeline, len(pipeline)-1)
			if !allOk {
				return &Verdict{
					Allow:    false,
					GateType: GateGitPushPR,
					Reason: fmt.Sprintf(
						"BLOCKED: PathFlow gate - prerequisite not met\n"+
							"Reason: git_push_pr requires all pipeline stages. Missing stage sentinel: pathflow-%s\n"+
							"Pipeline for %s: %v\n"+
							"Gate: git_push_pr\n", missingStage, workType, pipeline),
				}
			}
		}
	}

	return &Verdict{Allow: true, GateType: GateGitPushPR}
}

// deriveSessionDirFromSentinelDir derives the session pathflow directory from sentinel dir.
func deriveSessionDirFromSentinelDir(sentinelDir string) string {
	projectDir := filepath.Join(sentinelDir, "..", "..", "..", "..")
	sessionID := filepath.Base(sentinelDir)
	return filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
}

// deriveConfigDirFromSentinelDir derives the pathflow config directory from sentinel dir.
func deriveConfigDirFromSentinelDir(sentinelDir string) string {
	projectDir := filepath.Join(sentinelDir, "..", "..", "..", "..")
	return filepath.Join(projectDir, ".codeflow", "config", "pathflow")
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
