package session

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/codeflow/codeflow-cli/internal/ledger"
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
	ProcessChecker ProcessChecker

	// HomeDir overrides os.UserHomeDir for testing.
	HomeDir string

	// PPID is the parent process ID for lead identification.
	PPID int
}

// NewCleaner creates a Cleaner with production defaults.
func NewCleaner() *Cleaner {
	home, _ := os.UserHomeDir()
	return &Cleaner{
		Now:            func() time.Time { return time.Now().UTC() },
		ProcessChecker: osProcessChecker{},
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

// resolveSessionID determines the session ID from the env file or environment.
func (c *Cleaner) resolveSessionID(projectDir string) string {
	// Priority 1: env file.
	envFilePath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if data, err := os.ReadFile(envFilePath); err == nil {
		if sid := parseEnvFileSessionID(string(data)); sid != "" {
			return sid
		}
	}

	// Priority 2: CODEFLOW_SESSION_ID environment variable.
	if sid := os.Getenv("CODEFLOW_SESSION_ID"); sid != "" {
		return sid
	}

	// Priority 3: current-session-id file.
	csidPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if data, err := os.ReadFile(csidPath); err == nil {
		if sid := string(data); sid != "" {
			return sid
		}
	}

	return ""
}

// shouldSkipCleanup checks the pathflow guard. Returns true if cleanup should
// be skipped (teammate shutdown while lead is alive).
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

	if teamInfo.LeadPID <= 0 {
		result.msg("SessionEnd: PathFlow active but lead PID unknown -- proceeding with cleanup")
		return false
	}

	// Check 1: Is this the lead's own SessionEnd?
	if teamInfo.LeadPID == c.PPID {
		result.msg("SessionEnd: PathFlow active, PPID matches lead PID %d -- proceeding with cleanup (lead's own SessionEnd)", teamInfo.LeadPID)
		return false
	}

	// Check 2: Is the lead alive?
	if c.ProcessChecker.IsAlive(teamInfo.LeadPID) {
		result.msg("SessionEnd: PathFlow active, lead PID %d alive -- skipping cleanup (teammate shutdown)", teamInfo.LeadPID)
		return true
	}

	// Check 3: Lead is dead -- orphaned session.
	result.msg("SessionEnd: PathFlow active but lead PID %d dead -- proceeding with cleanup (orphaned session)", teamInfo.LeadPID)
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

// cleanRuntimeFiles removes env file and project temp files.
func (c *Cleaner) cleanRuntimeFiles(projectDir string, result *CleanupResult) {
	// Remove env file.
	envFile := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	_ = os.Remove(envFile)

	// Remove current-session-id.
	csidFile := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	_ = os.Remove(csidFile)
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
