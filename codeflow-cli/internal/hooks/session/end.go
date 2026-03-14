package session

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/codeflow/codeflow-cli/internal/session"
)

// endHookInput represents the JSON structure sent by Claude Code on stdin to
// SessionEnd hooks.
type endHookInput struct {
	SessionID      string `json:"session_id"`      // Claude's per-agent UUID (metadata only)
	TranscriptPath string `json:"transcript_path"` // Path to session transcript
}

// CleanupResult holds the output of a session-end cleanup operation.
type CleanupResult struct {
	SessionID           string
	PF7Valid            bool
	SentinelsCleaned    int
	TaskPreserved       bool
	TeamName            string
	EnvFileRemoved      bool
	TeamConfigRemoved   bool
	TeamTasksRemoved    bool
	SessionStateRemoved bool
	Warnings            []string
	Messages            []string
}

// Cleaner performs session-end cleanup with injectable dependencies.
type Cleaner struct {
	Now     func() time.Time
	HomeDir string
}

// NewCleaner creates a Cleaner with production defaults.
func NewCleaner() *Cleaner {
	home, _ := os.UserHomeDir()
	return &Cleaner{
		Now:     func() time.Time { return time.Now().UTC() },
		HomeDir: home,
	}
}

// EndCleanup performs all session cleanup steps.
func (c *Cleaner) EndCleanup(stdin io.Reader, projectDir string) (*CleanupResult, error) {
	if projectDir == "" {
		return nil, fmt.Errorf("session cleanup: empty project directory")
	}

	result := &CleanupResult{}
	logger := newCleanupLogger(projectDir, c.Now)

	input := parseEndStdin(stdin)

	sessionID := c.resolveSessionID(projectDir)
	if sessionID == "" || sessionID == "unknown" {
		result.warn("no session ID found, skipping cleanup")
		logger.log("cleanup_skipped", map[string]any{
			"reason":     "no_session_id",
			"session_id": sessionID,
		})
		return result, nil
	}
	result.SessionID = sessionID

	logger.log("cleanup_started", map[string]any{
		"session_id":       sessionID,
		"pid":              os.Getpid(),
		"stdin_session_id": input.SessionID,
	})

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)

	if c.shouldSkipCleanup(projectDir, sessionStateDir, sessionID, result) {
		pathflowDir := filepath.Join(sessionStateDir, "pathflow")
		status, _ := ReadPathflowSessionStatus(pathflowDir)
		skipFields := map[string]any{
			"reason":     "active_session",
			"session_id": sessionID,
		}
		if status != nil {
			skipFields["session_status"] = status.Status
			skipFields["team_name"] = status.TeamName
			skipFields["last_completed_phase"] = status.LastCompletedPhase
			teamCfgPath := filepath.Join(c.HomeDir, ".claude", "teams", status.TeamName, "config.json")
			_, cfgErr := os.Stat(teamCfgPath)
			skipFields["team_config_exists"] = cfgErr == nil
		}
		logger.log("cleanup_skipped", skipFields)
		return result, nil
	}

	result.PF7Valid = c.validatePF7(projectDir, sessionID, result)
	c.cleanPathflowSentinels(projectDir, sessionID, result)
	c.handleActiveTask(projectDir, result)

	teamName := c.readTeamName(sessionStateDir)
	result.TeamName = teamName

	c.cleanTeamArtifacts(teamName, sessionStateDir, result)
	c.cleanSessionState(sessionStateDir, sessionID, result)
	c.cleanRuntimeFiles(projectDir, sessionID, result)
	c.cleanProjectTemp(projectDir)

	logger.log("cleanup_completed", map[string]any{
		"session_id":            sessionID,
		"pf7_valid":             result.PF7Valid,
		"sentinels_cleaned":     result.SentinelsCleaned,
		"team_config_removed":   result.TeamConfigRemoved,
		"team_tasks_removed":    result.TeamTasksRemoved,
		"session_state_removed": result.SessionStateRemoved,
		"env_file_removed":      result.EnvFileRemoved,
	})

	return result, nil
}

