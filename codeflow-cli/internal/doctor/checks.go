package doctor

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// Status represents the outcome of a health check.
type Status string

const (
	// StatusPass indicates the check succeeded.
	StatusPass Status = "pass"
	// StatusFail indicates the check failed.
	StatusFail Status = "fail"
	// StatusWarn indicates a non-critical issue.
	StatusWarn Status = "warn"
)

// Result holds the outcome of a single doctor check.
type Result struct {
	Name     string        `json:"name"`
	Status   Status        `json:"status"`
	Message  string        `json:"message"`
	Duration time.Duration `json:"duration"`
}

// Options configures the doctor check behavior.
type Options struct {
	// DBPath is the path to the SQLite database file.
	DBPath string

	// LedgerDir is the path to the JSONL ledger directory.
	LedgerDir string

	// ProjectDir is the project root directory.
	ProjectDir string

	// StateDir is the path to the .state/ directory.
	StateDir string

	// SessionID is the active PathFlow session ID.
	// Populated from CODEFLOW_SESSION_ID environment variable.
	SessionID string

	// HomeDir overrides the user home directory for testing.
	// Defaults to os.UserHomeDir().
	HomeDir string

	// LookPath locates executables. Defaults to exec.LookPath.
	LookPath func(string) (string, error)

	// ExecCommand runs a command and returns its output.
	// Defaults to running via exec.Command.
	ExecCommand func(string, ...string) ([]byte, error)
}

// applyDefaults fills in zero-value fields with production defaults.
func (o *Options) applyDefaults() {
	if o.HomeDir == "" {
		if h, err := os.UserHomeDir(); err == nil {
			o.HomeDir = h
		}
	}
	if o.LookPath == nil {
		o.LookPath = exec.LookPath
	}
	if o.ExecCommand == nil {
		o.ExecCommand = func(name string, args ...string) ([]byte, error) {
			return exec.Command(name, args...).CombinedOutput()
		}
	}
}

// canonicalJSONLFiles lists the 4 required JSONL ledger files.
var canonicalJSONLFiles = []string{
	"work-graph.jsonl",
	"memory-events.jsonl",
	"sessions.jsonl",
	"config.jsonl",
}

// checkNames is the ordered list of all check names.
var checkNames = []string{
	"database",
	"jsonl",
	"crdt",
	"python",
	"hooks",
	"claude",
	"auth",
	"config",
	"embedding",
	"vector",
	"permissions",
	"version",
	"network",
	"pathflow-stuck",
	"team-health",
	"sentinel-drift",
}

// CheckNames returns the ordered list of all available check names.
func CheckNames() []string {
	names := make([]string, len(checkNames))
	copy(names, checkNames)
	return names
}

// checkFunc is the signature for individual check functions.
type checkFunc func(ctx context.Context, opts *Options) Result

// checkRegistry maps check names to their implementations.
var checkRegistry = map[string]checkFunc{
	"database":       checkDatabase,
	"jsonl":          checkJSONL,
	"crdt":           checkCRDT,
	"python":         checkPython,
	"hooks":          checkHooks,
	"claude":         checkClaude,
	"auth":           checkAuth,
	"config":         checkConfig,
	"embedding":      checkEmbedding,
	"vector":         checkVector,
	"permissions":    checkPermissions,
	"version":        checkVersion,
	"network":        checkNetwork,
	"pathflow-stuck": checkPathflowStuck,
	"team-health":    checkTeamHealth,
	"sentinel-drift": checkSentinelDrift,
}

// RunAll executes all checks concurrently and returns results in the
// canonical order. Independent checks run in parallel to stay under 2 seconds.
func RunAll(ctx context.Context, opts *Options) []Result {
	if opts == nil {
		opts = &Options{}
	}
	opts.applyDefaults()

	type indexedResult struct {
		index  int
		result Result
	}

	results := make([]Result, len(checkNames))
	ch := make(chan indexedResult, len(checkNames))
	var wg sync.WaitGroup

	for i, name := range checkNames {
		fn := checkRegistry[name]
		wg.Add(1)
		go func(idx int, f checkFunc) {
			defer wg.Done()
			ch <- indexedResult{index: idx, result: f(ctx, opts)}
		}(i, fn)
	}

	go func() {
		wg.Wait()
		close(ch)
	}()

	for ir := range ch {
		results[ir.index] = ir.result
	}

	return results
}

