package session

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/pathflow"
)

// sessionIDRe validates the CODEFLOW_SESSION_ID format: ses-{13-digit-timestamp}{12-hex-chars}.
var sessionIDRe = regexp.MustCompile(`^ses-[0-9]{13}[a-f0-9]{12}$`)

// hookInput represents the JSON structure sent by Claude Code on stdin to
// SessionStart hooks.
type hookInput struct {
	SessionID string `json:"session_id"` // Claude's per-agent UUID (metadata only)
	Source    string `json:"source"`     // startup, resume, compact, clear
}

// InitResult holds the output of a successful session initialization.
type InitResult struct {
	// SessionID is the CODEFLOW_SESSION_ID used for this session.
	SessionID string

	// IsResume is true when an existing pathflow-active flag was found.
	IsResume bool

	// IsTeammate is true when an existing lead PID is alive (teammate mode).
	IsTeammate bool

	// EnvVars contains environment variables to output for the hook framework.
	EnvVars map[string]string

	// Warnings collects non-fatal messages emitted during initialization.
	Warnings []string

	// Messages collects informational messages for stdout output.
	Messages []string
}

// ProcessChecker abstracts PID liveness checks for testability.
type ProcessChecker interface {
	IsAlive(pid int) bool
}

// TmuxChecker abstracts tmux pane liveness checks for testability.
type TmuxChecker interface {
	IsPaneAlive(paneID string) bool
}

// osProcessChecker uses kill -0 to check PID liveness.
type osProcessChecker struct{}

func (osProcessChecker) IsAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	p, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	// On Unix, FindProcess always succeeds. Use Signal(0) to check liveness.
	return p.Signal(nil) == nil
}

// osTmuxChecker uses tmux list-panes to check pane liveness.
type osTmuxChecker struct{}

func (osTmuxChecker) IsPaneAlive(paneID string) bool {
	if paneID == "" {
		return false
	}
	out, err := exec.Command("tmux", "list-panes", "-a", "-F", "#{pane_id}").Output()
	if err != nil {
		return false
	}
	for _, line := range strings.Split(string(out), "\n") {
		if strings.TrimSpace(line) == paneID {
			return true
		}
	}
	return false
}

// Initializer performs session initialization with injectable dependencies.
type Initializer struct {
	// Now returns the current time. Override in tests for deterministic output.
	Now func() time.Time

	// ProcessChecker checks if a PID is alive.
	ProcessChecker ProcessChecker

	// TmuxChecker checks if a tmux pane is alive.
	TmuxChecker TmuxChecker

	// PPID is the parent process ID for pathflow-team.json updates.
	PPID int

	// HomeDir overrides os.UserHomeDir for testing.
	HomeDir string
}

// NewInitializer creates an Initializer with production defaults.
func NewInitializer() *Initializer {
	home, _ := os.UserHomeDir()
	return &Initializer{
		Now:            func() time.Time { return time.Now().UTC() },
		ProcessChecker: osProcessChecker{},
		TmuxChecker:    osTmuxChecker{},
		PPID:           os.Getppid(),
		HomeDir:        home,
	}
}

