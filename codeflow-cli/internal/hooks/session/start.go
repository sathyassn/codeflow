package session

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime/debug"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/pathflow"
	"github.com/codeflow/codeflow-cli/internal/session"
)

// sessionIDRe validates the CODEFLOW_SESSION_ID format. Accepts both:
//   - Legacy format: ses-{13-digit-timestamp}{12-hex-chars} (25 chars total)
//   - ULID format:   ses-{26-char-lowercase-ULID}          (30 chars total)
var sessionIDRe = regexp.MustCompile(`^ses-(?:[0-9]{13}[a-f0-9]{12}|[0-9a-z]{26})$`)

// PathflowSessionStatusFile is the filename for the structured session status tracker.
const PathflowSessionStatusFile = "pathflow-session-status.json"

// PathflowSessionStatus represents the structured session status tracker.
// It replaces the old is-pathflow-active flag with a richer state model.
type PathflowSessionStatus struct {
	SessionID          string `json:"session_id"`
	TeamName           string `json:"team_name"`
	Status             string `json:"status"`
	WorkType           string `json:"work_type,omitempty"`
	LastCompletedPhase string `json:"last_completed_phase"`
	LastCompletedStage string `json:"last_completed_stage"`
	CreatedAt          string `json:"created_at"`
	UpdatedAt          string `json:"updated_at"`
}

// ReadPathflowSessionStatus reads the session status file from the given pathflow directory.
// Returns nil and no error if the file does not exist.
func ReadPathflowSessionStatus(pathflowDir string) (*PathflowSessionStatus, error) {
	statusPath := filepath.Join(pathflowDir, PathflowSessionStatusFile)
	data, err := os.ReadFile(statusPath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("read session status: %w", err)
	}
	var status PathflowSessionStatus
	if err := json.Unmarshal(data, &status); err != nil {
		return nil, fmt.Errorf("parse session status: %w", err)
	}
	return &status, nil
}

// WritePathflowSessionStatus writes the session status file atomically.
func WritePathflowSessionStatus(pathflowDir string, status *PathflowSessionStatus) error {
	statusPath := filepath.Join(pathflowDir, PathflowSessionStatusFile)
	data, err := json.Marshal(status)
	if err != nil {
		return fmt.Errorf("marshal session status: %w", err)
	}
	tmpPath := statusPath + ".tmp"
	if err := os.WriteFile(tmpPath, data, 0o644); err != nil {
		return fmt.Errorf("write session status tmp: %w", err)
	}
	if err := os.Rename(tmpPath, statusPath); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("rename session status: %w", err)
	}
	return nil
}

// UpdatePathflowSessionStatus reads, applies updates, and writes the session status.
func UpdatePathflowSessionStatus(pathflowDir string, now func() time.Time, updateFn func(*PathflowSessionStatus)) error {
	status, err := ReadPathflowSessionStatus(pathflowDir)
	if err != nil {
		return err
	}
	if status == nil {
		status = &PathflowSessionStatus{}
	}
	updateFn(status)
	status.UpdatedAt = now().Format("2006-01-02T15:04:05.000Z")
	return WritePathflowSessionStatus(pathflowDir, status)
}

// IsPathflowActive checks if a PathFlow session is active by reading the session
// status file. Returns true if the file exists and status is not "pf-complete".
// No backward compatibility fallback — pathflow-session-status.json is the sole authority.
func IsPathflowActive(pathflowDir string) bool {
	status, err := ReadPathflowSessionStatus(pathflowDir)
	if err == nil && status != nil {
		return status.Status != "pf-complete"
	}
	return false
}

// SessionStarter abstracts session ID generation and DB registration for testability.
type SessionStarter interface {
	StartSession(ctx context.Context, claudeID string, projectDir string) (string, error)
}

type dbSessionStarter struct{}

