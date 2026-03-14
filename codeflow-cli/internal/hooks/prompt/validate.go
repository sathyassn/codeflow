package prompt

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
)

// PromptValidator outputs context reminders for user prompt submissions.
type PromptValidator struct {
	ProjectDir        string
	ProtectedBranches []string

	// GitRunner returns (branch, uncommittedCount) for git context.
	// Override in tests to avoid real git calls.
	GitRunner func(projectDir string) (branch string, uncommitted int)

	// PathFlowChecker returns true if PathFlow is active.
	// Override in tests.
	PathFlowChecker func(projectDir string) bool
}

// activeTaskJSON represents the .state/runtime/active-task.json structure.
type activeTaskJSON struct {
	TaskID   string `json:"task_id"`
	FormatID string `json:"format_id"`
}

// DefaultProtectedBranches returns the default protected branch list.
func DefaultProtectedBranches() []string {
	return []string{"main", "master"}
}

// Validate gathers context and writes reminders to outW. Always returns nil.
func (v *PromptValidator) Validate(_ io.Reader, outW io.Writer) error {
	var reminders []string

	// 1. Git context.
	branch, uncommitted := v.gitContext()
	if uncommitted > 0 {
		reminders = append(reminders,
			fmt.Sprintf("Note: %d uncommitted changes on branch '%s'", uncommitted, branch))
	}

	// 2. Protected branch warning.
	if v.isProtectedBranch(branch) {
		reminders = append(reminders,
			fmt.Sprintf("Warning: On protected branch '%s'. Create a feature branch before changes.\n"+
				"  Delegate to cf-git-operations teammate: SendMessage(recipient=\"cf-git-operations\", content=\"create-feature-branch name=feat/...\")",
				branch))
	}

	// 3. Active task.
	taskID := v.activeTaskID()
	if taskID != "" {
		reminders = append(reminders, fmt.Sprintf("Active work: %s", taskID))
	} else {
		reminders = append(reminders,
			"Note: No active task registered. Delegate to cf-knowledge-layer teammate: "+
				"SendMessage(recipient=\"cf-knowledge-layer\", content=\"ensure-work-registered\")")
	}

	// 4. PathFlow mode.
	if v.isPathFlowActive() {
		reminders = append(reminders,
			"PathFlow mode active: Coordinate with teammates for specialized tasks.\n"+
				"  Git operations -> cf-git-operations | Code review -> cf-review")
	}

	// Write each reminder wrapped in tags.
	for _, r := range reminders {
		fmt.Fprintf(outW, "<user-prompt-submit-hook>\n%s\n</user-prompt-submit-hook>\n", r)
	}

	return nil
}

// gitContext returns branch name and uncommitted change count.
func (v *PromptValidator) gitContext() (string, int) {
	if v.GitRunner != nil {
		return v.GitRunner(v.ProjectDir)
	}
	return defaultGitContext(v.ProjectDir)
}

// defaultGitContext uses real git commands.
func defaultGitContext(projectDir string) (string, int) {
	branch := runGitCmd(projectDir, "branch", "--show-current")
	if branch == "" {
		branch = "unknown"
	}

	status := runGitCmd(projectDir, "status", "--porcelain")
	count := 0
	if status != "" {
		count = len(strings.Split(strings.TrimSpace(status), "\n"))
	}

	return branch, count
}

// runGitCmd runs a git command and returns trimmed stdout.
func runGitCmd(projectDir string, args ...string) string {
	fullArgs := append([]string{"-C", projectDir}, args...)
	cmd := newExecCmd("git", fullArgs...)
	out, err := cmd.Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// isProtectedBranch checks if a branch matches any protected branch pattern.
func (v *PromptValidator) isProtectedBranch(branch string) bool {
	for _, protected := range v.ProtectedBranches {
		if branch == protected {
			return true
		}
		if strings.Contains(protected, "*") {
			matched, err := filepath.Match(protected, branch)
			if err == nil && matched {
				return true
			}
		}
	}
	return false
}

// activeTaskID reads the active task ID from .state/runtime/active-task.json.
func (v *PromptValidator) activeTaskID() string {
	path := filepath.Join(v.ProjectDir, ".state", "runtime", "active-task.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return ""
	}

	var task activeTaskJSON
	if err := json.Unmarshal(data, &task); err != nil {
		return ""
	}

	if task.FormatID != "" {
		return task.FormatID
	}
	return task.TaskID
}

// isPathFlowActive checks if a PathFlow session is active.
func (v *PromptValidator) isPathFlowActive() bool {
	if v.PathFlowChecker != nil {
		return v.PathFlowChecker(v.ProjectDir)
	}
	return defaultPathFlowCheck(v.ProjectDir)
}

// defaultPathFlowCheck looks for active PathFlow sessions using the session
// status file (pathflow-session-status.json).
func defaultPathFlowCheck(projectDir string) bool {
	sessionDir := filepath.Join(projectDir, ".state", "session")
	entries, err := os.ReadDir(sessionDir)
	if err != nil {
		return false
	}

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		pathflowDir := filepath.Join(sessionDir, entry.Name(), "pathflow")
		if session.IsPathflowActive(pathflowDir) {
			return true
		}
	}
	return false
}