// StartInit performs all session initialization steps. It reads hook JSON
// from stdin, detects stale sessions, generates/loads a session ID, creates
// directories, initializes PathFlow state, and returns the result.
func (init_ *Initializer) StartInit(stdin io.Reader, projectDir string) (*InitResult, error) {
	if projectDir == "" {
		return nil, fmt.Errorf("session init: empty project directory")
	}

	result := &InitResult{
		EnvVars:  make(map[string]string),
		Warnings: nil,
		Messages: nil,
	}

	// --- Section 1: Parse stdin JSON ---
	input := parseStdin(stdin)

	// --- Section 1b: PID-based stale session cleanup ---
	envFilePath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	existingSID, teamMode, err := init_.handleStaleCleanup(projectDir, envFilePath, input.Source)
	if err != nil {
		result.warn("stale cleanup error: %v", err)
	}
	if teamMode {
		result.IsTeammate = true
		result.SessionID = existingSID
		result.msg("TEAMMATE MODE: You are a teammate joining session %s.", existingSID)
		result.msg("Lead PID is alive. Minimal init applied.")
		result.msg("Checkpoint and pathflow flag creation skipped (lead handles those).")
		result.EnvVars["CODEFLOW_SESSION_ID"] = existingSID
		result.EnvVars["CF_PROJECT_ROOT"] = filepath.Base(projectDir)
		return result, nil
	}

	// --- Section 2: Session ID generation ---
	sessionID := existingSID
	if sessionID == "" {
		sessionID = os.Getenv("CODEFLOW_SESSION_ID")
	}
	if sessionID == "" {
		sessionID = init_.generateSessionID()
		// Write env file atomically
		if err := init_.writeEnvFile(envFilePath, sessionID, projectDir); err != nil {
			result.warn("env file write error: %v", err)
		}
	}
	result.SessionID = sessionID
	result.EnvVars["CODEFLOW_SESSION_ID"] = sessionID
	result.EnvVars["CF_PROJECT_ROOT"] = filepath.Base(projectDir)

	// --- Section 3: Directory creation ---
	init_.createDirectories(projectDir, sessionID)

	// --- Section 4: Stale session detection (warning-only) ---
	staleWarnings := init_.detectStaleSessions(projectDir, sessionID)
	for _, w := range staleWarnings {
		result.warn("%s", w)
	}

	// --- Section 5: Orphan sentinel sweep ---
	init_.sweepOrphanSentinels(projectDir, sessionID)

	// --- Section 6: Active task context expiry ---
	init_.cleanupActiveTask(projectDir)

	// --- Section 7: PathFlow flag creation ---
	isResume := init_.createPathFlowFlag(projectDir, sessionID, result)
	result.IsResume = isResume

	// --- Section 7c: Checkpoint pre-initialization ---
	init_.initCheckpoint(projectDir, sessionID, result)

	// --- Section 8: Session metadata ---
	init_.writeSessionMetadata(projectDir, sessionID, input, result)

	// --- Section 9: Stale team detection ---
	teamWarnings := init_.detectStaleTeams()
	for _, w := range teamWarnings {
		result.warn("%s", w)
	}

	// --- Section 10: Compact recovery detection ---
	init_.detectCompactRecovery(projectDir, input.Source, result)

	// --- Section 11: Project temp directory ---
	init_.createProjectTempDir(projectDir, result)

	// --- Write current-session-id ---
	sessionIDPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if err := os.WriteFile(sessionIDPath, []byte(sessionID), 0o644); err != nil {
		result.warn("current-session-id write error: %v", err)
	}

	return result, nil
}

// parseStdin reads and parses the hook JSON from stdin.
func parseStdin(r io.Reader) hookInput {
	var input hookInput
	if r == nil {
		return hookInput{Source: "unknown"}
	}
	data, err := io.ReadAll(r)
	if err != nil || len(data) == 0 {
		return hookInput{Source: "unknown"}
	}
	if err := json.Unmarshal(data, &input); err != nil {
		return hookInput{Source: "unknown"}
	}
	if input.Source == "" {
		input.Source = "unknown"
	}
	return input
}

// pathflowTeamJSON represents the pathflow-team.json file structure.
type pathflowTeamJSON struct {
	LeadPID  int    `json:"lead_pid"`
	TeamName string `json:"team_name"`
}

// handleStaleCleanup checks for stale sessions and performs cleanup.
// Returns (existingSessionID, isTeammateMode, error).
func (init_ *Initializer) handleStaleCleanup(projectDir, envFilePath, source string) (string, bool, error) {
	data, err := os.ReadFile(envFilePath)
	if err != nil {
		// No env file -- nothing to clean up.
		return "", false, nil
	}

	// Parse existing session ID from env file.
	oldSID := parseEnvFileSessionID(string(data))
	if oldSID == "" {
		return "", false, nil
	}

	// Validate session ID format.
	if !sessionIDRe.MatchString(oldSID) {
		if source == "startup" || source == "unknown" {
			// Discard invalid env file for fresh startup.
			_ = os.Remove(envFilePath)
			return "", false, nil
		}
		// Non-startup: preserve invalid SID for continuation.
		return oldSID, false, nil
	}

	// Check for pathflow-team.json.
	teamFilePath := filepath.Join(projectDir, ".state", "session", oldSID, "pathflow", "pathflow-team.json")
	teamData, err := os.ReadFile(teamFilePath)
	if err != nil {
		// No team file -- check for pathflow-active flag.
		return init_.handleNoTeamFile(projectDir, envFilePath, oldSID, source)
	}

	var teamInfo pathflowTeamJSON
	if err := json.Unmarshal(teamData, &teamInfo); err != nil {
		return init_.handleNoTeamFile(projectDir, envFilePath, oldSID, source)
	}

	// Check if lead PID is alive.
	if teamInfo.LeadPID > 0 && init_.ProcessChecker.IsAlive(teamInfo.LeadPID) {
		// Lead is alive -- teammate mode.
		return oldSID, true, nil
	}

	// Lead PID is dead.
	if source == "startup" || source == "unknown" {
		// Fresh startup with dead PID -- full cleanup.
		init_.cleanupStaleSession(projectDir, envFilePath, oldSID, teamInfo.TeamName)
		return "", false, nil
	}

	// Compact/resume/clear with dead PID -- update lead_pid.
	init_.updateLeadPID(teamFilePath, teamData)
	return oldSID, false, nil
}