func (dbSessionStarter) StartSession(ctx context.Context, claudeID string, projectDir string) (string, error) {
	dbDir := filepath.Join(projectDir, ".state", "db")
	if err := os.MkdirAll(dbDir, 0o755); err != nil {
		return "", fmt.Errorf("creating database directory: %w", err)
	}
	dbPath := filepath.Join(dbDir, "codeflow.db")
	d, err := db.NewDB(dbPath)
	if err != nil {
		return "", fmt.Errorf("opening database: %w", err)
	}
	defer d.Close()
	if err := d.InitFromSchema(ctx); err != nil {
		return "", fmt.Errorf("initializing database schema: %w", err)
	}
	if _, err := d.Migrate(ctx); err != nil {
		return "", fmt.Errorf("applying database migrations: %w", err)
	}
	ledgerDir := filepath.Join(projectDir, ".state", "ledger")
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	for _, dir := range []string{ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return "", fmt.Errorf("creating directory %s: %w", dir, err)
		}
	}
	return session.Start(ctx, d, claudeID, ledgerDir, runtimeDir)
}

type hookInput struct {
	SessionID string `json:"session_id"`
	Source    string `json:"source"`
}

// InitResult holds the output of a successful session initialization.
type InitResult struct {
	SessionID  string
	IsResume   bool
	IsTeammate bool
	EnvVars    map[string]string
	Warnings   []string
	Messages   []string
}

// TmuxChecker abstracts tmux pane liveness checks for testability.
// Retained ONLY for detectStaleTeams (team config cleanup).
// It is NOT used for session lifecycle decisions.
type TmuxChecker interface {
	IsPaneAlive(paneID string) bool
}

type osTmuxChecker struct{}

