package session

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/codeflow/codeflow-cli/internal/session"
)

// endHookInput represents the JSON structure sent by Claude Code on stdin to
// SessionEnd hooks.
type endHookInput struct {
	SessionID      string `json:"session_id"`       // Claude's per-agent UUID (metadata only)
	TranscriptPath string `json:"transcript_path"`  // Path to session transcript
}

// CleanupResult holds the output of a session-end cleanup operation.
type CleanupResult struct {
	// SessionID is the CODEFLOW_SESSION_ID used for this session.
	SessionID string

	// PF7Valid is true when the pathflow-pf-7 sentinel was found.
	PF7Valid bool

	// SentinelsCleaned is the count of sentinel files removed.
	SentinelsCleaned int

	// TaskPreserved is true when an active task was kept for the next session.
	TaskPreserved bool

	// TeamName is the team name read from pathflow-team.json (if any).
	TeamName string

	// Warnings collects non-fatal messages emitted during cleanup.
	Warnings []string

	// Messages collects informational messages for stderr output.
	Messages []string
}

// Cleaner performs session-end cleanup with injectable dependencies.
type Cleaner struct {
	// Now returns the current time. Override in tests for deterministic output.
	Now func() time.Time

	// ProcessChecker checks if a PID is alive.
	// Retained for backward compatibility but no longer used for lead liveness.
	ProcessChecker ProcessChecker

	// TmuxChecker checks if a tmux pane is alive.
	// Used for session liveness detection via team config pane IDs.
	TmuxChecker TmuxChecker

	// HomeDir overrides os.UserHomeDir for testing.
	HomeDir string

	// PPID is the parent process ID.
	// Informational only — liveness checks use tmux pane status.
	PPID int
}

// NewCleaner creates a Cleaner with production defaults.
func NewCleaner() *Cleaner {
	home, _ := os.UserHomeDir()
	return &Cleaner{
		Now:            func() time.Time { return time.Now().UTC() },
		ProcessChecker: osProcessChecker{},
		TmuxChecker:    osTmuxChecker{},
		HomeDir:        home,
		PPID:           os.Getppid(),
	}
}

// EndCleanup performs all session cleanup steps. It reads hook JSON from stdin,
// validates PF7 completion, cleans up sentinels, archives session state,
// removes stale runtime files, and writes a session_end ledger event.
func (c *Cleaner) EndCleanup(stdin io.Reader, projectDir string) (*CleanupResult, error) {
	if projectDir == "" {
		return nil, fmt.Errorf("session cleanup: empty project directory")
	}

	result := &CleanupResult{}

	// --- Section 1: Parse stdin JSON ---
	input := parseEndStdin(stdin)
	_ = input // transcript_path available for future use

	// --- Section 2: Resolve session ID ---
	sessionID := c.resolveSessionID(projectDir)
	if sessionID == "" || sessionID == "unknown" {
		result.warn("no session ID found, skipping cleanup")
		return result, nil
	}
	result.SessionID = sessionID

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)

	// --- Section 3: PathFlow guard ---
	// If pathflow-active flag exists, check if the lead is still alive.
	// If lead is alive and this is NOT the lead, skip cleanup (teammate shutdown).
	if c.shouldSkipCleanup(sessionStateDir, result) {
		return result, nil
	}

	// --- Section 4: PF7 diagnostic ---
	result.PF7Valid = c.validatePF7(projectDir, sessionID, result)

	// --- Section 5: PathFlow sentinel cleanup ---
	c.cleanPathflowSentinels(projectDir, sessionID, result)

	// --- Section 6: Active task preservation ---
	c.handleActiveTask(projectDir, result)

	// --- Section 8: Read team info before removing session dir ---
	teamName := c.readTeamName(sessionStateDir)
	result.TeamName = teamName

	// --- Section 9: Team config/task list backstop cleanup ---
	c.cleanTeamArtifacts(teamName, result)

	// --- Section 10: Session state directory cleanup ---
	c.cleanSessionState(sessionStateDir, sessionID, result)

	// --- Section 11: Runtime file cleanup ---
	c.cleanRuntimeFiles(projectDir, result)

	// --- Section 12: Project temp directory cleanup ---
	c.cleanProjectTemp(projectDir)

	// --- Section 13: Write session_end ledger event ---
	c.writeLedgerEvent(projectDir, sessionID, result)

	return result, nil
}