// handleNoTeamFile handles the case where pathflow-team.json doesn't exist.
func (init_ *Initializer) handleNoTeamFile(projectDir, envFilePath, oldSID, source string) (string, bool, error) {
	flagPath := filepath.Join(projectDir, ".state", "session", oldSID, "pathflow", "is-pathflow-active")
	if _, err := os.Stat(flagPath); err != nil {
		// No flag either -- orphan env file, clean it.
		_ = os.Remove(envFilePath)
		return "", false, nil
	}

	// Flag exists but no team file.
	if source == "startup" || source == "unknown" {
		// Pre-TeamCreate crash -- cleanup.
		sessionDir := filepath.Join(projectDir, ".state", "session", oldSID)
		_ = os.RemoveAll(sessionDir)
		sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", oldSID)
		_ = os.RemoveAll(sentinelDir)
		_ = os.Remove(envFilePath)
		return "", false, nil
	}

	// Compact/resume/clear -- proceed with existing SID.
	return oldSID, false, nil
}

// cleanupStaleSession removes all artifacts of a stale session.
func (init_ *Initializer) cleanupStaleSession(projectDir, envFilePath, sid, teamName string) {
	// Remove stale team config and task list.
	if teamName != "" {
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "teams", teamName))
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
	}

	// Remove session directory.
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "session", sid))

	// Remove sentinels.
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid))

	// Remove env file, active task, and session ID.
	_ = os.Remove(envFilePath)
	_ = os.Remove(filepath.Join(projectDir, ".state", "runtime", "active-task.json"))
	_ = os.Remove(filepath.Join(projectDir, ".state", "runtime", "current-session-id"))
}

// updateLeadPID updates the lead_pid in pathflow-team.json for compact recovery.
func (init_ *Initializer) updateLeadPID(teamFilePath string, teamData []byte) {
	var raw map[string]any
	if err := json.Unmarshal(teamData, &raw); err != nil {
		return
	}
	raw["lead_pid"] = init_.PPID
	updated, err := json.Marshal(raw)
	if err != nil {
		return
	}
	// Atomic write via tmp+rename.
	tmpPath := teamFilePath + ".tmp"
	if err := os.WriteFile(tmpPath, updated, 0o644); err != nil {
		return
	}
	if err := os.Rename(tmpPath, teamFilePath); err != nil {
		_ = os.Remove(tmpPath)
	}
}

// parseEnvFileSessionID extracts CODEFLOW_SESSION_ID from an env file.
func parseEnvFileSessionID(content string) string {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		// Match: export CODEFLOW_SESSION_ID='...'
		if strings.HasPrefix(line, "export CODEFLOW_SESSION_ID=") {
			val := strings.TrimPrefix(line, "export CODEFLOW_SESSION_ID=")
			val = strings.Trim(val, "'\"")
			return val
		}
	}
	return ""
}

// generateSessionID creates a new session ID in the format ses-{13-digit-timestamp}{12-hex-chars}.
func (init_ *Initializer) generateSessionID() string {
	ts := init_.Now().UnixMilli()
	// Pad timestamp to 13 digits.
	tsStr := fmt.Sprintf("%013d", ts)

	// Generate 6 random bytes -> 12 hex chars.
	b := make([]byte, 6)
	if _, err := rand.Read(b); err != nil {
		// Fallback: use time-based pseudo-random.
		b = []byte(fmt.Sprintf("%06x", ts%0xFFFFFF))[:6]
	}
	hexStr := hex.EncodeToString(b)

	return "ses-" + tsStr + hexStr
}