func (osTmuxChecker) IsPaneAlive(paneID string) bool {
	if paneID == "" {
		return false
	}
	out, err := exec.Command("tmux", "list-panes", "-a", "-F", "#{pane_id}").Output()
	if err != nil {
		return true
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
	Now            func() time.Time
	TmuxChecker    TmuxChecker
	SessionStarter SessionStarter
	ReadBuildInfo  func() string
	HomeDir        string
}

// NewInitializer creates an Initializer with production defaults.
func NewInitializer() *Initializer {
	home, _ := os.UserHomeDir()
	return &Initializer{
		Now:            func() time.Time { return time.Now().UTC() },
		TmuxChecker:    osTmuxChecker{},
		SessionStarter: dbSessionStarter{},
		ReadBuildInfo:  readBinaryVCSRevision,
		HomeDir:        home,
	}
}

// StartInit performs all session initialization steps.
func (init_ *Initializer) StartInit(stdin io.Reader, projectDir string) (*InitResult, error) {
	if projectDir == "" {
		return nil, fmt.Errorf("session init: empty project directory")
	}
	result := &InitResult{
		EnvVars: make(map[string]string),
	}

	input := parseStdin(stdin)

	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	lockFile, lockErr := session.AcquireSessionLock(runtimeDir)
	if lockErr != nil {
		result.warn("session lock: %v (proceeding without lock)", lockErr)
	}
	releaseLock := func() {
		session.ReleaseSessionLock(lockFile)
		lockFile = nil
	}
	defer releaseLock()

	envFilePath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	existingSID, teamMode, err := init_.handleStaleCleanup(projectDir, envFilePath, input.Source)
	if err != nil {
		result.warn("stale cleanup error: %v", err)
	}
	if teamMode {
		result.IsTeammate = true
		result.SessionID = existingSID
		result.msg("TEAMMATE MODE: You are a teammate joining session %s.", existingSID)
		result.msg("Checkpoint and pathflow flag creation skipped (lead handles those).")
		result.EnvVars["CODEFLOW_SESSION_ID"] = existingSID
		result.EnvVars["CF_PROJECT_ROOT"] = filepath.Base(projectDir)
		return result, nil
	}

	sessionID := existingSID
	if sessionID == "" {
		sessionID = os.Getenv("CODEFLOW_SESSION_ID")
	}
	if sessionID == "" && (input.Source == "startup" || input.Source == "unknown") {
		claudeID := input.SessionID
		if claudeID == "" {
			claudeID = "unknown"
		}
		var startErr error
		sessionID, startErr = init_.SessionStarter.StartSession(context.TODO(), claudeID, projectDir)
		if startErr != nil {
			result.warn("session start error: %v", startErr)
			return nil, fmt.Errorf("session init: generating session ID: %w", startErr)
		}
		if err := session.WriteEnvFile(runtimeDir, sessionID, projectDir); err != nil {
			result.warn("env file write error: %v", err)
		}
	} else if sessionID == "" {
		result.warn("source=%s but no existing session ID found in env file or environment", input.Source)
		envFilePath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
		if data, readErr := os.ReadFile(envFilePath); readErr == nil {
			sessionID = parseEnvFileSessionID(string(data))
		}
		if sessionID == "" {
			return nil, fmt.Errorf("session init: source=%s requires existing session but none found", input.Source)
		}
	}
	result.SessionID = sessionID
	result.EnvVars["CODEFLOW_SESSION_ID"] = sessionID
	result.EnvVars["CF_PROJECT_ROOT"] = filepath.Base(projectDir)

	releaseLock()

	init_.createDirectories(projectDir, sessionID)

	if input.Source == "startup" || input.Source == "unknown" {
		init_.sweepAllStaleSessions(projectDir, sessionID)
	}

	init_.cleanupActiveTask(projectDir)

	isResume := init_.createSessionStatus(projectDir, sessionID, result)
	result.IsResume = isResume

	init_.initCheckpoint(projectDir, sessionID, result)
	init_.writeSessionMetadata(projectDir, sessionID, input, result)

	if input.Source == "startup" || input.Source == "unknown" {
		teamWarnings := init_.detectStaleTeams()
		if len(teamWarnings) > 0 {
			staleLogger := newCleanupLogger(projectDir, init_.Now)
			for _, w := range teamWarnings {
				result.warn("%s", w)
				staleLogger.log("stale_team_detected", map[string]any{
					"warning":      w,
					"action_taken": "cleaned",
				})
			}
		}
	}

	init_.detectCompactRecovery(projectDir, input.Source, result)
	init_.createProjectTempDir(projectDir, result)
	init_.autoRebuildCLI(projectDir, result)

	return result, nil
}

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

type pathflowTeamJSON struct {
	LeadPID  int    `json:"lead_pid"`
	TeamName string `json:"team_name"`
}

// handleStaleCleanup determines if the current caller is a teammate or the lead,
// using multi-signal detection based on CODEFLOW_SESSION_ID env var and
// pathflow-session-status.json (replaces tmux pane liveness checks).
func (init_ *Initializer) handleStaleCleanup(projectDir, envFilePath, source string) (string, bool, error) {
	data, err := os.ReadFile(envFilePath)
	if err != nil {
		return "", false, nil
	}
	existingSID := parseEnvFileSessionID(string(data))
	if existingSID == "" {
		return "", false, nil
	}
	if !sessionIDRe.MatchString(existingSID) {
		if source == "startup" || source == "unknown" {
			_ = os.Remove(envFilePath)
			return "", false, nil
		}
		return existingSID, false, nil
	}

	// STEP 2a: Minimal path for compact/resume/clear — safe for all agents.
	if source == "compact" || source == "resume" || source == "clear" {
		return existingSID, false, nil
	}

	// STEP 3: Teammate detection for startup/unknown sources.
	// Signal 1: CODEFLOW_SESSION_ID env var.
	envSID := os.Getenv("CODEFLOW_SESSION_ID")

	// Case C: envSID != "" and envSID != existingSID → new lead, stale env file.
	if envSID != "" && envSID != existingSID {
		init_.cleanupStaleSession(projectDir, envFilePath, existingSID, "")
		return "", false, nil
	}

	// Case B: envSID="" and existingSID != "" → prior session may be stale.
	// Case D: envSID == existingSID → may be teammate.
	// Both: continue to Signal 2 for confirmation.

	// Signal 2: pathflow-session-status.json confirmation.
	pathflowDir := filepath.Join(projectDir, ".state", "session", existingSID, "pathflow")
	status, readErr := ReadPathflowSessionStatus(pathflowDir)

	if readErr != nil || status == nil {
		// No status file. Check for pathflow-team.json as fallback for no-team-file case.
		return init_.handleNoTeamFile(projectDir, envFilePath, existingSID, source)
	}

	switch status.Status {
	case "pf-complete":
		// Session done. Clean up for new lead.
		init_.cleanupStaleSession(projectDir, envFilePath, existingSID, status.TeamName)
		return "", false, nil

	case "created":
		// Session started but no team yet. Treat as stale on startup.
		if source == "startup" || source == "unknown" {
			init_.cleanupStaleSession(projectDir, envFilePath, existingSID, "")
			return "", false, nil
		}
		return existingSID, false, nil

	case "pf-started", "pf-in-progress":
		// Active session. Check env var match + team config for teammate detection.
		if status.TeamName != "" && envSID == existingSID {
			teamCfgPath := filepath.Join(init_.HomeDir, ".claude", "teams", status.TeamName, "config.json")
			if _, cfgErr := os.Stat(teamCfgPath); cfgErr == nil {
				// Active session confirmed. Caller is a teammate.
				return existingSID, true, nil
			}
		}

		// envSID="" (new lead) or team config missing → stale on startup.
		if source == "startup" || source == "unknown" {
			init_.cleanupStaleSession(projectDir, envFilePath, existingSID, status.TeamName)
			return "", false, nil
		}
		return existingSID, false, nil
	}

	// Unknown status — treat as stale on startup.
	if source == "startup" || source == "unknown" {
		init_.cleanupStaleSession(projectDir, envFilePath, existingSID, status.TeamName)
		return "", false, nil
	}
	return existingSID, false, nil
}

func (init_ *Initializer) handleNoTeamFile(projectDir, envFilePath, oldSID, source string) (string, bool, error) {
	pathflowDir := filepath.Join(projectDir, ".state", "session", oldSID, "pathflow")

	hasStatus := false
	if _, err := os.Stat(filepath.Join(pathflowDir, PathflowSessionStatusFile)); err == nil {
		hasStatus = true
	}

	if !hasStatus {
		_ = os.Remove(envFilePath)
		return "", false, nil
	}

	if source == "startup" || source == "unknown" {
		sessionDir := filepath.Join(projectDir, ".state", "session", oldSID)
		_ = os.RemoveAll(sessionDir)
		sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", oldSID)
		_ = os.RemoveAll(sentinelDir)
		_ = os.Remove(envFilePath)
		return "", false, nil
	}

	return oldSID, false, nil
}

func (init_ *Initializer) cleanupStaleSession(projectDir, envFilePath, sid, teamName string) {
	if teamName != "" {
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "teams", teamName))
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
	}
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "session", sid))
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid))
	_ = os.Remove(envFilePath)
	_ = os.Remove(filepath.Join(projectDir, ".state", "runtime", "active-task.json"))
}

