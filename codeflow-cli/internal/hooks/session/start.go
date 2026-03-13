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
	"strconv"
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

// SessionStarter abstracts session ID generation and DB registration for testability.
type SessionStarter interface {
	// StartSession generates a ULID-based session ID and records the session in
	// the database and ledger. Returns the generated session ID.
	// Note: codeflow-env.sh is NOT written by StartSession. The caller
	// (StartInit) handles it via writeEnvFile().
	StartSession(ctx context.Context, claudeID string, projectDir string) (string, error)
}

// dbSessionStarter implements SessionStarter using the real session.Start function.
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

	// Apply any pending migrations (e.g., adding format_id column to tasks).
	// InitFromSchema uses CREATE TABLE IF NOT EXISTS which silently skips
	// existing tables, so migrations are needed for schema evolution.
	if _, err := d.Migrate(ctx); err != nil {
		return "", fmt.Errorf("applying database migrations: %w", err)
	}

	ledgerDir := filepath.Join(projectDir, ".state", "ledger")
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")

	// Ensure ledger and runtime directories exist before session.Start writes to them.
	for _, dir := range []string{ledgerDir, runtimeDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return "", fmt.Errorf("creating directory %s: %w", dir, err)
		}
	}

	return session.Start(ctx, d, claudeID, ledgerDir, runtimeDir)
}

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
		// If tmux is unreachable, assume pane is alive (safe default).
		// Treating failure as "all panes dead" would destroy live sessions.
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
	// Now returns the current time. Override in tests for deterministic output.
	Now func() time.Time

	// ProcessChecker checks if a PID is alive.
	ProcessChecker ProcessChecker

	// TmuxChecker checks if a tmux pane is alive.
	TmuxChecker TmuxChecker

	// SessionStarter generates session IDs and registers sessions in the database.
	SessionStarter SessionStarter

	// ReadBuildInfo returns the short VCS revision of the running binary.
	// Override in tests for deterministic output.
	ReadBuildInfo func() string

	// PPID is the parent process ID for pathflow-team.json updates.
	PPID int

	// HomeDir overrides os.UserHomeDir for testing.
	HomeDir string
}

// getClaudePID returns the persistent Claude Code process PID by walking up
// the process tree one level. The hook execution chain is:
//
//	claude (persistent) → /bin/zsh (ephemeral) → codeflow binary
//
// os.Getppid() returns the ephemeral shell PID. This function gets its parent
// (the claude process) via ps, falling back to os.Getppid() on error.
func getClaudePID() int {
	ppid := os.Getppid()
	out, err := exec.Command("ps", "-o", "ppid=", "-p", strconv.Itoa(ppid)).Output()
	if err != nil {
		return ppid
	}
	claudePID, err := strconv.Atoi(strings.TrimSpace(string(out)))
	if err != nil {
		return ppid
	}
	if claudePID <= 1 {
		return ppid // Don't return init/launchd PID
	}
	return claudePID
}