// ValidatePF7 checks if the pathflow-pf-7 sentinel exists.
func ValidatePF7(sentinelDir string) (bool, []string) {
	pf7Path := filepath.Join(sentinelDir, "pathflow-pf-7")
	if _, err := os.Stat(pf7Path); err == nil {
		return true, nil
	}

	var warnings []string
	warnings = append(warnings, "Incomplete PF7 shutdown (pf-7 sentinel absent -- possible crash or skip)")

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

func (c *Cleaner) resolveSessionID(projectDir string) string {
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	sid, err := session.Current(runtimeDir)
	if err != nil {
		return ""
	}
	return sid
}

// shouldSkipCleanup checks the session status to determine whether cleanup
// should be skipped (teammate shutdown while session is still active).
// When team config is missing but sentinels exist, treats as a race condition
// rather than assuming the team was dissolved.
func (c *Cleaner) shouldSkipCleanup(projectDir, sessionStateDir, sessionID string, result *CleanupResult) bool {
	pathflowDir := filepath.Join(sessionStateDir, "pathflow")

	status, err := ReadPathflowSessionStatus(pathflowDir)
	if err != nil {
		result.msg("SessionEnd: Session status file unreadable -- proceeding with cleanup")
		return false
	}

	if status == nil {
		return false
	}

	switch status.Status {
	case "created":
		result.msg("SessionEnd: Session status is 'created' (no team) -- proceeding with cleanup")
		return false

	case "pf-complete":
		result.msg("SessionEnd: Session status is 'pf-complete' -- proceeding with cleanup")
		return false

	case "pf-started", "pf-in-progress":
		if status.TeamName == "" {
			result.msg("SessionEnd: Session active but no team name -- proceeding with cleanup")
			return false
		}

		teamCfgPath := filepath.Join(c.HomeDir, ".claude", "teams", status.TeamName, "config.json")
		if _, cfgErr := os.Stat(teamCfgPath); cfgErr != nil {
			// Config missing -- check for sentinel evidence before assuming dissolved.
			sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
			if hasActiveSentinels(sentinelDir) {
				result.msg("SessionEnd: Team %q config missing but sentinels exist -- skipping cleanup (possible race)", status.TeamName)
				return true
			}
			result.msg("SessionEnd: Session active but team %q config missing and no sentinels -- proceeding with cleanup (team dissolved)", status.TeamName)
			return false
		}

		if status.LastCompletedPhase == "pf-7" {
			result.msg("SessionEnd: Session active but PF7 already completed -- proceeding with cleanup")
			return false
		}

		result.msg("SessionEnd: Session active (status=%q, team=%q) -- skipping cleanup (teammate shutdown)", status.Status, status.TeamName)
		return true

	default:
		result.msg("SessionEnd: Unknown session status %q -- proceeding with cleanup", status.Status)
		return false
	}
}

// hasActiveSentinels checks if the sentinel directory exists and contains
// any sentinel files, indicating an active session.
func hasActiveSentinels(sentinelDir string) bool {
	entries, err := os.ReadDir(sentinelDir)
	if err != nil {
		return false
	}
	for _, e := range entries {
		if !e.IsDir() {
			return true
		}
	}
	return false
}

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

func (c *Cleaner) cleanPathflowSentinels(projectDir, sessionID string, result *CleanupResult) {
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if _, err := os.Stat(sentinelDir); err != nil {
		return
	}
	if err := os.RemoveAll(sentinelDir); err == nil {
		result.SentinelsCleaned++
	}
}

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

	_ = os.Remove(taskPath)
}

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
// Re-checks session status before deletion -- if status is still active
// (pf-started/pf-in-progress), skips deletion to avoid race conditions.
func (c *Cleaner) cleanTeamArtifacts(teamName, sessionStateDir string, result *CleanupResult) {
	if teamName == "" {
		return
	}

	// Re-check session status before deleting team artifacts.
	pathflowDir := filepath.Join(sessionStateDir, "pathflow")
	status, _ := ReadPathflowSessionStatus(pathflowDir)
	if status != nil && (status.Status == "pf-started" || status.Status == "pf-in-progress") {
		result.msg("SessionEnd: Skipping team artifact cleanup -- session still active (status=%q)", status.Status)
		return
	}

	teamDir := filepath.Join(c.HomeDir, ".claude", "teams", teamName)
	if _, err := os.Stat(teamDir); err == nil {
		_ = os.RemoveAll(teamDir)
		result.TeamConfigRemoved = true
		result.msg("SessionEnd: Removed team config: %s", teamName)
	}

	taskDir := filepath.Join(c.HomeDir, ".claude", "tasks", teamName)
	if _, err := os.Stat(taskDir); err == nil {
		_ = os.RemoveAll(taskDir)
		result.TeamTasksRemoved = true
		result.msg("SessionEnd: Removed task list: %s", teamName)
	}
}