func parseEnvFileSessionID(content string) string {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "export CODEFLOW_SESSION_ID=") {
			val := strings.TrimPrefix(line, "export CODEFLOW_SESSION_ID=")
			val = strings.Trim(val, "'\"")
			return val
		}
	}
	return ""
}

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

// sweepAllStaleSessions uses pathflow-session-status.json to determine which
// old sessions to clean. Replaces the tmux pane-based liveness check.
func (init_ *Initializer) sweepAllStaleSessions(projectDir, currentSID string) {
	logger := newCleanupLogger(projectDir, init_.Now)
	var found, cleaned int
	sessionBase := filepath.Join(projectDir, ".state", "session")
	entries, err := os.ReadDir(sessionBase)
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
		found++

		pathflowDir := filepath.Join(sessionBase, sid, "pathflow")
		status, readErr := ReadPathflowSessionStatus(pathflowDir)

		if readErr != nil || status == nil {
			// Missing status file: clean unconditionally (pre-redesign orphan).
			init_.removeStaleSessionArtifacts(projectDir, sid, "")
			cleaned++
			continue
		}

		switch status.Status {
		case "pf-complete":
			// Session finished — clean.
			init_.removeStaleSessionArtifacts(projectDir, sid, status.TeamName)
			cleaned++

		case "created":
			// Stuck at startup. Check age.
			if init_.statusFileAge(status) > time.Hour {
				init_.removeStaleSessionArtifacts(projectDir, sid, "")
				cleaned++
			}

		case "pf-started", "pf-in-progress":
			if status.TeamName == "" {
				if init_.statusFileAge(status) > time.Hour {
					init_.removeStaleSessionArtifacts(projectDir, sid, "")
					cleaned++
				}
				continue
			}
			teamCfgPath := filepath.Join(init_.HomeDir, ".claude", "teams", status.TeamName, "config.json")
			if _, cfgErr := os.Stat(teamCfgPath); cfgErr != nil {
				// Config missing — team already dissolved.
				init_.removeStaleSessionArtifacts(projectDir, sid, status.TeamName)
				cleaned++
				continue
			}
			// Config exists — check age.
			if init_.statusFileAge(status) > 24*time.Hour {
				init_.removeStaleSessionArtifacts(projectDir, sid, status.TeamName)
				cleaned++
			}

		default:
			// Unknown status — clean.
			init_.removeStaleSessionArtifacts(projectDir, sid, status.TeamName)
			cleaned++
		}
	}

	logger.log("stale_session_sweep", map[string]any{
		"sessions_found":   found,
		"sessions_cleaned": cleaned,
		"sessions_skipped": found - cleaned,
	})

	sentinelBase := filepath.Join(projectDir, ".state", "sentinels", "pathflow")
	sentinelEntries, err := os.ReadDir(sentinelBase)
	if err != nil {
		return
	}
	for _, e := range sentinelEntries {
		if !e.IsDir() || !strings.HasPrefix(e.Name(), "ses-") {
			continue
		}
		sid := e.Name()
		if sid == currentSID {
			continue
		}
		sessionDir := filepath.Join(sessionBase, sid)
		if _, statErr := os.Stat(sessionDir); os.IsNotExist(statErr) {
			_ = os.RemoveAll(filepath.Join(sentinelBase, sid))
		}
	}
}