// NewInitializer creates an Initializer with production defaults.
func NewInitializer() *Initializer {
	home, _ := os.UserHomeDir()
	return &Initializer{
		Now:            func() time.Time { return time.Now().UTC() },
		ProcessChecker: osProcessChecker{},
		TmuxChecker:    osTmuxChecker{},
		SessionStarter: dbSessionStarter{},
		ReadBuildInfo:  readBinaryVCSRevision,
		PPID:           getClaudePID(),
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

	// --- Section 1a: Acquire session lock ---
	// Serialize session creation across concurrent agents (teammates + lead).
	// The lock covers: read env file -> teammate detection -> SID generation -> write env file.
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	lockFile, lockErr := session.AcquireSessionLock(runtimeDir)
	if lockErr != nil {
		result.warn("session lock: %v (proceeding without lock)", lockErr)
	}
	releaseLock := func() {
		session.ReleaseSessionLock(lockFile)
		lockFile = nil
	}
	defer releaseLock() // safety net for early returns

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
	// Source guard: only generate a new session ID on "startup".
	// For compact/resume/clear, reuse the existing session from codeflow-env.sh.
	sessionID := existingSID
	if sessionID == "" {
		sessionID = os.Getenv("CODEFLOW_SESSION_ID")
	}
	if sessionID == "" && (input.Source == "startup" || input.Source == "unknown") {
		// Generate session ID via session.Start (DB-backed, ULID format).
		// Use the Claude agent UUID as the claudeID; fall back to "unknown" if empty.
		claudeID := input.SessionID
		if claudeID == "" {
			claudeID = "unknown"
		}
		var startErr error
		sessionID, startErr = init_.SessionStarter.StartSession(
			context.TODO(), claudeID, projectDir,
		)
		if startErr != nil {
			result.warn("session start error: %v", startErr)
			return nil, fmt.Errorf("session init: generating session ID: %w", startErr)
		}
		// Write env file atomically via the session package (single authoritative writer).
		if err := session.WriteEnvFile(runtimeDir, sessionID, projectDir); err != nil {
			result.warn("env file write error: %v", err)
		}
	} else if sessionID == "" {
		// Non-startup source with no existing session ID -- log warning but don't crash.
		result.warn("source=%s but no existing session ID found in env file or environment", input.Source)
		// Read from env file one more time as a fallback.
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

	// Release session lock — env file written, teammates can now detect this session.
	releaseLock()

	// --- Section 3: Directory creation ---
	init_.createDirectories(projectDir, sessionID)

	// --- Section 4+5: Sweep all stale sessions (replaces detectStaleSessions + sweepOrphanSentinels) ---
	// Only sweep on startup (fresh session) to avoid costly scans on compact/resume.
	if input.Source == "startup" || input.Source == "unknown" {
		init_.sweepAllStaleSessions(projectDir, sessionID)
	}

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
	// Only detect stale teams on startup. On compact/resume/clear the lead's
	// context overflowed but teammates are likely still alive — destroying their
	// team config would kill an active session.
	if input.Source == "startup" || input.Source == "unknown" {
		teamWarnings := init_.detectStaleTeams()
		for _, w := range teamWarnings {
			result.warn("%s", w)
		}
	}

	// --- Section 10: Compact recovery detection ---
	init_.detectCompactRecovery(projectDir, input.Source, result)

	// --- Section 11: Project temp directory ---
	init_.createProjectTempDir(projectDir, result)

	// --- Section 12: Auto-rebuild CLI binary ---
	// Catches stale binaries after git pull/merge (post-commit hook only covers local commits).
	init_.autoRebuildCLI(projectDir, result)

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

	// Check session liveness via tmux pane status from team config.
	if teamInfo.TeamName != "" && init_.hasLiveTeamPanes(teamInfo.TeamName) {
		// Team has live tmux panes -- teammate mode.
		return oldSID, true, nil
	}

	// No live panes (or no team config).
	if source == "startup" || source == "unknown" {
		// Fresh startup with dead session -- full cleanup.
		init_.cleanupStaleSession(projectDir, envFilePath, oldSID, teamInfo.TeamName)
		return "", false, nil
	}

	// Resume with dead session -- update lead_pid (user relaunched claude).
	// lead_pid is informational only but kept for backward compatibility.
	if source == "resume" {
		init_.updateLeadPID(teamFilePath, teamData)
		return oldSID, false, nil
	}

	// Compact/clear with no live panes -- don't update PID.
	// On compact/clear, the claude process does NOT restart. If no panes
	// are alive and source is compact/clear, this caller is a surviving tmux
	// teammate whose lead died. Updating the PID would incorrectly claim
	// leadership.
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

	// Remove env file and active task.
	_ = os.Remove(envFilePath)
	_ = os.Remove(filepath.Join(projectDir, ".state", "runtime", "active-task.json"))
}

// updateLeadPID updates the lead_pid in pathflow-team.json for compact recovery.
// The PID is the persistent Claude Code process PID (grandparent of the codeflow
// binary), captured via getClaudePID(). Used for session liveness checks alongside
// tmux pane status from ~/.claude/teams/{team_name}/config.json.
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

// hasLiveTeamPanes checks whether any tmux pane in the team config is alive.
// Returns false if the config is missing, unreadable, or has no live panes.
func (init_ *Initializer) hasLiveTeamPanes(teamName string) bool {
	cfgPath := filepath.Join(init_.HomeDir, ".claude", "teams", teamName, "config.json")
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
		if init_.TmuxChecker.IsPaneAlive(m.TmuxPaneID) {
			return true
		}
	}
	return false
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


// writeEnvFile is removed. Use session.WriteEnvFile() instead (single authoritative writer).

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

// sweepAllStaleSessions scans ALL ses-* dirs under .state/session/ (excluding
// currentSID), checks if the team has live tmux panes via team config, and
// removes stale sessions along with their sentinel dirs and team artifacts.
// Called on source=startup only, after generating a new SID.
func (init_ *Initializer) sweepAllStaleSessions(projectDir, currentSID string) {
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

		teamFilePath := filepath.Join(sessionBase, sid, "pathflow", "pathflow-team.json")
		teamData, readErr := os.ReadFile(teamFilePath)
		if readErr != nil {
			// No team file -- stale session, clean it up.
			init_.removeStaleSessionArtifacts(projectDir, sid, "")
			continue
		}

		var teamInfo pathflowTeamJSON
		if err := json.Unmarshal(teamData, &teamInfo); err != nil {
			// Invalid team file -- stale.
			init_.removeStaleSessionArtifacts(projectDir, sid, "")
			continue
		}

		// Check session liveness via tmux pane status from team config.
		if teamInfo.TeamName != "" && init_.hasLiveTeamPanes(teamInfo.TeamName) {
			// Team has live panes -- skip.
			continue
		}

		// No live panes or no team name -- clean up.
		init_.removeStaleSessionArtifacts(projectDir, sid, teamInfo.TeamName)
	}

	// Also sweep orphan sentinel dirs that have no matching session dir.
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

// removeStaleSessionArtifacts removes session dir, sentinel dir, and team
// config/tasks for a stale session.
func (init_ *Initializer) removeStaleSessionArtifacts(projectDir, sid, teamName string) {
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "session", sid))
	_ = os.RemoveAll(filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid))
	if teamName != "" {
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "teams", teamName))
		_ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
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