// writeEnvFile atomically writes the codeflow-env.sh file.
func (init_ *Initializer) writeEnvFile(envFilePath, sessionID, projectDir string) error {
	dir := filepath.Dir(envFilePath)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("creating env file dir: %w", err)
	}

	projectName := filepath.Base(projectDir)
	content := fmt.Sprintf("export CODEFLOW_SESSION_ID='%s'\nexport CF_PROJECT_ROOT='%s'\n", sessionID, projectName)

	tmpPath := envFilePath + ".tmp"
	if err := os.WriteFile(tmpPath, []byte(content), 0o644); err != nil {
		return fmt.Errorf("writing env temp file: %w", err)
	}
	if err := os.Rename(tmpPath, envFilePath); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("renaming env file: %w", err)
	}
	return nil
}

// createDirectories ensures all required .state/ directories exist.
func (init_ *Initializer) createDirectories(projectDir, sessionID string) {
	dirs := []string{
		filepath.Join(projectDir, ".state", "logs", "sessions"),
		filepath.Join(projectDir, ".state", "logs", "security"),
		filepath.Join(projectDir, ".state", "db"),
		filepath.Join(projectDir, ".state", "runtime"),
		filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID),
		filepath.Join(projectDir, ".state", "session", sessionID),
		filepath.Join(projectDir, ".state", "session"),
	}
	for _, d := range dirs {
		_ = os.MkdirAll(d, 0o755)
	}
}

// detectStaleSessions scans for sessions with pathflow-active flags and dead tmux panes.
func (init_ *Initializer) detectStaleSessions(projectDir, currentSID string) []string {
	sessionDir := filepath.Join(projectDir, ".state", "session")
	entries, err := os.ReadDir(sessionDir)
	if err != nil {
		return nil
	}

	var warnings []string
	for _, e := range entries {
		if !e.IsDir() || !strings.HasPrefix(e.Name(), "ses-") {
			continue
		}
		sid := e.Name()
		if sid == currentSID {
			continue
		}

		flagPath := filepath.Join(sessionDir, sid, "pathflow", "is-pathflow-active")
		if _, err := os.Stat(flagPath); err != nil {
			continue
		}

		// Check if session is stale by reading team info from the flag file.
		if init_.isSessionStale(flagPath) {
			warnings = append(warnings, fmt.Sprintf("STALE SESSION: %s (pathflow-active flag set, no live tmux panes)", sid))
		}
	}

	if len(warnings) > 0 {
		warnings = append(warnings, "Run '/cf-cleanup --sessions' to clean up stale session state.")
	}
	return warnings
}

// isSessionStale checks if a session's tmux panes are dead.
func (init_ *Initializer) isSessionStale(flagPath string) bool {
	data, err := os.ReadFile(flagPath)
	if err != nil {
		return true
	}

	var flag struct {
		TeamName string `json:"team_name"`
	}
	if err := json.Unmarshal(data, &flag); err != nil || flag.TeamName == "" {
		return true
	}

	// Read team config.
	teamCfgPath := filepath.Join(init_.HomeDir, ".claude", "teams", flag.TeamName, "config.json")
	cfgData, err := os.ReadFile(teamCfgPath)
	if err != nil {
		return true
	}

	var cfg struct {
		Members []struct {
			TmuxPaneID string `json:"tmuxPaneId"`
		} `json:"members"`
	}
	if err := json.Unmarshal(cfgData, &cfg); err != nil {
		return true
	}

	// If any pane is alive, session is not stale.
	for _, m := range cfg.Members {
		if init_.TmuxChecker.IsPaneAlive(m.TmuxPaneID) {
			return false
		}
	}
	return true
}

// sweepOrphanSentinels removes pathflow sentinel dirs that have no corresponding session dir.
func (init_ *Initializer) sweepOrphanSentinels(projectDir, currentSID string) {
	sentinelBase := filepath.Join(projectDir, ".state", "sentinels", "pathflow")
	entries, err := os.ReadDir(sentinelBase)
	if err != nil {
		return
	}

	for _, e := range entries {
		if !e.IsDir() || !strings.HasPrefix(e.Name(), "ses-") {
			continue
		}
		sid := e.Name()
		if sid == currentSID {
			continue
		}
		sessionDir := filepath.Join(projectDir, ".state", "session", sid)
		if _, err := os.Stat(sessionDir); os.IsNotExist(err) {
			_ = os.RemoveAll(filepath.Join(sentinelBase, sid))
		}
	}
}