// statusFileAge returns the age of a session based on its updated_at (preferred)
// or created_at timestamp.
func (init_ *Initializer) statusFileAge(status *PathflowSessionStatus) time.Duration {
	ts := status.UpdatedAt
	if ts == "" {
		ts = status.CreatedAt
	}
	if ts == "" {
		return 0
	}
	t, err := time.Parse("2006-01-02T15:04:05.000Z", ts)
	if err != nil {
		return 0
	}
	return init_.Now().Sub(t)
}

func (init_ *Initializer) removeStaleSessionArtifacts(projectDir, sid, teamName string) {
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "session", sid))
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid))
	if teamName != "" {
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "teams", teamName))
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
	}
}

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
	if task.Status == "completed" || task.Status == "done" {
		_ = os.Remove(taskPath)
		return
	}
	if task.UpdatedAt != "" {
		if t, err := time.Parse(time.RFC3339, task.UpdatedAt); err == nil {
			if init_.Now().Sub(t) > 24*time.Hour {
				_ = os.Remove(taskPath)
			}
		}
	}
}

// pathflowFlag represents the old is-pathflow-active JSON file (deprecated).
type pathflowFlag struct {
	SessionID     string `json:"session_id"`
	CreatedAt     string `json:"created_at"`
	TrackingLevel string `json:"tracking_level"`
}

// createSessionStatus creates the pathflow-session-status.json file.
// Returns true if the file already existed (resume scenario).
func (init_ *Initializer) createSessionStatus(projectDir, sessionID string, result *InitResult) bool {
	pathflowDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	statusPath := filepath.Join(pathflowDir, PathflowSessionStatusFile)

	if _, err := os.Stat(statusPath); err == nil {
		return true
	}

	if err := os.MkdirAll(pathflowDir, 0o755); err != nil {
		result.warn("pathflow dir creation error: %v", err)
		return false
	}

	now := init_.Now().Format("2006-01-02T15:04:05.000Z")
	status := &PathflowSessionStatus{
		SessionID: sessionID,
		Status:    "created",
		CreatedAt: now,
		UpdatedAt: now,
	}
	if err := WritePathflowSessionStatus(pathflowDir, status); err != nil {
		result.warn("session status write error: %v", err)
		return false
	}
	return false
}

func (init_ *Initializer) initCheckpoint(projectDir, sessionID string, result *InitResult) {
	checkpointPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-phase-tasks.json")
	configPath := filepath.Join(projectDir, ".codeflow", "config", "pathflow", "pathflow-config.json")
	cp := &pathflow.Checkpoint{Now: init_.Now}
	if err := cp.InitAllPhases(checkpointPath, configPath); err != nil {
		result.warn("checkpoint init error: %v", err)
	}
}