func (c *Cleaner) cleanSessionState(sessionStateDir, sessionID string, result *CleanupResult) {
	if sessionID == "unknown" {
		return
	}
	if _, err := os.Stat(sessionStateDir); err != nil {
		return
	}
	if err := os.RemoveAll(sessionStateDir); err != nil {
		result.warn("session state cleanup error: %v", err)
	} else {
		result.SessionStateRemoved = true
	}
}

func (c *Cleaner) cleanRuntimeFiles(projectDir, sessionID string, result *CleanupResult) {
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")

	envPath := filepath.Join(runtimeDir, session.EnvFile)
	envExisted := false
	if _, err := os.Stat(envPath); err == nil {
		envExisted = true
	}

	session.CleanRuntimeFiles(runtimeDir, sessionID)

	if envExisted {
		if _, err := os.Stat(envPath); os.IsNotExist(err) {
			result.EnvFileRemoved = true
		}
	}
}

func (c *Cleaner) cleanProjectTemp(projectDir string) {
	projectName := filepath.Base(projectDir)
	if projectName == "" {
		projectName = "codeflow"
	}
	tmpDir := filepath.Join(os.TempDir(), "claude", projectName)

	absProject, _ := filepath.Abs(projectDir)
	absTmp, _ := filepath.Abs(tmpDir)
	if absProject != "" && absTmp != "" && absProject == absTmp {
		return
	}

	_ = os.RemoveAll(tmpDir)
}

func (r *CleanupResult) warn(format string, args ...any) {
	r.Warnings = append(r.Warnings, fmt.Sprintf(format, args...))
}

func (r *CleanupResult) msg(format string, args ...any) {
	r.Messages = append(r.Messages, fmt.Sprintf(format, args...))
}

// cleanupLogger writes structured JSONL events to .state/logs/sessions/cleanup-{date}.jsonl.
type cleanupLogger struct {
	logDir string
	now    func() time.Time
}

func newCleanupLogger(projectDir string, now func() time.Time) *cleanupLogger {
	return &cleanupLogger{
		logDir: filepath.Join(projectDir, ".state", "logs", "sessions"),
		now:    now,
	}
}

func (l *cleanupLogger) log(event string, fields map[string]any) {
	if fields == nil {
		fields = make(map[string]any)
	}
	fields["event"] = event
	fields["timestamp"] = l.now().Format(time.RFC3339)

	data, err := json.Marshal(fields)
	if err != nil {
		return
	}

	if err := os.MkdirAll(l.logDir, 0o755); err != nil {
		return
	}

	date := l.now().Format("2006-01-02")
	logFile := filepath.Join(l.logDir, "cleanup-"+date+".jsonl")

	f, err := os.OpenFile(logFile, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()

	_, _ = f.Write(append(data, '\n'))
}