// cleanupActiveTask removes completed/stale active-task.json files.
func (init_ *Initializer) cleanupActiveTask(projectDir string) {
	taskPath := filepath.Join(projectDir, ".state", "runtime", "active-task.json")
	data, err := os.ReadFile(taskPath)
	if err != nil {
		return
	}

	var task struct {
		Status    string `json:"status"`
		UpdatedAt string `json:"updated_at"`
	}
	if err := json.Unmarshal(data, &task); err != nil {
		_ = os.Remove(taskPath)
		return
	}

	// Remove completed tasks.
	if task.Status == "completed" || task.Status == "done" {
		_ = os.Remove(taskPath)
		return
	}

	// Remove stale tasks (>24h old).
	if task.UpdatedAt != "" {
		if t, err := time.Parse(time.RFC3339, task.UpdatedAt); err == nil {
			if init_.Now().Sub(t) > 24*time.Hour {
				_ = os.Remove(taskPath)
			}
		}
	}
}

// pathflowFlag represents the is-pathflow-active JSON file.
type pathflowFlag struct {
	SessionID     string `json:"session_id"`
	TeamName      string `json:"team_name"`
	CreatedAt     string `json:"created_at"`
	TrackingLevel string `json:"tracking_level"`
}

// createPathFlowFlag creates the is-pathflow-active flag file.
// Returns true if the flag already existed (resume scenario).
func (init_ *Initializer) createPathFlowFlag(projectDir, sessionID string, result *InitResult) bool {
	pathflowDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	flagPath := filepath.Join(pathflowDir, "is-pathflow-active")

	// Guard: if flag already exists, this is a resume.
	if _, err := os.Stat(flagPath); err == nil {
		return true
	}

	if err := os.MkdirAll(pathflowDir, 0o755); err != nil {
		result.warn("pathflow dir creation error: %v", err)
		return false
	}

	flag := pathflowFlag{
		SessionID:     sessionID,
		TeamName:      "",
		CreatedAt:     init_.Now().Format("2006-01-02T15:04:05.000Z"),
		TrackingLevel: "pending",
	}

	data, err := json.Marshal(flag)
	if err != nil {
		result.warn("pathflow flag marshal error: %v", err)
		return false
	}

	if err := os.WriteFile(flagPath, data, 0o644); err != nil {
		result.warn("pathflow flag write error: %v", err)
		return false
	}
	return false
}

// initCheckpoint pre-initializes the checkpoint file for all 7 phases.
func (init_ *Initializer) initCheckpoint(projectDir, sessionID string, result *InitResult) {
	checkpointPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-phase-tasks.json")
	configPath := filepath.Join(projectDir, ".codeflow", "config", "pathflow", "pathflow-config.json")

	cp := &pathflow.Checkpoint{
		Now: init_.Now,
	}
	if err := cp.InitAllPhases(checkpointPath, configPath); err != nil {
		result.warn("checkpoint init error: %v", err)
	}
}

// writeSessionMetadata writes session-{SID}.meta JSON file.
func (init_ *Initializer) writeSessionMetadata(projectDir, sessionID string, input hookInput, result *InitResult) {
	metaPath := filepath.Join(projectDir, ".state", "logs", "sessions", "session-"+sessionID+".meta")

	gitBranch := detectGitBranch(projectDir)
	gitCommit := detectGitCommit(projectDir)
	user := os.Getenv("USER")
	if user == "" {
		user = "unknown"
	}

	meta := map[string]any{
		"session_id":    sessionID,
		"claude_uuid":  input.SessionID,
		"source":       input.Source,
		"started_at":   init_.Now().Format("2006-01-02T15:04:05.000Z"),
		"started_epoch": init_.Now().Unix(),
		"repo_root":    projectDir,
		"git_branch":   gitBranch,
		"git_commit":   gitCommit,
		"user":         user,
	}

	data, err := json.Marshal(meta)
	if err != nil {
		result.warn("session meta marshal error: %v", err)
		return
	}
	if err := os.WriteFile(metaPath, data, 0o644); err != nil {
		result.warn("session meta write error: %v", err)
	}
}

// detectGitBranch returns the current git branch name.
func detectGitBranch(projectDir string) string {
	cmd := exec.Command("git", "-C", projectDir, "branch", "--show-current")
	out, err := cmd.Output()
	if err != nil {
		return "unknown"
	}
	return strings.TrimSpace(string(out))
}

// detectGitCommit returns the short commit hash.
func detectGitCommit(projectDir string) string {
	cmd := exec.Command("git", "-C", projectDir, "rev-parse", "--short", "HEAD")
	out, err := cmd.Output()
	if err != nil {
		return "unknown"
	}
	return strings.TrimSpace(string(out))
}