// detectStaleTeams scans for team configs with dead tmux panes and cleans them up.
// Returns warnings for each team that was cleaned.
//
// SAFETY: If tmux is unavailable (not installed, server not running), this
// function skips detection entirely. Treating tmux failure as "all panes dead"
// would destroy live sessions that use in-process agents (no tmux panes).
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

		// Count alive members.
		alive := 0
		for _, m := range cfg.Members {
			if init_.TmuxChecker.IsPaneAlive(m.TmuxPaneID) {
				alive++
			}
		}

		// If ALL members are dead, remove the team directory and task list.
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

// readBinaryVCSRevision extracts the short VCS revision from the running binary's
// embedded build info. Returns "" if unavailable.
// Override via Initializer.ReadBuildInfo for testing.
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

// autoRebuildCLI rebuilds the codeflow binary when the installed version's VCS
// revision differs from the current git HEAD. This catches stale binaries after
// git pull/merge — the post-commit git hook only fires for local commits.
// Non-fatal: warnings only, never blocks session start.
func (init_ *Initializer) autoRebuildCLI(projectDir string, result *InitResult) {
	// Only rebuild if this repo contains the CodeFlow CLI source.
	modFile := filepath.Join(projectDir, "codeflow-cli", "go.mod")
	if _, err := os.Stat(modFile); os.IsNotExist(err) {
		return
	}

	// Get current git HEAD short hash.
	gitHead := detectGitCommit(projectDir)
	if gitHead == "" || gitHead == "unknown" {
		return
	}

	// Get the running binary's embedded VCS revision.
	binaryRev := init_.ReadBuildInfo()
	if binaryRev == "" {
		// Binary was built without VCS info (e.g., "go install" without a git checkout).
		// Cannot compare — skip rebuild.
		return
	}

	// Compare: if they match, the binary is current.
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