// ValidatePF7 checks if the pathflow-pf-7 sentinel exists, indicating a clean
// PF7 shutdown. Returns true if the sentinel exists, along with any warnings.
func ValidatePF7(sentinelDir string) (bool, []string) {
	pf7Path := filepath.Join(sentinelDir, "pathflow-pf-7")
	if _, err := os.Stat(pf7Path); err == nil {
		return true, nil
	}

	var warnings []string
	warnings = append(warnings, "Incomplete PF7 shutdown (pf-7 sentinel absent -- possible crash or skip)")

	// Check which phase sentinels do exist for diagnostic context.
	entries, err := os.ReadDir(sentinelDir)
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("could not read sentinel directory: %v", err))
		return false, warnings
	}

	var found []string
	for _, e := range entries {
		if !e.IsDir() {
			found = append(found, e.Name())
		}
	}
	if len(found) > 0 {
		warnings = append(warnings, fmt.Sprintf("existing sentinels: %v", found))
	}

	return false, warnings
}

// parseEndStdin reads and parses the hook JSON from stdin.
func parseEndStdin(r io.Reader) endHookInput {
	var input endHookInput
	if r == nil {
		return input
	}
	data, err := io.ReadAll(r)
	if err != nil || len(data) == 0 {
		return input
	}
	_ = json.Unmarshal(data, &input)
	return input
}

// resolveSessionID determines the session ID via session.Current()
// (single authoritative resolution: env var → codeflow-env.sh).
func (c *Cleaner) resolveSessionID(projectDir string) string {
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	sid, err := session.Current(runtimeDir)
	if err != nil {
		return ""
	}
	return sid
}

// shouldSkipCleanup checks the pathflow guard. Returns true if cleanup should
// be skipped (teammate shutdown while session is still active).
//
// Liveness is determined by tmux pane status from the team config at
// ~/.claude/teams/{team_name}/config.json, not by PID checks. The process
// chain is: claude (persistent) -> /bin/zsh (ephemeral) -> codeflow binary,
// so os.Getppid() returns an ephemeral shell PID that dies immediately after
// each hook invocation, making PID-based checks effectively dead code.
func (c *Cleaner) shouldSkipCleanup(sessionStateDir string, result *CleanupResult) bool {
	flagPath := filepath.Join(sessionStateDir, "pathflow", "is-pathflow-active")
	if _, err := os.Stat(flagPath); err != nil {
		// No pathflow flag -- proceed with cleanup.
		return false
	}

	teamFilePath := filepath.Join(sessionStateDir, "pathflow", "pathflow-team.json")
	teamData, err := os.ReadFile(teamFilePath)
	if err != nil {
		// No team file -- proceed with cleanup.
		result.msg("SessionEnd: PathFlow active but no team file -- proceeding with cleanup (no lead to protect)")
		return false
	}

	var teamInfo pathflowTeamJSON
	if err := json.Unmarshal(teamData, &teamInfo); err != nil {
		result.msg("SessionEnd: PathFlow active but team file unreadable -- proceeding with cleanup")
		return false
	}

	// Use tmux pane liveness from team config instead of PID checks.
	// The lead_pid field is informational only (ephemeral shell PID).
	if teamInfo.TeamName == "" {
		result.msg("SessionEnd: PathFlow active but no team name -- proceeding with cleanup")
		return false
	}

	alive := c.hasLiveTeamPanes(teamInfo.TeamName)
	if alive {
		result.msg("SessionEnd: PathFlow active, team %q has live tmux panes -- skipping cleanup (teammate shutdown)", teamInfo.TeamName)
		return true
	}

	result.msg("SessionEnd: PathFlow active but no live tmux panes for team %q -- proceeding with cleanup (session ended)", teamInfo.TeamName)
	return false
}

// hasLiveTeamPanes checks whether any tmux pane in the team config is alive.
// Returns false if the config is missing, unreadable, or has no live panes.
func (c *Cleaner) hasLiveTeamPanes(teamName string) bool {
	cfgPath := filepath.Join(c.HomeDir, ".claude", "teams", teamName, "config.json")
	data, err := os.ReadFile(cfgPath)
	if err != nil {
		return false
	}

	var cfg struct {
		Members []struct {
			TmuxPaneID string `json:"tmuxPaneId"`
		} `json:"members"`
	}
	if err := json.Unmarshal(data, &cfg); err != nil {
		return false
	}

	for _, m := range cfg.Members {
		if c.TmuxChecker.IsPaneAlive(m.TmuxPaneID) {
			return true
		}
	}
	return false
}

// validatePF7 checks for PF7 completion and logs the result.
func (c *Cleaner) validatePF7(projectDir, sessionID string, result *CleanupResult) bool {
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	valid, warnings := ValidatePF7(sentinelDir)

	if valid {
		result.msg("SessionEnd: Clean PF7 shutdown (all phases completed)")
	} else {
		for _, w := range warnings {
			result.msg("SessionEnd: %s", w)
		}
	}

	return valid
}