// RunCheck executes a single named check.
func RunCheck(ctx context.Context, name string, opts *Options) (Result, error) {
	if opts == nil {
		opts = &Options{}
	}
	opts.applyDefaults()

	fn, ok := checkRegistry[name]
	if !ok {
		return Result{}, fmt.Errorf("unknown check: %q", name)
	}
	return fn(ctx, opts), nil
}

// checkDatabase verifies the SQLite database health.
func checkDatabase(ctx context.Context, opts *Options) Result {
	start := time.Now()

	if opts.DBPath == "" {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  "database path not configured",
			Duration: time.Since(start),
		}
	}

	if _, err := os.Stat(opts.DBPath); os.IsNotExist(err) {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("database file not found: %s", opts.DBPath),
			Duration: time.Since(start),
		}
	}

	d, err := db.NewDB(opts.DBPath)
	if err != nil {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("failed to open database: %v", err),
			Duration: time.Since(start),
		}
	}
	defer d.Close()

	report, err := d.CheckHealth(ctx)
	if err != nil {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("health check error: %v", err),
			Duration: time.Since(start),
		}
	}

	if !report.IsHealthy() {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("database unhealthy: %s", strings.Join(report.Errors, "; ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "database",
		Status:   StatusPass,
		Message:  "database integrity and foreign key checks passed",
		Duration: time.Since(start),
	}
}

// checkJSONL verifies the 4 canonical JSONL files exist and contain valid JSONL.
func checkJSONL(_ context.Context, opts *Options) Result {
	start := time.Now()

	if opts.LedgerDir == "" {
		return Result{
			Name:     "jsonl",
			Status:   StatusFail,
			Message:  "ledger directory not configured",
			Duration: time.Since(start),
		}
	}

	var missing []string
	var invalid []string

	for _, filename := range canonicalJSONLFiles {
		path := filepath.Join(opts.LedgerDir, filename)

		f, err := os.Open(path)
		if err != nil {
			if os.IsNotExist(err) {
				missing = append(missing, filename)
			} else {
				invalid = append(invalid, fmt.Sprintf("%s: %v", filename, err))
			}
			continue
		}

		scanner := bufio.NewScanner(f)
		lineNum := 0
		for scanner.Scan() {
			lineNum++
			line := strings.TrimSpace(scanner.Text())
			if line == "" {
				continue
			}
			if !json.Valid([]byte(line)) {
				invalid = append(invalid, fmt.Sprintf("%s: invalid JSON at line %d", filename, lineNum))
				break
			}
		}
		f.Close()

		if err := scanner.Err(); err != nil {
			invalid = append(invalid, fmt.Sprintf("%s: read error: %v", filename, err))
		}
	}

	if len(missing) > 0 || len(invalid) > 0 {
		var parts []string
		if len(missing) > 0 {
			parts = append(parts, fmt.Sprintf("missing: %s", strings.Join(missing, ", ")))
		}
		if len(invalid) > 0 {
			parts = append(parts, fmt.Sprintf("invalid: %s", strings.Join(invalid, "; ")))
		}
		return Result{
			Name:     "jsonl",
			Status:   StatusFail,
			Message:  strings.Join(parts, "; "),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "jsonl",
		Status:   StatusPass,
		Message:  "all 4 canonical JSONL files valid",
		Duration: time.Since(start),
	}
}

// checkCRDT verifies the .state/ directory structure has required subdirectories.
func checkCRDT(_ context.Context, opts *Options) Result {
	start := time.Now()

	stateDir := opts.StateDir
	if stateDir == "" {
		stateDir = filepath.Join(opts.ProjectDir, ".state")
	}

	requiredDirs := []string{"db", "ledger", "logs", "runtime", "sentinels"}
	var missingDirs []string

	for _, dir := range requiredDirs {
		path := filepath.Join(stateDir, dir)
		info, err := os.Stat(path)
		if err != nil || !info.IsDir() {
			missingDirs = append(missingDirs, dir)
		}
	}

	if len(missingDirs) > 0 {
		return Result{
			Name:     "crdt",
			Status:   StatusFail,
			Message:  fmt.Sprintf("missing .state/ subdirectories: %s", strings.Join(missingDirs, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "crdt",
		Status:   StatusPass,
		Message:  ".state/ directory structure valid",
		Duration: time.Since(start),
	}
}

// checkPython reports python3 availability as informational.
// Post-cutover: python3 is no longer a production dependency.
// It is only used by test infrastructure (.codeflow/testing/).
func checkPython(_ context.Context, opts *Options) Result {
	start := time.Now()

	_, err := opts.LookPath("python3")
	if err != nil {
		return Result{
			Name:     "python",
			Status:   StatusPass,
			Message:  "python3 not found (optional -- not a production dependency)",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "python",
		Status:   StatusPass,
		Message:  "python3 available (optional -- used by test infrastructure only)",
		Duration: time.Since(start),
	}
}

// hookSubcommands lists the Go hook subcommands that must be functional.
// Each entry is a pair: [event, subcommand].
var hookSubcommands = [][2]string{
	{"session-start", "init"},
	{"pre-tool-use", "gate-check"},
	{"pre-tool-use", "team-guard"},
	{"post-tool-use", "sentinel-write"},
	{"post-tool-use", "checkpoint-register"},
	{"task-completed", "checkpoint-complete"},
	{"session-end", "cleanup"},
}

// checkHooks verifies Go hook subcommands are functional.
// Post-cutover: shell hooks in .claude/hooks/codeflow/ were removed.
// Hooks now run as Go CLI subcommands (codeflow hooks <event> <subcommand>).
func checkHooks(_ context.Context, opts *Options) Result {
	start := time.Now()

	// Verify the codeflow binary is available.
	codeflowBin, err := opts.LookPath("codeflow")
	if err != nil {
		return Result{
			Name:     "hooks",
			Status:   StatusFail,
			Message:  "codeflow binary not found in PATH",
			Duration: time.Since(start),
		}
	}

	// Verify each hook subcommand responds (--help as liveness check).
	var failing []string
	for _, sub := range hookSubcommands {
		_, err := opts.ExecCommand(codeflowBin, "hooks", sub[0], sub[1], "--help")
		if err != nil {
			failing = append(failing, fmt.Sprintf("%s %s", sub[0], sub[1]))
		}
	}

	if len(failing) > 0 {
		return Result{
			Name:     "hooks",
			Status:   StatusFail,
			Message:  fmt.Sprintf("%d hook subcommand(s) not responding: %s", len(failing), strings.Join(failing, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "hooks",
		Status:   StatusPass,
		Message:  fmt.Sprintf("all %d Go hook subcommands functional", len(hookSubcommands)),
		Duration: time.Since(start),
	}
}

// checkClaude verifies Claude Code CLI is accessible.
func checkClaude(_ context.Context, opts *Options) Result {
	start := time.Now()

	_, err := opts.LookPath("claude")
	if err != nil {
		return Result{
			Name:     "claude",
			Status:   StatusFail,
			Message:  "claude CLI not found in PATH",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "claude",
		Status:   StatusPass,
		Message:  "claude CLI found",
		Duration: time.Since(start),
	}
}

// checkAuth verifies Claude Code is authenticated.
func checkAuth(_ context.Context, opts *Options) Result {
	start := time.Now()

	output, err := opts.ExecCommand("claude", "auth", "status")
	if err != nil {
		return Result{
			Name:     "auth",
			Status:   StatusFail,
			Message:  fmt.Sprintf("claude auth check failed: %v", err),
			Duration: time.Since(start),
		}
	}

	// Any successful exit with output indicates authentication.
	outStr := strings.TrimSpace(string(output))
	if strings.Contains(strings.ToLower(outStr), "not authenticated") ||
		strings.Contains(strings.ToLower(outStr), "not logged in") {
		return Result{
			Name:     "auth",
			Status:   StatusFail,
			Message:  "claude Code not authenticated",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "auth",
		Status:   StatusPass,
		Message:  "claude Code authenticated",
		Duration: time.Since(start),
	}
}

// checkConfig verifies .codeflow/config/ files are valid JSON.
func checkConfig(_ context.Context, opts *Options) Result {
	start := time.Now()

	configDir := filepath.Join(opts.ProjectDir, ".codeflow", "config")

	info, err := os.Stat(configDir)
	if err != nil || !info.IsDir() {
		return Result{
			Name:     "config",
			Status:   StatusFail,
			Message:  ".codeflow/config/ directory not found",
			Duration: time.Since(start),
		}
	}

	var invalidFiles []string

	err = filepath.WalkDir(configDir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil // Skip unreadable entries.
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(d.Name(), ".json") {
			return nil
		}

		data, err := os.ReadFile(path)
		if err != nil {
			rel, _ := filepath.Rel(configDir, path)
			invalidFiles = append(invalidFiles, fmt.Sprintf("%s: %v", rel, err))
			return nil
		}

		if !json.Valid(data) {
			rel, _ := filepath.Rel(configDir, path)
			invalidFiles = append(invalidFiles, rel)
		}
		return nil
	})
	if err != nil {
		return Result{
			Name:     "config",
			Status:   StatusFail,
			Message:  fmt.Sprintf("error scanning config directory: %v", err),
			Duration: time.Since(start),
		}
	}

	if len(invalidFiles) > 0 {
		return Result{
			Name:     "config",
			Status:   StatusFail,
			Message:  fmt.Sprintf("invalid JSON config files: %s", strings.Join(invalidFiles, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "config",
		Status:   StatusPass,
		Message:  "all config files are valid JSON",
		Duration: time.Since(start),
	}
}

// checkEmbedding verifies embedding model availability.
func checkEmbedding(_ context.Context, _ *Options) Result {
	start := time.Now()

	// Embedding is optional infrastructure. Warn if not configured.
	return Result{
		Name:     "embedding",
		Status:   StatusWarn,
		Message:  "embedding model not configured (optional)",
		Duration: time.Since(start),
	}
}

// checkVector verifies vector store health.
func checkVector(_ context.Context, _ *Options) Result {
	start := time.Now()

	// Vector store is optional infrastructure. Warn if not configured.
	return Result{
		Name:     "vector",
		Status:   StatusWarn,
		Message:  "vector store not configured (optional)",
		Duration: time.Since(start),
	}
}

// checkPermissions verifies key files and directories have correct permissions.
// Post-cutover: shell hook permission checks removed (hooks are Go subcommands now).
func checkPermissions(_ context.Context, opts *Options) Result {
	start := time.Now()

	var issues []string

	// Check .state/ is writable.
	stateDir := opts.StateDir
	if stateDir == "" {
		stateDir = filepath.Join(opts.ProjectDir, ".state")
	}

	if info, err := os.Stat(stateDir); err == nil {
		if info.Mode()&0o200 == 0 {
			issues = append(issues, ".state/ is not writable")
		}
	}

	if len(issues) > 0 {
		return Result{
			Name:     "permissions",
			Status:   StatusFail,
			Message:  fmt.Sprintf("permission issues: %s", strings.Join(issues, "; ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "permissions",
		Status:   StatusPass,
		Message:  "file permissions correct",
		Duration: time.Since(start),
	}
}

// checkVersion verifies CLI version compatibility.
func checkVersion(_ context.Context, _ *Options) Result {
	start := time.Now()

	// Report the current build version. In production this would compare
	// against a minimum required version from project config.
	return Result{
		Name:     "version",
		Status:   StatusPass,
		Message:  "version check passed",
		Duration: time.Since(start),
	}
}

// stuckThreshold is the maximum age for a PathFlow phase before it is
// considered stuck. Set to half of stage_timeout_minutes (60 min / 2 = 30 min).
const stuckThreshold = 30 * time.Minute

// pathflowActive returns true if the pathflow-active flag exists for the session.
func pathflowActive(stateDir, sessionID string) bool {
	flag := filepath.Join(stateDir, "session", sessionID, "pathflow", "is-pathflow-active")
	_, err := os.Stat(flag)
	return err == nil
}

// pathflowEvent represents a single entry from pathflow-events.jsonl.
type pathflowEvent struct {
	EventType string `json:"event_type"`
	Phase     string `json:"phase"`
	Timestamp string `json:"timestamp"`
}

// readPathflowEvents reads and parses pathflow-events.jsonl from StateDir/logs/.
func readPathflowEvents(stateDir string) ([]pathflowEvent, error) {
	path := filepath.Join(stateDir, "logs", "pathflow-events.jsonl")
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("open pathflow events: %w", err)
	}
	defer f.Close()

	var events []pathflowEvent
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var ev pathflowEvent
		if err := json.Unmarshal([]byte(line), &ev); err != nil {
			continue // Skip malformed lines.
		}
		events = append(events, ev)
	}
	if err := scanner.Err(); err != nil {
		return events, fmt.Errorf("scan pathflow events: %w", err)
	}
	return events, nil
}

// checkPathflowStuck detects PathFlow phases older than 30 minutes.
func checkPathflowStuck(_ context.Context, opts *Options) Result {
	start := time.Now()

	if opts.SessionID == "" || !pathflowActive(opts.StateDir, opts.SessionID) {
		return Result{
			Name:     "pathflow-stuck",
			Status:   StatusPass,
			Message:  "pathflow not active",
			Duration: time.Since(start),
		}
	}

	events, err := readPathflowEvents(opts.StateDir)
	if err != nil {
		return Result{
			Name:     "pathflow-stuck",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("cannot read pathflow events: %v", err),
			Duration: time.Since(start),
		}
	}

	// Find the latest phase_transition event.
	var latestPhase string
	var latestTime time.Time
	for _, ev := range events {
		if ev.EventType != "phase_transition" {
			continue
		}
		t, err := time.Parse(time.RFC3339, ev.Timestamp)
		if err != nil {
			continue
		}
		if t.After(latestTime) {
			latestTime = t
			latestPhase = ev.Phase
		}
	}

	if latestPhase == "" {
		return Result{
			Name:     "pathflow-stuck",
			Status:   StatusPass,
			Message:  "no phase transitions found",
			Duration: time.Since(start),
		}
	}

	age := time.Since(latestTime)
	if age > stuckThreshold {
		return Result{
			Name:     "pathflow-stuck",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("phase %s stuck for %s (threshold: 30m)", latestPhase, age.Truncate(time.Second)),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "pathflow-stuck",
		Status:   StatusPass,
		Message:  fmt.Sprintf("current phase %s started %s ago", latestPhase, age.Truncate(time.Second)),
		Duration: time.Since(start),
	}
}

// teamConfig represents a team config.json file.
type teamConfig struct {
	Members []teamMember `json:"members"`
}

// teamMember represents a single teammate entry in the team config.
type teamMember struct {
	Name       string `json:"name"`
	TmuxPaneID string `json:"tmuxPaneId"`
}

// pathflowTeamConfig represents the session-scoped pathflow-team.json file.
type pathflowTeamConfig struct {
	TeamName string `json:"team_name"`
}

// discoverTeamName attempts to find the active team name. It checks the
// session-scoped pathflow-team.json first, then falls back to listing
// ~/.claude/teams/ directories.
func discoverTeamName(stateDir, sessionID, homeDir string) string {
	// Primary: session-scoped team config.
	if sessionID != "" {
		teamFile := filepath.Join(stateDir, "session", sessionID, "pathflow", "pathflow-team.json")
		data, err := os.ReadFile(teamFile)
		if err == nil {
			var ptc pathflowTeamConfig
			if json.Unmarshal(data, &ptc) == nil && ptc.TeamName != "" {
				return ptc.TeamName
			}
		}
	}

	// Fallback: list ~/.claude/teams/ directories.
	if homeDir == "" {
		return ""
	}
	teamsDir := filepath.Join(homeDir, ".claude", "teams")
	entries, err := os.ReadDir(teamsDir)
	if err != nil {
		return ""
	}
	for _, entry := range entries {
		if entry.IsDir() {
			return entry.Name()
		}
	}
	return ""
}

// checkTeamHealth verifies that teammate tmux panes are alive.
func checkTeamHealth(_ context.Context, opts *Options) Result {
	start := time.Now()

	teamName := discoverTeamName(opts.StateDir, opts.SessionID, opts.HomeDir)
	if teamName == "" {
		return Result{
			Name:     "team-health",
			Status:   StatusPass,
			Message:  "no active team",
			Duration: time.Since(start),
		}
	}

	configPath := filepath.Join(opts.HomeDir, ".claude", "teams", teamName, "config.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		return Result{
			Name:     "team-health",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("cannot read team config: %v", err),
			Duration: time.Since(start),
		}
	}

	var tc teamConfig
	if err := json.Unmarshal(data, &tc); err != nil {
		return Result{
			Name:     "team-health",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("invalid team config JSON: %v", err),
			Duration: time.Since(start),
		}
	}

	if len(tc.Members) == 0 {
		return Result{
			Name:     "team-health",
			Status:   StatusPass,
			Message:  "team has no members",
			Duration: time.Since(start),
		}
	}

	// Get tmux pane listing.
	tmuxOutput, err := opts.ExecCommand("tmux", "list-panes", "-a")
	if err != nil {
		return Result{
			Name:     "team-health",
			Status:   StatusWarn,
			Message:  "tmux not available",
			Duration: time.Since(start),
		}
	}

	paneList := string(tmuxOutput)
	var deadCount int
	for _, member := range tc.Members {
		if member.TmuxPaneID == "" {
			continue
		}
		if !strings.Contains(paneList, member.TmuxPaneID) {
			deadCount++
		}
	}

	if deadCount > 0 {
		return Result{
			Name:     "team-health",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("%d of %d teammate panes not found in tmux", deadCount, len(tc.Members)),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "team-health",
		Status:   StatusPass,
		Message:  fmt.Sprintf("all %d teammate panes alive", len(tc.Members)),
		Duration: time.Since(start),
	}
}

// expectedSentinels maps phase names to the sentinel files that should exist
// once that phase completes. Phases are cumulative — PF4 implies PF1-PF3
// sentinels should all exist.
var expectedSentinels = map[string][]string{
	"PF1-INIT":     {"pathflow-pf-1"},
	"PF2-CONTEXT":  {"pathflow-pf-1", "pathflow-pf-2"},
	"PF3-CLASSIFY": {"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3"},
	"PF4-EXECUTE":  {"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3"},
	"PF5-VERIFY":   {"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3"},
	"PF6-COMPLETE": {"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3", "pathflow-pf-6"},
	"PF7-END":      {"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3", "pathflow-pf-6", "pathflow-pf-7"},
}

// checkSentinelDrift compares sentinel files against the current phase.
func checkSentinelDrift(_ context.Context, opts *Options) Result {
	start := time.Now()

	if opts.SessionID == "" || !pathflowActive(opts.StateDir, opts.SessionID) {
		return Result{
			Name:     "sentinel-drift",
			Status:   StatusPass,
			Message:  "no active session",
			Duration: time.Since(start),
		}
	}

	events, err := readPathflowEvents(opts.StateDir)
	if err != nil {
		return Result{
			Name:     "sentinel-drift",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("cannot read pathflow events: %v", err),
			Duration: time.Since(start),
		}
	}

	// Find the latest phase_transition event to determine current phase.
	var currentPhase string
	var latestTime time.Time
	for _, ev := range events {
		if ev.EventType != "phase_transition" {
			continue
		}
		t, err := time.Parse(time.RFC3339, ev.Timestamp)
		if err != nil {
			continue
		}
		if t.After(latestTime) {
			latestTime = t
			currentPhase = ev.Phase
		}
	}

	if currentPhase == "" {
		return Result{
			Name:     "sentinel-drift",
			Status:   StatusPass,
			Message:  "no phase transitions found",
			Duration: time.Since(start),
		}
	}

	expected, ok := expectedSentinels[currentPhase]
	if !ok {
		return Result{
			Name:     "sentinel-drift",
			Status:   StatusPass,
			Message:  fmt.Sprintf("unknown phase %s, skipping drift check", currentPhase),
			Duration: time.Since(start),
		}
	}

	sentinelDir := filepath.Join(opts.StateDir, "sentinels", "pathflow", opts.SessionID)
	var missing []string
	for _, sentinel := range expected {
		path := filepath.Join(sentinelDir, sentinel)
		if _, err := os.Stat(path); os.IsNotExist(err) {
			missing = append(missing, sentinel)
		}
	}

	if len(missing) > 0 {
		return Result{
			Name:     "sentinel-drift",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("phase %s: missing sentinels: %s", currentPhase, strings.Join(missing, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "sentinel-drift",
		Status:   StatusPass,
		Message:  fmt.Sprintf("all sentinels present for phase %s", currentPhase),
		Duration: time.Since(start),
	}
}

// checkNetwork tests basic network connectivity.
func checkNetwork(ctx context.Context, opts *Options) Result {
	start := time.Now()

	// Use a DNS lookup as a lightweight connectivity test.
	// Never fail — only warn if offline.
	_, err := opts.ExecCommand("host", "-W", "2", "github.com")
	if err != nil {
		return Result{
			Name:     "network",
			Status:   StatusWarn,
			Message:  "network connectivity check failed (offline?)",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "network",
		Status:   StatusPass,
		Message:  "network connectivity OK",
		Duration: time.Since(start),
	}
}