// detectStaleTeams scans for team configs with dead tmux panes.
func (init_ *Initializer) detectStaleTeams() []string {
	teamsDir := filepath.Join(init_.HomeDir, ".claude", "teams")
	entries, err := os.ReadDir(teamsDir)
	if err != nil {
		return nil
	}

	var warnings []string
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		cfgPath := filepath.Join(teamsDir, e.Name(), "config.json")
		data, err := os.ReadFile(cfgPath)
		if err != nil {
			continue
		}

		var cfg struct {
			Members []struct {
				TmuxPaneID string `json:"tmuxPaneId"`
			} `json:"members"`
		}
		if err := json.Unmarshal(data, &cfg); err != nil {
			continue
		}
		if len(cfg.Members) == 0 {
			continue
		}

		stale := 0
		for _, m := range cfg.Members {
			if !init_.TmuxChecker.IsPaneAlive(m.TmuxPaneID) {
				stale++
			}
		}
		if stale > 0 {
			warnings = append(warnings,
				fmt.Sprintf("STALE TEAM: '%s' has %d/%d members with dead panes",
					e.Name(), stale, len(cfg.Members)))
		}
	}
	return warnings
}

// detectCompactRecovery emits advisory messages for compact/resume/clear continuations.
func (init_ *Initializer) detectCompactRecovery(projectDir, source string, result *InitResult) {
	if source != "compact" && source != "resume" && source != "clear" {
		return
	}

	hasTeamConfig := false
	hasPathflowFlag := false

	// Check for team configs.
	teamsDir := filepath.Join(init_.HomeDir, ".claude", "teams")
	entries, _ := os.ReadDir(teamsDir)
	for _, e := range entries {
		if e.IsDir() {
			cfgPath := filepath.Join(teamsDir, e.Name(), "config.json")
			if _, err := os.Stat(cfgPath); err == nil {
				hasTeamConfig = true
				break
			}
		}
	}

	// Check for pathflow-active flags.
	sessionDir := filepath.Join(projectDir, ".state", "session")
	sessionEntries, _ := os.ReadDir(sessionDir)
	for _, e := range sessionEntries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "ses-") {
			flagPath := filepath.Join(sessionDir, e.Name(), "pathflow", "is-pathflow-active")
			if _, err := os.Stat(flagPath); err == nil {
				hasPathflowFlag = true
				break
			}
		}
	}

	if hasTeamConfig || hasPathflowFlag {
		result.msg("COMPACT RECOVERY: Session continued after context overflow.")
		result.msg("MANDATORY: Verify teammate liveness BEFORE any respawn.")
		result.msg("   Step 1: SendMessage to each teammate -- ask \"What is your current state?\"")
		result.msg("   Step 2: Wait 30 seconds for responses")
		result.msg("   Step 3: Only respawn confirmed-dead teammates (verify tmux if no response)")
		result.msg("FORBIDDEN: Respawning without verification. Teammates are likely still alive.")
	}
}

// createProjectTempDir creates the project temp directory.
func (init_ *Initializer) createProjectTempDir(projectDir string, result *InitResult) {
	projectName := filepath.Base(projectDir)
	if projectName == "" {
		projectName = "codeflow"
	}
	tmpDir := filepath.Join("/tmp", "claude", projectName)

	// Guard: never rm -rf the project root.
	absProject, _ := filepath.Abs(projectDir)
	absTmp, _ := filepath.Abs(tmpDir)
	if absProject != "" && absTmp != "" && absProject == absTmp {
		result.warn("project temp dir matches project root, skipping cleanup")
		return
	}

	_ = os.RemoveAll(tmpDir)
	if err := os.MkdirAll(tmpDir, 0o755); err != nil {
		result.warn("project temp dir creation error: %v", err)
	}
}

// warn adds a warning message.
func (r *InitResult) warn(format string, args ...any) {
	r.Warnings = append(r.Warnings, fmt.Sprintf(format, args...))
}

// msg adds an informational message.
func (r *InitResult) msg(format string, args ...any) {
	r.Messages = append(r.Messages, fmt.Sprintf(format, args...))
}

// ValidateSessionID checks if a session ID matches the expected format.
func ValidateSessionID(sid string) bool {
	return sessionIDRe.MatchString(sid)
}

// FormatEnvOutput produces the JSON output expected by the hook framework.
func (r *InitResult) FormatEnvOutput() ([]byte, error) {
	output := map[string]any{
		"env": r.EnvVars,
	}
	return json.Marshal(output)
}