// cleanPathflowSentinels removes all PathFlow sentinels for this session.
func (c *Cleaner) cleanPathflowSentinels(projectDir, sessionID string, result *CleanupResult) {
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if _, err := os.Stat(sentinelDir); err != nil {
		return
	}
	if err := os.RemoveAll(sentinelDir); err == nil {
		result.SentinelsCleaned++
	}
}

// handleActiveTask preserves active tasks in_progress, removes completed ones.
func (c *Cleaner) handleActiveTask(projectDir string, result *CleanupResult) {
	taskPath := filepath.Join(projectDir, ".state", "runtime", "active-task.json")
	data, err := os.ReadFile(taskPath)
	if err != nil {
		return
	}

	var task struct {
		Status string `json:"status"`
		TaskID string `json:"task_id"`
	}
	if err := json.Unmarshal(data, &task); err != nil {
		_ = os.Remove(taskPath)
		return
	}

	if task.Status == "in_progress" {
		result.TaskPreserved = true
		result.msg("SessionEnd: Task %s preserved for next session", task.TaskID)
		return
	}

	// Not in_progress -- safe to remove.
	_ = os.Remove(taskPath)
}

// readTeamName extracts the team_name from pathflow-team.json.
func (c *Cleaner) readTeamName(sessionStateDir string) string {
	teamFilePath := filepath.Join(sessionStateDir, "pathflow", "pathflow-team.json")
	data, err := os.ReadFile(teamFilePath)
	if err != nil {
		return ""
	}

	var teamInfo struct {
		TeamName string `json:"team_name"`
	}
	if err := json.Unmarshal(data, &teamInfo); err != nil {
		return ""
	}
	return teamInfo.TeamName
}

// cleanTeamArtifacts removes team config and task list directories as a backstop.
func (c *Cleaner) cleanTeamArtifacts(teamName string, result *CleanupResult) {
	if teamName == "" {
		return
	}

	teamDir := filepath.Join(c.HomeDir, ".claude", "teams", teamName)
	if _, err := os.Stat(teamDir); err == nil {
		_ = os.RemoveAll(teamDir)
		result.msg("SessionEnd: Removed team config: %s", teamName)
	}

	taskDir := filepath.Join(c.HomeDir, ".claude", "tasks", teamName)
	if _, err := os.Stat(taskDir); err == nil {
		_ = os.RemoveAll(taskDir)
		result.msg("SessionEnd: Removed task list: %s", teamName)
	}
}

// cleanSessionState removes the session state directory.
func (c *Cleaner) cleanSessionState(sessionStateDir, sessionID string, result *CleanupResult) {
	if sessionID == "unknown" {
		return
	}
	if _, err := os.Stat(sessionStateDir); err != nil {
		return
	}
	if err := os.RemoveAll(sessionStateDir); err != nil {
		result.warn("session state cleanup error: %v", err)
	}
}

// cleanRuntimeFiles removes runtime session files via the session package
// (single authoritative owner of session file lifecycle).
func (c *Cleaner) cleanRuntimeFiles(projectDir string, result *CleanupResult) {
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	session.CleanRuntimeFiles(runtimeDir)
}

// cleanProjectTemp removes the project temp directory.
func (c *Cleaner) cleanProjectTemp(projectDir string) {
	projectName := filepath.Base(projectDir)
	if projectName == "" {
		projectName = "codeflow"
	}
	tmpDir := filepath.Join(os.TempDir(), "claude", projectName)

	// Guard: never rm -rf the project root.
	absProject, _ := filepath.Abs(projectDir)
	absTmp, _ := filepath.Abs(tmpDir)
	if absProject != "" && absTmp != "" && absProject == absTmp {
		return
	}

	_ = os.RemoveAll(tmpDir)
}

// writeLedgerEvent writes a session_end event to sessions.jsonl.
func (c *Cleaner) writeLedgerEvent(projectDir, sessionID string, result *CleanupResult) {
	ledgerDir := filepath.Join(projectDir, ".state", "ledger")
	w, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		result.warn("ledger writer creation error: %v", err)
		return
	}

	now := c.Now()
	event := ledger.Event{
		EventType: "session_end",
		SessionID: sessionID,
		Timestamp: now.Format(time.RFC3339),
		Data: map[string]any{
			"pf7_valid":          result.PF7Valid,
			"sentinels_cleaned":  result.SentinelsCleaned,
			"task_preserved":     result.TaskPreserved,
			"cleanup_completed":  true,
		},
	}

	if err := w.AppendEvent(event); err != nil {
		result.warn("ledger event write error: %v", err)
	}
}

// warn adds a warning message.
func (r *CleanupResult) warn(format string, args ...any) {
	r.Warnings = append(r.Warnings, fmt.Sprintf(format, args...))
}

// msg adds an informational message.
func (r *CleanupResult) msg(format string, args ...any) {
	r.Messages = append(r.Messages, fmt.Sprintf(format, args...))
}