func (init_ *Initializer) writeSessionMetadata(projectDir, sessionID string, input hookInput, result *InitResult) {
	metaPath := filepath.Join(projectDir, ".state", "logs", "sessions", "session-"+sessionID+".meta")
	gitBranch := detectGitBranch(projectDir)
	gitCommit := detectGitCommit(projectDir)
	user := os.Getenv("USER")
	if user == "" {
		user = "unknown"
	}
	meta := map[string]any{
		"session_id":     sessionID,
		"claude_uuid":   input.SessionID,
		"source":        input.Source,
		"started_at":    init_.Now().Format("2006-01-02T15:04:05.000Z"),
		"started_epoch": init_.Now().Unix(),
		"repo_root":     projectDir,
		"git_branch":    gitBranch,
		"git_commit":    gitCommit,
		"user":          user,
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

func detectGitBranch(projectDir string) string {
	cmd := exec.Command("git", "-C", projectDir, "branch", "--show-current")
	out, err := cmd.Output()
	if err != nil {
		return "unknown"
	}
	return strings.TrimSpace(string(out))
}

func detectGitCommit(projectDir string) string {
	cmd := exec.Command("git", "-C", projectDir, "rev-parse", "--short", "HEAD")
	out, err := cmd.Output()
	if err != nil {
		return "unknown"
	}
	return strings.TrimSpace(string(out))
}

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
		teamName := e.Name()
		cfgPath := filepath.Join(teamsDir, teamName, "config.json")
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
		alive := 0
		for _, m := range cfg.Members {
			if init_.TmuxChecker.IsPaneAlive(m.TmuxPaneID) {
				alive++
			}
		}
		if alive == 0 {
			_ = os.RemoveAll(filepath.Join(teamsDir, teamName))
			_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
			warnings = append(warnings,
				fmt.Sprintf("STALE TEAM CLEANED: '%s' (%d members, all panes dead)",
					teamName, len(cfg.Members)))
		}
	}
	return warnings
}

func (init_ *Initializer) detectCompactRecovery(projectDir, source string, result *InitResult) {
	if source != "compact" && source != "resume" && source != "clear" {
		return
	}

	hasTeamConfig := false
	hasActiveSession := false

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

	sessionDir := filepath.Join(projectDir, ".state", "session")
	sessionEntries, _ := os.ReadDir(sessionDir)
	for _, e := range sessionEntries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "ses-") {
			pfDir := filepath.Join(sessionDir, e.Name(), "pathflow")
			if IsPathflowActive(pfDir) {
				hasActiveSession = true
				break
			}
		}
	}

	if hasTeamConfig || hasActiveSession {
		result.msg("COMPACT RECOVERY: Session continued after context overflow.")
		result.msg("MANDATORY: Verify teammate liveness BEFORE any respawn.")
		result.msg("   Step 1: SendMessage to each teammate -- ask \"What is your current state?\"")
		result.msg("   Step 2: Wait 30 seconds for responses")
		result.msg("   Step 3: Only respawn confirmed-dead teammates (verify tmux if no response)")
		result.msg("FORBIDDEN: Respawning without verification. Teammates are likely still alive.")
	}
}

func (init_ *Initializer) createProjectTempDir(projectDir string, result *InitResult) {
	projectName := filepath.Base(projectDir)
	if projectName == "" {
		projectName = "codeflow"
	}
	tmpDir := filepath.Join("/tmp", "claude", projectName)
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

func readBinaryVCSRevision() string {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return ""
	}
	for _, s := range info.Settings {
		if s.Key == "vcs.revision" && s.Value != "" {
			if len(s.Value) > 7 {
				return s.Value[:7]
			}
			return s.Value
		}
	}
	return ""
}

func (init_ *Initializer) autoRebuildCLI(projectDir string, result *InitResult) {
	modFile := filepath.Join(projectDir, "codeflow-cli", "go.mod")
	if _, err := os.Stat(modFile); os.IsNotExist(err) {
		return
	}
	gitHead := detectGitCommit(projectDir)
	if gitHead == "" || gitHead == "unknown" {
		return
	}
	binaryRev := init_.ReadBuildInfo()
	if binaryRev == "" {
		return
	}
	if binaryRev == gitHead {
		return
	}
	result.msg("[auto-rebuild] Binary version %s differs from HEAD %s, rebuilding...", binaryRev, gitHead)
	cliDir := filepath.Join(projectDir, "codeflow-cli")
	cmd := exec.Command("go", "install", "./cmd/codeflow/")
	cmd.Dir = cliDir
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	output, err := cmd.CombinedOutput()
	if err != nil {
		result.warn("[auto-rebuild] rebuild failed: %v (output: %s)", err, strings.TrimSpace(string(output)))
		return
	}
	result.msg("[auto-rebuild] rebuild complete.")
}

func (r *InitResult) warn(format string, args ...any) {
	r.Warnings = append(r.Warnings, fmt.Sprintf(format, args...))
}

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
